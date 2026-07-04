//! axum app assembly. Routes: `/health`, `/api/search`.
#![forbid(unsafe_code)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

use axum::{routing::{get, post}, Json, Router};
use latex_ai_core::search::{Hit, fuzzy_rank};
use serde::Deserialize;
use serde_json::{Value, json};
use tower_http::trace::TraceLayer;
use std::sync::Arc;

pub mod routes;
pub mod error;

#[derive(Clone)]
pub struct AppState {
    pub planner: Arc<latex_ai_core::ai::GeminiPlanner>,
    pub compiler: latex_ai_compiler::Engine,
    pub queue: Arc<latex_ai_compiler::queue::CompilationQueue>,
    pub store: Arc<latex_ai_core::storage::Store>,
}

use tower_http::cors::{CorsLayer, Any};
use std::env;

/// Build the router. Future milestones merge feature routers here
/// (masterplan.md §4.6).
pub fn build_router(state: AppState) -> Router {
    let allowed_origins = env::var("ALLOWED_ORIGINS").unwrap_or_else(|_| "http://localhost:5173,http://127.0.0.1:5173".to_string());
    let origins: Vec<_> = allowed_origins.split(',').filter_map(|s| s.parse().ok()).collect();

    let cors = CorsLayer::new()
        .allow_origin(origins)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        .route("/health", get(health))
        .route("/api/search", post(search))
        .route("/api/config", get(routes::config::get_config))
        .route("/api/files", get(routes::files::list_files))
        .route("/api/compile", post(routes::compile::compile))
        .route("/api/compile/stream/{id}", get(routes::compile::compile_stream))
        .route("/api/plan/stream", post(routes::agent::plan_stream))
        .route("/api/agent/stream", post(routes::agent::agent_loop_stream))
        .route("/api/agent/apply-selected", post(routes::agent::apply_selected))
        .route("/api/visual-fix", post(routes::visual::visual_fix))
        .route("/api/pdf/page", get(routes::visual::get_pdf_page))
        .layer(TraceLayer::new_for_http())
        .layer(cors)
        .with_state(state)
}

async fn health() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

/// Fuzzy-search request. ponytail: candidates are passed in the body for now
/// (interim POST). Switches to `GET /api/search?q=` once the server owns the
/// workspace file list — masterplan.md §8.3.
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

async fn search(Json(req): Json<SearchRequest>) -> Json<SearchResponse> {
    Json(SearchResponse {
        hits: fuzzy_rank(&req.query, &req.candidates),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;
    use latex_ai_core::ai::GeminiPlanner;

    async fn test_state() -> AppState {
        let db_url = "sqlite::memory:";
        let pool = sqlx::sqlite::SqlitePoolOptions::new().connect(db_url).await.unwrap();
        
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                file_path TEXT NOT NULL,
                command TEXT NOT NULL,
                new_text TEXT NOT NULL,
                diff TEXT NOT NULL,
                edit_type TEXT NOT NULL,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP
            )"
        ).execute(&pool).await.unwrap();

        let engine = latex_ai_compiler::Engine::new(2);
        let queue = Arc::new(latex_ai_compiler::queue::CompilationQueue::new(Arc::new(engine.clone())));
        
        AppState {
            planner: Arc::new(GeminiPlanner::new("gemini-2.5-flash")),
            compiler: engine,
            queue,
            store: Arc::new(latex_ai_core::storage::Store::new(pool, std::env::current_dir().unwrap())),
        }
    }

    #[tokio::test]
    async fn health_returns_ok() {
        let app = build_router(test_state().await);
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
    async fn search_ranks_best_match_first() {
        let app = build_router(test_state().await);
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
}
