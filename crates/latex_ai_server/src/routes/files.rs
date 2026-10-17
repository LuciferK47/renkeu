//! Workspace file routes: list, read (with `ETag`), save (with `If-Match`
//! optimistic concurrency + history). Every path goes through
//! `core::security::validate_path`.

use crate::AppState;
use crate::error::ApiError;
use axum::Json;
use axum::extract::{Query, State};
use axum::http::HeaderMap;
use latex_ai_core::security::{AccessKind, is_denied, validate_path};
use latex_ai_core::storage::{Precondition, etag_of};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;
use tracing::instrument;

#[derive(Serialize)]
pub struct FileList {
    pub files: Vec<String>,
}

/// Workspace source files (allowlisted extensions), relative to `root`,
/// skipping dotdirs / `target` / `node_modules`. Blocking — call from
/// `spawn_blocking`.
pub(crate) fn list_workspace_files(root: &Path) -> Vec<String> {
    let mut result = Vec::new();
    let mut dirs = vec![root.to_path_buf()];

    while let Some(dir) = dirs.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            // `DirEntry::file_type` does not follow symlinks, so a link back
            // up the tree is skipped instead of looped on forever, and a link
            // pointing outside the workspace is never listed.
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            let path = entry.path();
            if file_type.is_dir() {
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                if !name_str.starts_with('.') && name_str != "target" && name_str != "node_modules"
                {
                    dirs.push(path);
                }
            } else if file_type.is_file() {
                if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                    if matches!(ext, "tex" | "bib" | "sty" | "cls" | "tikz") {
                        if let Ok(rel) = path.strip_prefix(root) {
                            if !is_denied(rel) {
                                result.push(rel.to_string_lossy().into_owned());
                            }
                        }
                    }
                }
            }
        }
    }
    result.sort();
    result
}

/// `GET /api/files` — list workspace source files.
#[instrument(skip(state))]
pub async fn list_files(State(state): State<AppState>) -> Json<FileList> {
    let root = state.workspace_root.clone();
    let files = tokio::task::spawn_blocking(move || list_workspace_files(&root))
        .await
        .unwrap_or_else(|e| {
            tracing::warn!(error = %e, "file listing task failed");
            Vec::new()
        });
    Json(FileList { files })
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileQuery {
    pub path: String,
}

#[derive(Debug, Serialize)]
pub struct FileResponse {
    pub content: String,
    pub etag: String,
    /// Unix seconds; informational only.
    pub modified_at: Option<u64>,
}

/// `GET /api/file?path=...` — read one workspace file with its `ETag`.
#[instrument(skip(state))]
pub async fn read_file(
    State(state): State<AppState>,
    Query(query): Query<FileQuery>,
) -> Result<Json<FileResponse>, ApiError> {
    let abs = validate_path(&state.workspace_root, &query.path, AccessKind::Read)?;

    let content = tokio::fs::read_to_string(&abs)
        .await
        .map_err(|e| ApiError::Internal {
            message: format!("read failed: {e}"),
        })?;

    let modified_at = tokio::fs::metadata(&abs)
        .await
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs());

    Ok(Json(FileResponse {
        etag: etag_of(&content),
        content,
        modified_at,
    }))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WriteFileRequest {
    pub path: String,
    pub content: String,
}

#[derive(Debug, Serialize)]
pub struct WriteFileResponse {
    pub etag: String,
    pub history_id: i64,
}

/// `POST /api/file` — save with a required `If-Match` `ETag` (409 on drift,
/// 428 when absent) and a history row. A blind overwrite is never accepted:
/// it is how concurrent edits used to be lost.
#[instrument(skip(state, req))]
pub async fn write_file(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<WriteFileRequest>,
) -> Result<Json<WriteFileResponse>, ApiError> {
    validate_path(&state.workspace_root, &req.path, AccessKind::Write)?;

    let if_match = headers
        .get("if-match")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim_matches('"').to_owned())
        .ok_or_else(|| ApiError::PreconditionRequired {
            message: "saving requires an If-Match header carrying the file's ETag".to_owned(),
        })?;

    let outcome = state
        .store
        .save_with_etag_history(
            &PathBuf::from(&req.path),
            &req.content,
            Precondition::Etag(&if_match),
            "save",
            "manual-save",
        )
        .await?;

    Ok(Json(WriteFileResponse {
        etag: outcome.new_etag,
        history_id: outcome.history_id,
    }))
}
