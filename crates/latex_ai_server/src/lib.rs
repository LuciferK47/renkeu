//! axum app assembly (masterplan.md §4.6).
#![forbid(unsafe_code)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

use axum::extract::{Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use latex_ai_core::ai::EditPlanner;
use latex_ai_core::metrics::Metrics;
use latex_ai_core::search::{Hit, fuzzy_rank};
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Arc;
use tower_http::trace::TraceLayer;

pub mod error;
pub mod jobs;
pub mod routes;
pub(crate) mod sse;

#[derive(Clone)]
pub struct AppState {
    /// Object-safe so tests can inject a scripted planner (masterplan §12 M2).
    pub planner: Arc<dyn EditPlanner>,
    pub queue: Arc<latex_ai_compiler::queue::CompilationQueue>,
    pub store: Arc<latex_ai_core::storage::Store>,
    pub jobs: Arc<jobs::JobRegistry>,
    /// Canonicalized workspace root every user-supplied path is sandboxed to.
    pub workspace_root: PathBuf,
    /// Planner model id, surfaced by `GET /api/config`.
    pub model: String,
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
        (None, false) => "http://localhost:5173,http://127.0.0.1:5173".to_owned(),
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
        .route("/api/config", get(routes::config::get_config))
        .route("/api/history", get(routes::history::list_history))
        .route("/api/files", get(routes::files::list_files))
        .route(
            "/api/file",
            get(routes::files::read_file).post(routes::files::write_file),
        )
        .route("/api/compile", post(routes::compile::compile))
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
    use latex_ai_core::ai::{AiError, PlanFuture};
    use latex_ai_core::compile::AgentEdit;
    use tokio_util::sync::CancellationToken;
    use tower::ServiceExt;

    /// Planner that always errors — router tests never reach the LLM.
    struct NoPlanner;
    impl EditPlanner for NoPlanner {
        fn plan_edits<'a>(
            &'a self,
            _instruction: &'a str,
            _file_content: &'a str,
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
        let engine = latex_ai_compiler::Engine::new(2);
        let queue = Arc::new(latex_ai_compiler::queue::CompilationQueue::new(Arc::new(
            engine,
        )));

        let state = AppState {
            planner: Arc::new(NoPlanner),
            queue,
            store: Arc::new(latex_ai_core::storage::Store::new(pool, root.clone())),
            jobs: Arc::new(jobs::JobRegistry::default()),
            workspace_root: root,
            model: "test-model".to_owned(),
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
