use axum::{extract::State, Json};
use serde::Serialize;
use tracing::instrument;
use crate::AppState;

#[derive(Serialize)]
pub struct ConfigResponse {
    pub ai_enabled: bool,
}

#[instrument(skip(_state))]
pub async fn get_config(State(_state): State<AppState>) -> Json<ConfigResponse> {
    let ai_enabled = std::env::var("GEMINI_API_KEY").is_ok();
    Json(ConfigResponse {
        ai_enabled,
    })
}
