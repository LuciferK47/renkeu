//! Edit-history route (masterplan.md §4.6): every storage mutation writes a
//! `history` row; this exposes them for the history sidebar / restore UX.

use crate::AppState;
use crate::error::ApiError;
use axum::Json;
use axum::extract::{Query, State};
use serde::{Deserialize, Serialize};
use tracing::instrument;

fn default_limit() -> i64 {
    100
}

#[derive(Debug, Deserialize)]
pub struct HistoryQuery {
    /// Restrict to one workspace file (relative path).
    pub path: Option<String>,
    /// Only agent-authored edits (`edit_type = 'agent-apply'`).
    #[serde(default)]
    pub agent_only: bool,
    #[serde(default = "default_limit")]
    pub limit: i64,
}

#[derive(Debug, Serialize)]
pub struct HistoryEntry {
    pub id: i64,
    pub file_path: String,
    pub command: String,
    pub diff: String,
    pub edit_type: String,
    pub created_at: Option<String>,
}

/// `GET /api/history?path=&agent_only=&limit=` — newest first.
#[instrument(skip(state))]
pub async fn list_history(
    State(state): State<AppState>,
    Query(query): Query<HistoryQuery>,
) -> Result<Json<Vec<HistoryEntry>>, ApiError> {
    let limit = query.limit.clamp(1, 1000);
    let entries = state
        .store
        .recent_history(query.path.as_deref(), query.agent_only, limit)
        .await?
        .into_iter()
        .map(|row| HistoryEntry {
            id: row.id,
            file_path: row.file_path,
            command: row.command,
            diff: row.diff,
            edit_type: row.edit_type,
            created_at: row.created_at,
        })
        .collect();

    Ok(Json(entries))
}
