//! Counter exposition and the middleware that feeds it (masterplan.md §12 M5).

use crate::AppState;
use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use tracing::instrument;

/// Count every routed request exactly once, and note the failures.
///
/// Sits below CORS and tracing so preflights and rejected origins are not
/// counted as application traffic.
pub async fn count_request(State(state): State<AppState>, req: Request, next: Next) -> Response {
    state.metrics.record_request();
    let response = next.run(req).await;
    if response.status().is_client_error() || response.status().is_server_error() {
        state.metrics.record_failed_response();
    }
    response
}

/// `GET /api/metrics` — JSON by default, Prometheus text when the caller
/// asks for it (`Accept: text/plain`), so both a scraper and the UI work
/// without a second endpoint.
#[instrument(skip(state))]
pub async fn get_metrics(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let snapshot = state.metrics.snapshot();

    let wants_prometheus = headers
        .get(header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|a| a.contains("text/plain"));

    if wants_prometheus {
        (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "text/plain; version=0.0.4")],
            snapshot.to_prometheus(),
        )
            .into_response()
    } else {
        axum::Json(snapshot).into_response()
    }
}
