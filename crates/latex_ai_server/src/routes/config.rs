//! Read-only runtime config for the UI. Writing settings server-side is M5
//! scope; the SPA persists its own preferences in `localStorage`.

use crate::AppState;
use axum::{Json, extract::State};
use serde::Serialize;
use tracing::instrument;

#[derive(Serialize)]
pub struct ConfigResponse {
    /// Whether an LLM API key is present, i.e. AI routes will work.
    pub ai_enabled: bool,
    /// The single configured planner model (masterplan Q1; no model registry
    /// until a second provider exists).
    pub model: String,
}

#[instrument(skip(state))]
pub async fn get_config(State(state): State<AppState>) -> Json<ConfigResponse> {
    Json(ConfigResponse {
        ai_enabled: std::env::var("GROQ_API_KEY").is_ok(),
        model: state.model.clone(),
    })
}
