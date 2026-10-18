//! axum app assembly (masterplan.md §4.6).
#![forbid(unsafe_code)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

use axum::extract::{Query, State};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use latex_ai_core::ai::EditPlanner;
use latex_ai_core::metrics::Metrics;
use latex_ai_core::search::{Hit, fuzzy_rank};
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Arc;
use tower_http::trace::TraceLayer;

pub(crate) mod agent;
pub mod bootstrap;
pub mod error;
pub mod jobs;
pub mod proposals;
pub mod routes;
pub(crate) mod sse;

#[derive(Clone)]
pub struct AppState {
    /// Object-safe so tests can inject a scripted planner (masterplan §12 M2).
    pub planner: Arc<dyn EditPlanner>,
    pub compiler: Arc<latex_ai_compiler::Compiler>,
    pub store: Arc<latex_ai_core::storage::Store>,
    pub jobs: Arc<jobs::JobRegistry>,
    /// Verified proposals held for review, with the base they were made on.
    pub proposals: Arc<proposals::ProposalRegistry>,
    /// Canonicalized workspace root every user-supplied path is sandboxed to.
    pub workspace_root: PathBuf,
    /// Hot-swappable configuration; routes call `snapshot()` once per request.
    pub settings: Arc<latex_ai_core::config::Settings>,
    /// Process-lifetime counters, surfaced by `GET /api/metrics`.
    pub metrics: Arc<Metrics>,
}

use std::env;
use tower_http::cors::{Any, CorsLayer};

/// Why the server refused to start.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum StartupError {
    #[error(
        "ENV=production requires ALLOWED_ORIGINS to be set to a comma-separated \
         list of origins (e.g. ALLOWED_ORIGINS=https://latex.example.com). \
         Refusing to start with the permissive development default."
    )]
    MissingAllowedOrigins,
    #[error("ALLOWED_ORIGINS contains no parseable origin: {0:?}")]
    UnparseableAllowedOrigins(String),
}

const DEV_ORIGINS: &str = "http://localhost:5173,http://127.0.0.1:5173,\
tauri://localhost,http://tauri.localhost,https://tauri.localhost";

/// Hostnames that are always valid in a `Host` header: the loopback names a
/// local browser or the Tauri shell uses.
const LOOPBACK_HOSTS: &[&str] = &["localhost", "127.0.0.1", "[::1]", "tauri.localhost"];

/// Hosts the server answers to: loopback, anything named in
/// `ALLOWED_ORIGINS` (a production deployment's own domain) and anything in
/// the comma-separated `ALLOWED_HOSTS`.
fn allowed_hosts() -> Vec<String> {
    let mut hosts: Vec<String> = LOOPBACK_HOSTS.iter().map(|h| (*h).to_owned()).collect();
    if let Ok(origins) = env::var("ALLOWED_ORIGINS") {
        for origin in origins.split(',') {
            if let Some(host) = origin
                .trim()
                .split_once("://")
                .map(|(_, rest)| rest.split('/').next().unwrap_or(rest))
                .and_then(|authority| host_without_port(authority))
            {
                hosts.push(host.to_ascii_lowercase());
            }
        }
    }
    if let Ok(extra) = env::var("ALLOWED_HOSTS") {
        hosts.extend(
            extra
                .split(',')
                .map(|h| h.trim().to_ascii_lowercase())
                .filter(|h| !h.is_empty()),
        );
    }
    hosts
}

/// `example.com:3000` -> `example.com`; `[::1]:3000` -> `[::1]`.
fn host_without_port(authority: &str) -> Option<&str> {
    if authority.is_empty() {
        return None;
    }
    if authority.starts_with('[') {
        let end = authority.find(']')?;
        return Some(&authority[..=end]);
    }
    Some(authority.split(':').next().unwrap_or(authority))
}

/// Reject requests whose `Host` is not one we serve.
///
/// A malicious page can make a victim's browser resolve
/// `attacker.example` to `127.0.0.1` (DNS rebinding) and then talk to this
/// unauthenticated server as a same-origin site — CORS does not apply. The
/// browser still sends the attacker's name in `Host`, which is what this
/// check refuses. A missing `Host` (HTTP/1.0 tooling) cannot come from a
/// browser and is let through.
async fn require_known_host(
    axum::extract::State(allowed): axum::extract::State<Arc<Vec<String>>>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    let host = request
        .headers()
        .get(axum::http::header::HOST)
        .and_then(|v| v.to_str().ok())
        .and_then(host_without_port)
        .map(str::to_ascii_lowercase);

    match host {
        Some(h) if !allowed.contains(&h) => {
            tracing::warn!(host = %h, "rejected request with unknown Host header");
            (
                axum::http::StatusCode::FORBIDDEN,
                Json(json!({ "error": "forbidden", "message": "unknown Host header" })),
            )
                .into_response()
        }
        _ => next.run(request).await,
    }
}

/// Resolve the CORS allow-list.
///
/// In production a missing or unusable `ALLOWED_ORIGINS` is fatal: silently
/// falling back to the dev origins would leave a deployment trusting
/// localhost (masterplan §11.6, §12 M5 audit: security).
fn cors_layer() -> Result<CorsLayer, StartupError> {
    let is_production = env::var("ENV").is_ok_and(|e| e.eq_ignore_ascii_case("production"));
    let configured = env::var("ALLOWED_ORIGINS")
        .ok()
        .filter(|v| !v.trim().is_empty());

    let raw = match (configured, is_production) {
        (Some(v), _) => v,
        (None, true) => return Err(StartupError::MissingAllowedOrigins),
        // Vite dev server, plus the origins a Tauri webview serves the SPA
        // from (`tauri://` on macOS/Linux, `http(s)://tauri.localhost` on
        // Windows). Without them the desktop app's every API call is a
        // blocked cross-origin request. Production never gets these: it
        // must name its origins explicitly.
        (None, false) => DEV_ORIGINS.to_owned(),
    };

    let origins: Vec<_> = raw
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse().ok())
        .collect();

    if origins.is_empty() {
        return Err(StartupError::UnparseableAllowedOrigins(raw));
    }

    Ok(CorsLayer::new()
        .allow_origin(origins)
        .allow_methods(Any)
        .allow_headers(Any))
}

/// Build the full router (masterplan.md §4.6, §8.3).
///
/// # Errors
/// [`StartupError`] when the CORS configuration is unusable — see
/// [`cors_layer`].
pub fn build_router(state: AppState) -> Result<Router, StartupError> {
    let cors = cors_layer()?;

    Ok(Router::new()
        .route("/health", get(health))
        .route("/api/search", post(search_candidates).get(search_workspace))
        .route(
            "/api/config",
            get(routes::config::get_config).patch(routes::config::patch_config),
        )
        .route(
            "/api/config/api-key",
            put(routes::config::put_api_key).delete(routes::config::delete_api_key),
        )
        .route("/api/history", get(routes::history::list_history))
        .route("/api/files", get(routes::files::list_files))
        .route("/api/tree", get(routes::files::get_tree))
        .route(
            "/api/file",
            get(routes::files::read_file)
                .post(routes::files::write_file)
                .delete(routes::manage::delete_file),
        )
        .route("/api/files/create", post(routes::manage::create_file))
        .route("/api/files/rename", post(routes::manage::rename))
        .route(
            "/api/upload",
            post(routes::manage::upload).layer(axum::extract::DefaultBodyLimit::max(
                routes::manage::MAX_UPLOAD_BYTES,
            )),
        )
        .route("/api/raw", get(routes::manage::raw))
        .route(
            "/api/project",
            get(routes::manage::get_project).put(routes::manage::put_project),
        )
        .route("/api/compile", post(routes::compile::compile))
        .route(
            "/api/compile/{id}/cancel",
            post(routes::compile::cancel_compile),
        )
        .route("/api/builds/{id}", get(routes::compile::get_build))
        .route("/api/builds/{id}/pdf", get(routes::compile::export_pdf))
        .route("/api/builds/{id}/log", get(routes::compile::get_build_log))
        .route(
            "/api/compile/stream/{id}",
            get(routes::compile::compile_stream),
        )
        .route("/api/plan/stream", post(routes::agent::plan_stream))
        .route("/api/agent/stream", post(routes::agent::agent_loop_stream))
        .route("/api/agent/preview", post(routes::agent::preview))
        .route(
            "/api/agent/apply-selected",
            post(routes::agent::apply_selected),
        )
        .route("/api/visual-fix", post(routes::visual::visual_fix))
        .route("/api/pdf/page", get(routes::visual::get_pdf_page))
        .route("/api/metrics", get(routes::metrics::get_metrics))
        // Counting sits inside CORS/trace so it sees exactly the requests
        // that reach a route, once each (§12 M5 audit: logic correctness).
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            routes::metrics::count_request,
        ))
        .layer(TraceLayer::new_for_http())
        .layer(cors)
        // Outermost: a rebinding request is refused before anything else
        // (including CORS and metrics) sees it.
        .layer(axum::middleware::from_fn_with_state(
            Arc::new(allowed_hosts()),
            require_known_host,
        ))
        .with_state(state))
}

async fn health() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

/// `POST /api/search` body: caller supplies the candidate list (command
/// palette entries). Interim contract from masterplan §8.3.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SearchRequest {
    query: String,
    candidates: Vec<String>,
}

#[derive(serde::Serialize)]
struct SearchResponse {
    hits: Vec<Hit>,
}

async fn search_candidates(Json(req): Json<SearchRequest>) -> Json<SearchResponse> {
    Json(SearchResponse {
        hits: fuzzy_rank(&req.query, &req.candidates),
    })
}

/// `GET /api/search?q=` — fuzzy search over the server-owned workspace file
/// list (masterplan §8.3 final form; the server owns the list via
/// `/api/files`' walker).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkspaceSearchQuery {
    q: String,
}

async fn search_workspace(
    State(state): State<AppState>,
    Query(query): Query<WorkspaceSearchQuery>,
) -> Json<SearchResponse> {
    let root = state.workspace_root.clone();
    let candidates =
        tokio::task::spawn_blocking(move || routes::files::list_workspace_files(&root))
            .await
            .unwrap_or_default();

    Json(SearchResponse {
        hits: fuzzy_rank(&query.q, &candidates),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use latex_ai_core::ai::{AiError, PlanFuture, PlanRequest};
    use latex_ai_core::compile::AgentEdit;
    use tokio_util::sync::CancellationToken;
    use tower::ServiceExt;

    /// Planner that always errors — router tests never reach the LLM.
    struct NoPlanner;
    impl EditPlanner for NoPlanner {
        fn plan_edits<'a>(
            &'a self,
            _req: PlanRequest<'a>,
            _cancel: CancellationToken,
        ) -> PlanFuture<'a> {
            Box::pin(async { Err::<Vec<AgentEdit>, _>(AiError::EmptyResponse) })
        }
        fn visual_fix<'a>(
            &'a self,
            _req: latex_ai_core::ai::VisualFixRequest<'a>,
            _cancel: CancellationToken,
        ) -> PlanFuture<'a> {
            Box::pin(async { Err::<Vec<AgentEdit>, _>(AiError::EmptyResponse) })
        }
    }

    async fn test_state() -> (AppState, tempfile::TempDir) {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                file_path TEXT NOT NULL,
                command TEXT NOT NULL,
                new_text TEXT NOT NULL,
                diff TEXT NOT NULL,
                edit_type TEXT NOT NULL,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP
            )",
        )
        .execute(&pool)
        .await
        .unwrap();

        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();

        let state = AppState {
            planner: Arc::new(NoPlanner),
            // These unit tests never compile, so the worker path is moot.
            compiler: Arc::new(latex_ai_compiler::Compiler::new(
                latex_ai_compiler::CompilerConfig::new(
                    "/nonexistent-worker".into(),
                    root.join(".builds"),
                ),
            )),
            store: Arc::new(latex_ai_core::storage::Store::new(pool, root.clone())),
            jobs: Arc::new(jobs::JobRegistry::default()),
            proposals: Arc::new(proposals::ProposalRegistry::default()),
            workspace_root: root,
            settings: Arc::new(
                latex_ai_core::config::Settings::open(
                    tempfile::tempdir().unwrap().keep(),
                    latex_ai_core::config::EnvOverrides::default(),
                )
                .unwrap(),
            ),
            metrics: Arc::new(latex_ai_core::metrics::Metrics::new()),
        };
        (state, dir)
    }

    #[tokio::test]
    async fn health_returns_ok() {
        let (state, _dir) = test_state().await;
        let app = build_router(state).unwrap();
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }

    async fn status_for_host(host: Option<&str>) -> StatusCode {
        let (state, _dir) = test_state().await;
        let app = build_router(state).unwrap();
        let mut request = Request::builder().uri("/health");
        if let Some(h) = host {
            request = request.header("host", h);
        }
        app.oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap()
            .status()
    }

    #[tokio::test]
    async fn rebinding_host_headers_are_refused() {
        assert_eq!(
            status_for_host(Some("evil.example")).await,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            status_for_host(Some("evil.example:3000")).await,
            StatusCode::FORBIDDEN
        );
        // A look-alike must not match by prefix or suffix.
        assert_eq!(
            status_for_host(Some("localhost.evil.example")).await,
            StatusCode::FORBIDDEN
        );
    }

    #[tokio::test]
    async fn loopback_hosts_and_hostless_requests_are_served() {
        for host in [
            "localhost",
            "localhost:3000",
            "127.0.0.1:3000",
            "[::1]:3000",
            "tauri.localhost",
        ] {
            assert_eq!(status_for_host(Some(host)).await, StatusCode::OK, "{host}");
        }
        assert_eq!(status_for_host(None).await, StatusCode::OK);
    }

    #[test]
    fn host_parsing_handles_ports_and_ipv6() {
        assert_eq!(host_without_port("example.com:8080"), Some("example.com"));
        assert_eq!(host_without_port("example.com"), Some("example.com"));
        assert_eq!(host_without_port("[::1]:3000"), Some("[::1]"));
        assert_eq!(host_without_port(""), None);
    }

    #[tokio::test]
    async fn tauri_origin_passes_the_cors_preflight() {
        let (state, _dir) = test_state().await;
        let app = build_router(state).unwrap();
        let res = app
            .oneshot(
                Request::builder()
                    .method("OPTIONS")
                    .uri("/api/files")
                    .header("origin", "tauri://localhost")
                    .header("access-control-request-method", "GET")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            res.headers()
                .get("access-control-allow-origin")
                .and_then(|v| v.to_str().ok()),
            Some("tauri://localhost")
        );
    }

    #[tokio::test]
    async fn search_post_ranks_best_match_first() {
        let (state, _dir) = test_state().await;
        let app = build_router(state).unwrap();
        let body = json!({
            "query": "librs",
            "candidates": ["src/main.rs", "src/lib.rs", "README.md"]
        });
        let res = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/search")
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let parsed: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(parsed["hits"][0]["value"], "src/lib.rs");
    }

    #[tokio::test]
    async fn search_get_ranks_workspace_files() {
        let (state, dir) = test_state().await;
        std::fs::write(dir.path().join("main.tex"), "x").unwrap();
        std::fs::write(dir.path().join("refs.bib"), "x").unwrap();
        let app = build_router(state).unwrap();

        let res = app
            .oneshot(
                Request::builder()
                    .uri("/api/search?q=maintex")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let parsed: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(parsed["hits"][0]["value"], "main.tex");
    }
}
