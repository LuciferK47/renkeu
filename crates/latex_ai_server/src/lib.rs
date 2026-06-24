//! axum app assembly. Routes: `/health`, `/api/search`.
#![forbid(unsafe_code)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

use axum::routing::{get, post};
use axum::{Json, Router};
use latex_ai_core::search::{Hit, fuzzy_rank};
use serde::Deserialize;
use serde_json::{Value, json};
use tower_http::trace::TraceLayer;

/// Build the router. Future milestones merge feature routers here
/// (masterplan.md §4.6).
pub fn build_router() -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/api/search", post(search))
        .layer(TraceLayer::new_for_http())
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

    #[tokio::test]
    async fn health_returns_ok() {
        let app = build_router();
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
        let app = build_router();
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
