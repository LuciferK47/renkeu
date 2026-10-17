//! M5 acceptance tests (masterplan.md §12 M5): hardening.
//!
//! - production refuses to start without `ALLOWED_ORIGINS` (audit: security);
//! - `/api/metrics` is non-zero after a fixture request (acceptance), in both
//!   JSON and Prometheus form;
//! - each routed request is counted exactly once (audit: logic correctness);
//! - the LLM counter increments even when the call fails, because it is
//!   recorded before the request is issued (audit: integration).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use latex_ai_core::ai::{AiError, EditPlanner, PlanFuture, VisualFixRequest, PlanRequest};
use latex_ai_server::{AppState, build_router, jobs::JobRegistry};
use serde_json::{Value, json};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;

/// Env vars are process-global. `cargo-nextest` runs each test in its own
/// process, but `cargo test` shares one, so these tests serialise around this
/// lock — and never hold it across an await, which is why the app is built
/// before it is taken.
static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Set the CORS-relevant env, run `build_router`, restore the previous env.
/// Entirely synchronous, so the guard is never held across an await.
fn with_cors_env(
    env: Option<&str>,
    origins: Option<&str>,
    state: AppState,
) -> Result<axum::Router, latex_ai_server::StartupError> {
    let _lock = ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _guard = EnvGuard::capture();
    unsafe {
        match env {
            Some(v) => std::env::set_var("ENV", v),
            None => std::env::remove_var("ENV"),
        }
        match origins {
            Some(v) => std::env::set_var("ALLOWED_ORIGINS", v),
            None => std::env::remove_var("ALLOWED_ORIGINS"),
        }
    }
    build_router(state)
}

struct FailingPlanner;
impl EditPlanner for FailingPlanner {
    fn plan_edits<'a>(&'a self, _req: PlanRequest<'a>, _cancel: CancellationToken) -> PlanFuture<'a> {
        Box::pin(async { Err(AiError::Request("provider unavailable".into())) })
    }
    fn visual_fix<'a>(
        &'a self,
        _req: VisualFixRequest<'a>,
        _cancel: CancellationToken,
    ) -> PlanFuture<'a> {
        Box::pin(async { Err(AiError::EmptyResponse) })
    }
}

struct TestApp {
    state: AppState,
    _workspace: tempfile::TempDir,
    pool: sqlx::SqlitePool,
}

async fn test_app() -> TestApp {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::query(
        "CREATE TABLE history (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            file_path TEXT NOT NULL, command TEXT NOT NULL, new_text TEXT NOT NULL,
            diff TEXT NOT NULL, edit_type TEXT NOT NULL,
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP)",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "CREATE TABLE compile_queue (
            job_id TEXT PRIMARY KEY, file_path TEXT NOT NULL, status TEXT NOT NULL,
            error TEXT, created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
            completed_at DATETIME)",
    )
    .execute(&pool)
    .await
    .unwrap();

    let workspace = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(workspace.path()).unwrap();
    std::fs::write(
        root.join("main.tex"),
        "\\documentclass{article}\n\\begin{document}x\\end{document}\n",
    )
    .unwrap();

    TestApp {
        state: AppState {
            planner: Arc::new(FailingPlanner),
            compiler: Arc::new(latex_ai_compiler::Compiler::new(latex_ai_compiler::CompilerConfig::new(
                env!("CARGO_BIN_EXE_latex-ai").into(),
                tempfile::tempdir().unwrap().keep(),
            ))),
            store: Arc::new(latex_ai_core::storage::Store::new(
                pool.clone(),
                root.clone(),
            )),
            jobs: Arc::new(JobRegistry::default()),
            proposals: Arc::new(latex_ai_server::proposals::ProposalRegistry::default()),
            workspace_root: root,
            settings: Arc::new(
                latex_ai_core::config::Settings::open(
                    tempfile::tempdir().unwrap().keep(),
                    latex_ai_core::config::EnvOverrides::default(),
                )
                .unwrap(),
            ),
            metrics: Arc::new(latex_ai_core::metrics::Metrics::new()),
        },
        _workspace: workspace,
        pool,
    }
}

async fn body_json(res: axum::response::Response) -> Value {
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

async fn body_text(res: axum::response::Response) -> String {
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    String::from_utf8(bytes.to_vec()).unwrap()
}

/// Restores whatever the env looked like before, so tests do not leak state.
struct EnvGuard {
    env: Option<String>,
    origins: Option<String>,
}

impl EnvGuard {
    fn capture() -> Self {
        Self {
            env: std::env::var("ENV").ok(),
            origins: std::env::var("ALLOWED_ORIGINS").ok(),
        }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        unsafe {
            match &self.env {
                Some(v) => std::env::set_var("ENV", v),
                None => std::env::remove_var("ENV"),
            }
            match &self.origins {
                Some(v) => std::env::set_var("ALLOWED_ORIGINS", v),
                None => std::env::remove_var("ALLOWED_ORIGINS"),
            }
        }
    }
}

// ------------------------------------------------------------------- CORS

#[tokio::test]
async fn production_refuses_to_start_without_allowed_origins() {
    let app = test_app().await;
    let err = with_cors_env(Some("production"), None, app.state.clone())
        .expect_err("production without ALLOWED_ORIGINS must not start");

    let message = err.to_string();
    assert!(
        message.contains("ALLOWED_ORIGINS"),
        "the error must name the variable: {message}"
    );
    assert!(
        message.contains("production"),
        "and say why it matters: {message}"
    );
}

#[tokio::test]
async fn production_with_origins_starts() {
    let app = test_app().await;
    assert!(
        with_cors_env(
            Some("production"),
            Some("https://latex.example.com"),
            app.state.clone()
        )
        .is_ok()
    );
}

#[tokio::test]
async fn an_empty_allowed_origins_is_treated_as_missing() {
    // A blank value is a misconfiguration, not an opt-out.
    let app = test_app().await;
    assert!(with_cors_env(Some("production"), Some("   "), app.state.clone()).is_err());
}

#[tokio::test]
async fn development_still_gets_the_permissive_default() {
    let app = test_app().await;
    assert!(
        with_cors_env(None, None, app.state.clone()).is_ok(),
        "dev must not need configuration"
    );
}

// ---------------------------------------------------------------- metrics

#[tokio::test]
async fn metrics_are_non_zero_after_a_fixture_request() {
    let app = test_app().await;
    let router = build_router(app.state.clone()).unwrap();

    // Start from zero.
    let res = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/metrics")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(body_json(res).await["requests_total"], 1, "counts itself");

    // A fixture request.
    let res = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let res = router
        .oneshot(
            Request::builder()
                .uri("/api/metrics")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = body_json(res).await;
    assert_eq!(body["requests_total"], 3, "two metrics reads plus /health");
    assert_eq!(body["responses_failed"], 0);
}

#[tokio::test]
async fn each_request_is_counted_exactly_once() {
    // masterplan §12 M5 audit (logic correctness).
    let app = test_app().await;
    let router = build_router(app.state.clone()).unwrap();

    for _ in 0..5 {
        let res = router
            .clone()
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

    assert_eq!(app.state.metrics.snapshot().requests_total, 5);
}

#[tokio::test]
async fn failed_responses_are_counted_separately() {
    let app = test_app().await;
    let router = build_router(app.state.clone()).unwrap();

    let res = router
        .oneshot(
            Request::builder()
                .uri("/api/file?path=../../etc/passwd")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);

    let snapshot = app.state.metrics.snapshot();
    assert_eq!(snapshot.requests_total, 1);
    assert_eq!(snapshot.responses_failed, 1);
}

#[tokio::test]
async fn the_llm_counter_increments_even_when_the_call_fails() {
    // masterplan §12 M5 audit (integration): counted BEFORE the call returns,
    // so a failing or in-flight provider is still visible.
    let app = test_app().await;
    assert_eq!(app.state.metrics.snapshot().llm_calls, 0);

    let res = build_router(app.state.clone())
        .unwrap()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/plan/stream")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({ "file_path": "main.tex", "instruction": "do something" }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    // Drain the stream so the spawned task has run.
    let _ = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();

    assert_eq!(
        app.state.metrics.snapshot().llm_calls,
        1,
        "the planner failed, but the attempt must still be counted"
    );
}

#[tokio::test]
async fn agent_runs_and_iterations_are_counted() {
    let app = test_app().await;
    let res = build_router(app.state.clone())
        .unwrap()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/agent/stream")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({ "file_path": "main.tex", "instruction": "fix it" }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let _ = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();

    let snapshot = app.state.metrics.snapshot();
    assert_eq!(snapshot.agent_runs, 1);
    assert!(snapshot.agent_iterations >= 1);
}

#[tokio::test]
async fn metrics_are_available_in_prometheus_form() {
    let app = test_app().await;
    let res = build_router(app.state.clone())
        .unwrap()
        .oneshot(
            Request::builder()
                .uri("/api/metrics")
                .header(header::ACCEPT, "text/plain")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    assert!(
        res.headers()
            .get(header::CONTENT_TYPE)
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("text/plain")
    );

    let text = body_text(res).await;
    assert!(text.contains("# TYPE latex_ai_requests_total counter"));
    assert!(text.contains("latex_ai_llm_calls_total"));
}

// ------------------------------------------------------------ persistence

#[tokio::test]
async fn a_compile_job_is_persisted_and_closed_out() {
    // masterplan §12 M5: the durable row is what makes crash recovery
    // possible, so the route must actually write it.
    let app = test_app().await;
    let router = build_router(app.state.clone()).unwrap();

    let res = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/compile")
                .header("content-type", "application/json")
                .body(Body::from(json!({ "file_path": "main.tex" }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let job_id = body_json(res).await["job_id"].as_str().unwrap().to_owned();

    // Drain the job stream so the compile has finished.
    let res = router
        .oneshot(
            Request::builder()
                .uri(format!("/api/compile/stream/{job_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let _ = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();

    let rows = latex_ai_core::storage::jobs::recent(&app.pool, 10)
        .await
        .unwrap();
    let row = rows
        .iter()
        .find(|r| r.job_id == job_id)
        .expect("the job must be persisted");
    assert_eq!(row.file_path, "main.tex");
    assert_ne!(
        row.status, "running",
        "a finished job must not stay running"
    );
    assert!(row.completed_at.is_some());
}

#[tokio::test]
async fn recovery_marks_an_interrupted_job_abandoned() {
    use latex_ai_core::storage::jobs;

    let app = test_app().await;
    jobs::record_started(&app.pool, "left-running", "main.tex")
        .await
        .unwrap();

    // What startup does after a crash.
    assert_eq!(jobs::recover_abandoned(&app.pool).await.unwrap(), 1);

    let rows = jobs::recent(&app.pool, 10).await.unwrap();
    let row = rows.iter().find(|r| r.job_id == "left-running").unwrap();
    assert_eq!(row.status, "abandoned");
}
