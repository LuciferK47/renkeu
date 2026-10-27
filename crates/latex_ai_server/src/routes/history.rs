//! Edit-history route (masterplan.md §4.6): every storage mutation writes a
//! `history` row; this exposes them for the history sidebar / restore UX.

use crate::AppState;
use crate::error::ApiError;
use axum::Json;
use axum::extract::{Path, Query, State};
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
    /// The text this entry replaced was kept, so the change can be undone.
    pub has_before: bool,
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
            has_before: row.has_before,
        })
        .collect();

    Ok(Json(entries))
}

#[derive(Debug, Serialize)]
pub struct HistoryDetail {
    pub id: i64,
    pub file_path: String,
    pub edit_type: String,
    pub created_at: Option<String>,
    /// The file as it was right after this entry.
    pub text_after: String,
    /// What it replaced, when that was recorded (deletes, merged autosaves).
    pub text_before: Option<String>,
}

/// `GET /api/history/{id}` — one entry with its text, so a version can be
/// restored. Restoring is the client saving that text through the normal
/// conditional-save (or create) route, which keeps one write path.
#[instrument(skip(state))]
pub async fn history_entry(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<HistoryDetail>, ApiError> {
    let detail = state
        .store
        .history_detail(id)
        .await?
        .ok_or_else(|| ApiError::NotFound {
            message: format!("no history entry {id}"),
        })?;
    // Old rows are never a way around the read denylist.
    if latex_ai_core::security::is_denied(std::path::Path::new(&detail.file_path)) {
        return Err(ApiError::Forbidden {
            message: "access to this file is denied".to_owned(),
        });
    }
    Ok(Json(HistoryDetail {
        id: detail.id,
        file_path: detail.file_path,
        edit_type: detail.edit_type,
        created_at: detail.created_at,
        text_after: detail.text_after,
        text_before: detail.text_before,
    }))
}
