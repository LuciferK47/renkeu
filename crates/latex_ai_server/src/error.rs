use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Serialize, Error)]
#[serde(tag = "error", deny_unknown_fields)]
pub enum ApiError {
    #[error("etag mismatch")]
    EtagConflict,
    #[error("rate limited, retry after {retry_after_s}s")]
    RateLimited { retry_after_s: u32 },
    #[error("security: {0}")]
    InvalidPath(String),
    #[error("compile timeout")]
    CompileTimeout,
    #[error("VLM parse error: {0}")]
    VlmParseError(String),
    #[error("synctex miss on page {page} at ({x}, {y})")]
    SyncTeXMiss { page: u32, x: f64, y: f64 },
    #[error("unauthorized")]
    Unauthorized,
    #[error("bad request: {0}")]
    BadRequest(String),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = match &self {
            Self::EtagConflict => StatusCode::CONFLICT,
            Self::RateLimited { .. } => StatusCode::TOO_MANY_REQUESTS,
            Self::InvalidPath(_) => StatusCode::BAD_REQUEST, // Or FORBIDDEN depending on exact context, but masterplan says 400 for InvalidPath
            Self::CompileTimeout => StatusCode::SERVICE_UNAVAILABLE, // or 504
            Self::VlmParseError(_) => StatusCode::UNPROCESSABLE_ENTITY,
            Self::SyncTeXMiss { .. } => StatusCode::NOT_FOUND,
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
        };

        // Masterplan says status code mapping:
        // 400 BadRequest/InvalidPath; 401 Unauthorized; 403 Security-related; 404 FileNotFound; 409 EtagConflict; 413 PayloadTooLarge; 422 garde validation; 429 RateLimited; 500 Uncaught; 503 CircuitBreakerOpen or PlannerUnavailable.
        let status = match &self {
            Self::EtagConflict => StatusCode::CONFLICT,
            Self::RateLimited { .. } => StatusCode::TOO_MANY_REQUESTS,
            Self::InvalidPath(_) => StatusCode::BAD_REQUEST,
            Self::CompileTimeout => StatusCode::SERVICE_UNAVAILABLE,
            Self::VlmParseError(_) => StatusCode::UNPROCESSABLE_ENTITY,
            Self::SyncTeXMiss { .. } => StatusCode::NOT_FOUND,
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
        };

        let body = serde_json::to_string(&self).unwrap_or_default();
        Response::builder()
            .status(status)
            .header("content-type", "application/json")
            .body(axum::body::Body::from(body))
            .unwrap()
    }
}
