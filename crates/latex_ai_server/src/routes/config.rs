//! Runtime configuration (masterplan §15.1).
//!
//! The API key is write-only: no response ever contains it, only where it
//! comes from (`key_source`). Fields pinned by an environment variable are
//! listed in `env_overridden` and answer 409 to changes.

use crate::AppState;
use crate::error::ApiError;
use axum::{Json, extract::State};
use latex_ai_core::config::{KeySource, Snapshot, SettingsError, ConfigPatch};
use serde::{Deserialize, Serialize};
use tracing::instrument;

#[derive(Serialize)]
pub struct ConfigResponse {
    /// Whether an LLM API key is present, i.e. AI routes will work.
    pub ai_enabled: bool,
    pub model: String,
    pub vision_model: Option<String>,
    pub timeout_secs: u64,
    /// Where the active key comes from; the key itself is never returned.
    pub key_source: KeySource,
    /// Fields an environment variable pins; PATCHing them is a 409.
    pub env_overridden: Vec<&'static str>,
}

impl From<&Snapshot> for ConfigResponse {
    fn from(s: &Snapshot) -> Self {
        Self {
            ai_enabled: s.ai_enabled(),
            model: s.config.ai.model.clone(),
            vision_model: s.config.ai.vision_model.clone(),
            timeout_secs: s.config.ai.timeout_secs,
            key_source: s.key_source,
            env_overridden: s.overridden.names(),
        }
    }
}

impl From<SettingsError> for ApiError {
    fn from(e: SettingsError) -> Self {
        match e {
            SettingsError::Locked { .. } => Self::Conflict {
                message: e.to_string(),
            },
            SettingsError::Config(c) => Self::Unprocessable {
                message: c.to_string(),
            },
            other => Self::Internal {
                message: other.to_string(),
            },
        }
    }
}

#[instrument(skip(state))]
pub async fn get_config(State(state): State<AppState>) -> Json<ConfigResponse> {
    Json(ConfigResponse::from(&*state.settings.snapshot()))
}

#[instrument(skip(state))]
pub async fn patch_config(
    State(state): State<AppState>,
    Json(patch): Json<ConfigPatch>,
) -> Result<Json<ConfigResponse>, ApiError> {
    let snapshot = state.settings.update(patch).await?;
    Ok(Json(ConfigResponse::from(&*snapshot)))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApiKeyBody {
    key: String,
}

// `ApiKeyBody` carries a secret, so it is never `Debug` and is skipped from
// the span.
#[instrument(skip(state, body))]
pub async fn put_api_key(
    State(state): State<AppState>,
    Json(body): Json<ApiKeyBody>,
) -> Result<Json<ConfigResponse>, ApiError> {
    let snapshot = state.settings.set_api_key(&body.key).await?;
    Ok(Json(ConfigResponse::from(&*snapshot)))
}

#[instrument(skip(state))]
pub async fn delete_api_key(
    State(state): State<AppState>,
) -> Result<Json<ConfigResponse>, ApiError> {
    let snapshot = state.settings.clear_api_key().await?;
    Ok(Json(ConfigResponse::from(&*snapshot)))
}
