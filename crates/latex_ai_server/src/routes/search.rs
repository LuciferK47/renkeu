//! Find and replace across the project's text files.

use crate::AppState;
use crate::error::ApiError;
use axum::Json;
use axum::extract::State;
use latex_ai_core::security::has_writable_extension;
use latex_ai_core::storage::{Precondition, etag_of};
use latex_ai_core::textsearch::{Hit, Matcher, SearchError};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::instrument;

/// Matches returned in all; the rest are counted but not listed.
const MAX_HITS: usize = 500;
/// Files larger than this are not searched (generated or data files).
const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_REPLACEMENT_BYTES: usize = 10_000;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SearchBody {
    pub query: String,
    #[serde(default)]
    pub regex: bool,
    #[serde(default)]
    pub case_sensitive: bool,
}

#[derive(Serialize)]
pub struct FileHits {
    pub path: String,
    pub hits: Vec<Hit>,
}

#[derive(Serialize)]
pub struct SearchResults {
    pub files: Vec<FileHits>,
    pub total: usize,
    /// More matches exist than were listed.
    pub truncated: bool,
}

fn matcher(body: &SearchBody) -> Result<Matcher, ApiError> {
    Matcher::new(&body.query, body.regex, body.case_sensitive).map_err(|e| match e {
        SearchError::Empty | SearchError::BadPattern(_) => ApiError::BadRequest {
            message: e.to_string(),
        },
    })
}

fn rel_string(path: &std::path::Path) -> String {
    path.components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// Searchable files: the project's editable text files, in path order.
async fn text_files(state: &AppState) -> Result<Vec<PathBuf>, ApiError> {
    let root = state.workspace_root.clone();
    let mut files =
        tokio::task::spawn_blocking(move || latex_ai_compiler::project::project_files(&root))
            .await
            .map_err(|e| ApiError::Internal {
                message: e.to_string(),
            })?
            .map_err(|e| ApiError::Internal {
                message: format!("reading the project: {e}"),
            })?;
    files.retain(|p| has_writable_extension(p));
    files.sort();
    Ok(files)
}

async fn read_text(state: &AppState, rel: &std::path::Path) -> Option<String> {
    let abs = state.workspace_root.join(rel);
    let meta = tokio::fs::metadata(&abs).await.ok()?;
    if meta.len() > MAX_FILE_BYTES {
        return None;
    }
    tokio::fs::read_to_string(&abs).await.ok()
}

/// `POST /api/project/search`
#[instrument(skip(state, body))]
pub async fn search(
    State(state): State<AppState>,
    Json(body): Json<SearchBody>,
) -> Result<Json<SearchResults>, ApiError> {
    let m = matcher(&body)?;
    let mut files = Vec::new();
    let mut total = 0usize;
    let mut listed = 0usize;
    for rel in text_files(&state).await? {
        let Some(text) = read_text(&state, &rel).await else {
            continue;
        };
        // Ask for one more than the room left, to know whether it overflowed.
        let room = MAX_HITS - listed;
        let mut hits = m.find(&text, room + 1);
        if hits.is_empty() {
            continue;
        }
        total += hits.len();
        if hits.len() > room {
            hits.truncate(room);
        }
        listed += hits.len();
        files.push(FileHits {
            path: rel_string(&rel),
            hits,
        });
        if listed >= MAX_HITS {
            break;
        }
    }
    Ok(Json(SearchResults {
        truncated: total > listed,
        files,
        total,
    }))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplaceBody {
    #[serde(flatten)]
    pub search: SearchBody,
    pub replacement: String,
    /// Limit the replacement to these files; absent means every match.
    #[serde(default)]
    pub paths: Option<Vec<String>>,
}

#[derive(Serialize)]
pub struct Replaced {
    pub path: String,
    pub count: usize,
}

#[derive(Serialize)]
pub struct ReplaceResults {
    pub changed: Vec<Replaced>,
    pub total: usize,
    /// Files left alone because they changed while this ran or were refused.
    pub skipped: Vec<String>,
}

/// `POST /api/project/replace` — every file is saved through the store, so
/// each change lands in the history and can be put back from there.
#[instrument(skip(state, body))]
pub async fn replace(
    State(state): State<AppState>,
    Json(body): Json<ReplaceBody>,
) -> Result<Json<ReplaceResults>, ApiError> {
    if body.replacement.len() > MAX_REPLACEMENT_BYTES {
        return Err(ApiError::BadRequest {
            message: "the replacement is too long".to_owned(),
        });
    }
    let m = matcher(&body.search)?;
    let mut changed = Vec::new();
    let mut skipped = Vec::new();
    let mut total = 0usize;
    for rel in text_files(&state).await? {
        let name = rel_string(&rel);
        if body
            .paths
            .as_ref()
            .is_some_and(|only| !only.contains(&name))
        {
            continue;
        }
        let Some(text) = read_text(&state, &rel).await else {
            continue;
        };
        let (new_text, count) = m.replace_all(&text, &body.replacement, !body.search.regex);
        if count == 0 {
            continue;
        }
        match state
            .store
            .save_with_etag_history(
                &rel,
                &new_text,
                Precondition::Etag(&etag_of(&text)),
                "replace",
                "replace-all",
            )
            .await
        {
            Ok(_) => {
                total += count;
                changed.push(Replaced { path: name, count });
            }
            Err(e) => {
                tracing::warn!(file = %name, error = %e, "replace skipped a file");
                skipped.push(name);
            }
        }
    }
    Ok(Json(ReplaceResults {
        changed,
        total,
        skipped,
    }))
}
