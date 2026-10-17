//! API error envelope (masterplan.md §8.1): serialized as
//! `{"error": "<variant>", ...fields}` with the §8.1 status-code mapping.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use latex_ai_core::security::SecurityError;
use latex_ai_core::storage::StorageError;
use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Serialize, Error)]
#[serde(tag = "error", rename_all = "snake_case")]
#[non_exhaustive]
pub enum ApiError {
    #[error("etag mismatch")]
    EtagConflict { current_etag: String },
    #[error("rate limited, retry after {retry_after_s}s")]
    RateLimited { retry_after_s: u32 },
    #[error("security: {message}")]
    InvalidPath { message: String },
    #[error("compile timeout")]
    CompileTimeout,
    #[error("VLM parse error: {message}")]
    VlmParseError { message: String },
    #[error("synctex miss on page {page} at ({x}, {y})")]
    SyncTexMiss { page: u32, x: f64, y: f64 },
    #[error("unauthorized")]
    Unauthorized,
    #[error("bad request: {message}")]
    BadRequest { message: String },
    #[error("not found: {message}")]
    NotFound { message: String },
    #[error("forbidden: {message}")]
    Forbidden { message: String },
    #[error("precondition required: {message}")]
    PreconditionRequired { message: String },
    #[error("conflict: {message}")]
    Conflict { message: String },
    #[error("unprocessable: {message}")]
    Unprocessable { message: String },
    #[error("unavailable: {message}")]
    Unavailable { message: String },
    #[error("internal error")]
    Internal { message: String },
}

impl ApiError {
    fn status(&self) -> StatusCode {
        // masterplan §8.1: 400 BadRequest/InvalidPath; 401 Unauthorized;
        // 404 not found; 409 EtagConflict; 422 validation; 429 RateLimited;
        // 500 uncaught; 503 unavailable.
        match self {
            Self::EtagConflict { .. } | Self::Conflict { .. } => StatusCode::CONFLICT,
            Self::Forbidden { .. } => StatusCode::FORBIDDEN,
            Self::PreconditionRequired { .. } => StatusCode::PRECONDITION_REQUIRED,
            Self::RateLimited { .. } => StatusCode::TOO_MANY_REQUESTS,
            Self::InvalidPath { .. } | Self::BadRequest { .. } => StatusCode::BAD_REQUEST,
            Self::CompileTimeout | Self::Unavailable { .. } => StatusCode::SERVICE_UNAVAILABLE,
            Self::VlmParseError { .. } | Self::Unprocessable { .. } => {
                StatusCode::UNPROCESSABLE_ENTITY
            }
            Self::SyncTexMiss { .. } | Self::NotFound { .. } => StatusCode::NOT_FOUND,
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::Internal { .. } => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        if matches!(self, Self::Internal { .. }) {
            tracing::error!(error = %self, "internal API error");
        }
        (self.status(), Json(self)).into_response()
    }
}

impl From<SecurityError> for ApiError {
    fn from(e: SecurityError) -> Self {
        match e {
            SecurityError::NotFound => Self::NotFound {
                message: "file not found".to_owned(),
            },
            SecurityError::Denied => Self::Forbidden {
                message: "access to this file is denied".to_owned(),
            },
            other => Self::InvalidPath {
                message: other.to_string(),
            },
        }
    }
}

impl From<StorageError> for ApiError {
    fn from(e: StorageError) -> Self {
        match e {
            StorageError::EtagConflict { actual, .. } => Self::EtagConflict {
                current_etag: actual,
            },
            StorageError::AlreadyExists => Self::Conflict {
                message: "file already exists".to_owned(),
            },
            StorageError::NotFound => Self::NotFound {
                message: "file not found".to_owned(),
            },
            StorageError::Apply(apply_err) => Self::Unprocessable {
                message: apply_err.to_string(),
            },
            StorageError::Policy(violation) => Self::Unprocessable {
                message: violation.to_string(),
            },
            StorageError::Io(io) => Self::Internal {
                message: io.to_string(),
            },
            StorageError::Database(db) => Self::Internal {
                message: db.to_string(),
            },
            other => Self::Internal {
                message: other.to_string(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_is_tagged_with_variant_name() {
        let e = ApiError::EtagConflict {
            current_etag: "abc".into(),
        };
        let json = serde_json::to_value(&e).unwrap();
        assert_eq!(json["error"], "etag_conflict");
        assert_eq!(json["current_etag"], "abc");
    }

    #[test]
    fn status_codes_follow_masterplan_mapping() {
        let cases: &[(ApiError, StatusCode)] = &[
            (
                ApiError::EtagConflict {
                    current_etag: String::new(),
                },
                StatusCode::CONFLICT,
            ),
            (
                ApiError::InvalidPath {
                    message: String::new(),
                },
                StatusCode::BAD_REQUEST,
            ),
            (
                ApiError::Forbidden {
                    message: String::new(),
                },
                StatusCode::FORBIDDEN,
            ),
            (
                ApiError::PreconditionRequired {
                    message: String::new(),
                },
                StatusCode::PRECONDITION_REQUIRED,
            ),
            (
                ApiError::NotFound {
                    message: String::new(),
                },
                StatusCode::NOT_FOUND,
            ),
            (
                ApiError::Unprocessable {
                    message: String::new(),
                },
                StatusCode::UNPROCESSABLE_ENTITY,
            ),
            (
                ApiError::Unavailable {
                    message: String::new(),
                },
                StatusCode::SERVICE_UNAVAILABLE,
            ),
            (
                ApiError::Internal {
                    message: String::new(),
                },
                StatusCode::INTERNAL_SERVER_ERROR,
            ),
        ];
        for (err, want) in cases {
            assert_eq!(err.status(), *want, "wrong status for {err}");
        }
    }
}
