//! Project file management: create, rename, delete, upload, raw download and
//! the persisted main-file choice. Every new name goes through
//! `security::resolve_new_path`; every existing one through `validate_path`.

use crate::AppState;
use crate::error::ApiError;
use axum::Json;
use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use latex_ai_core::project::{ProjectData, ProjectSettings};
use latex_ai_core::security::{
    AccessKind, has_writable_extension, resolve_new_path, validate_path,
};
use latex_ai_core::storage::Precondition;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tracing::instrument;

/// Largest file `POST /api/upload` accepts.
pub const MAX_UPLOAD_BYTES: usize = 25 * 1024 * 1024;

/// Non-text types a project may hold: what `\includegraphics` and
/// `\includepdf` read, with the content type they are served under.
const BINARY_TYPES: &[(&str, &str)] = &[
    ("png", "image/png"),
    ("jpg", "image/jpeg"),
    ("jpeg", "image/jpeg"),
    ("gif", "image/gif"),
    ("svg", "image/svg+xml"),
    ("pdf", "application/pdf"),
];

fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default()
}

fn content_type_for(path: &Path) -> &'static str {
    let ext = extension(path);
    BINARY_TYPES
        .iter()
        .find(|(e, _)| *e == ext)
        .map_or("application/octet-stream", |(_, t)| t)
}

fn upload_allowed(path: &Path) -> bool {
    has_writable_extension(path) || BINARY_TYPES.iter().any(|(e, _)| *e == extension(path))
}

fn rel_string(path: &Path) -> String {
    path.components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// Project-relative form of an absolute path that `resolve_new_path` returned.
fn relative(state: &AppState, abs: &Path) -> Result<PathBuf, ApiError> {
    abs.strip_prefix(&state.workspace_root)
        .map(Path::to_path_buf)
        .map_err(|_| ApiError::InvalidPath {
            message: "path leaves the workspace".to_owned(),
        })
}

fn io_error(what: &str, e: &std::io::Error) -> ApiError {
    ApiError::Internal {
        message: format!("{what}: {e}"),
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateRequest {
    pub path: String,
    #[serde(default)]
    pub content: String,
}

#[derive(Serialize)]
pub struct CreateResponse {
    pub path: String,
    pub etag: String,
}

/// `POST /api/files/create` — a new text file, with any missing directories.
#[instrument(skip(state, req))]
pub async fn create_file(
    State(state): State<AppState>,
    Json(req): Json<CreateRequest>,
) -> Result<Json<CreateResponse>, ApiError> {
    let abs = resolve_new_path(&state.workspace_root, &req.path)?;
    let rel = relative(&state, &abs)?;
    if !has_writable_extension(&rel) {
        return Err(ApiError::InvalidPath {
            message: "new files must be .tex, .bib, .sty, .cls or .tikz (upload images instead)"
                .to_owned(),
        });
    }
    if let Some(parent) = abs.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| io_error("creating the folder", &e))?;
    }
    let outcome = state
        .store
        .save_with_etag_history(
            &rel,
            &req.content,
            Precondition::CreateNew,
            "create",
            "create",
        )
        .await?;
    Ok(Json(CreateResponse {
        path: rel_string(&rel),
        etag: outcome.new_etag,
    }))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenameRequest {
    pub from: String,
    pub to: String,
}

#[derive(Serialize)]
pub struct RenameResponse {
    pub from: String,
    pub to: String,
}

/// `POST /api/files/rename` — move a file or a whole directory. Never
/// replaces anything: an existing target is a 409.
#[instrument(skip(state))]
pub async fn rename(
    State(state): State<AppState>,
    Json(req): Json<RenameRequest>,
) -> Result<Json<RenameResponse>, ApiError> {
    // Read access: the source may be a directory, which has no extension for
    // the write check to look at. The denylist applies either way.
    let from_abs = validate_path(&state.workspace_root, &req.from, AccessKind::Read)?;
    let from_rel = relative(&state, &from_abs)?;
    let to_abs = resolve_new_path(&state.workspace_root, &req.to)?;
    let to_rel = relative(&state, &to_abs)?;

    let is_file = tokio::fs::metadata(&from_abs)
        .await
        .map_err(|e| io_error("reading the source", &e))?
        .is_file();
    if is_file && !upload_allowed(&from_rel) {
        return Err(ApiError::InvalidPath {
            message: "this kind of file cannot be renamed here".to_owned(),
        });
    }
    if is_file && extension(&from_rel) != extension(&to_rel) {
        return Err(ApiError::InvalidPath {
            message: "a rename may not change the file type".to_owned(),
        });
    }
    if to_rel.starts_with(&from_rel) {
        return Err(ApiError::InvalidPath {
            message: "cannot move a folder into itself".to_owned(),
        });
    }

    // Serialise with saves so a write cannot land on the old name mid-move.
    let _guard = state.store.lock_writes().await;
    if tokio::fs::symlink_metadata(&to_abs).await.is_ok() {
        return Err(ApiError::Conflict {
            message: format!("{} already exists", rel_string(&to_rel)),
        });
    }
    if let Some(parent) = to_abs.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| io_error("creating the folder", &e))?;
    }
    tokio::fs::rename(&from_abs, &to_abs)
        .await
        .map_err(|e| io_error("renaming", &e))?;
    state
        .store
        .rename_history(&rel_string(&from_rel), &rel_string(&to_rel))
        .await?;
    Ok(Json(RenameResponse {
        from: rel_string(&from_rel),
        to: rel_string(&to_rel),
    }))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PathQuery {
    pub path: String,
}

/// `DELETE /api/file?path=` — requires the file's `ETag` in `If-Match`, the
/// same rule as saving. The removed text stays in history.
#[instrument(skip(state, headers))]
pub async fn delete_file(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<PathQuery>,
) -> Result<StatusCode, ApiError> {
    let abs = validate_path(&state.workspace_root, &q.path, AccessKind::Write)?;
    let rel = relative(&state, &abs)?;
    let etag = headers
        .get(header::IF_MATCH)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim_matches('"').to_owned())
        .ok_or_else(|| ApiError::PreconditionRequired {
            message: "deleting requires an If-Match header carrying the file's ETag".to_owned(),
        })?;
    state
        .store
        .delete_with_etag_history(&rel, &etag, "delete")
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /api/upload?path=` — the request body is the file. Never replaces.
#[instrument(skip(state, body), fields(bytes = body.len()))]
pub async fn upload(
    State(state): State<AppState>,
    Query(q): Query<PathQuery>,
    body: Bytes,
) -> Result<Json<CreateResponse>, ApiError> {
    let abs = resolve_new_path(&state.workspace_root, &q.path)?;
    let rel = relative(&state, &abs)?;
    if !upload_allowed(&rel) {
        return Err(ApiError::InvalidPath {
            message: format!("a .{} file cannot be added to a project", extension(&rel)),
        });
    }
    if body.is_empty() {
        return Err(ApiError::BadRequest {
            message: "the upload is empty".to_owned(),
        });
    }
    let _guard = state.store.lock_writes().await;
    if tokio::fs::symlink_metadata(&abs).await.is_ok() {
        return Err(ApiError::Conflict {
            message: format!("{} already exists", rel_string(&rel)),
        });
    }
    if let Some(parent) = abs.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| io_error("creating the folder", &e))?;
    }
    latex_ai_core::fsutil::atomic_write(&abs, &body)
        .await
        .map_err(|e| io_error("writing the upload", &e))?;
    Ok(Json(CreateResponse {
        path: rel_string(&rel),
        etag: latex_ai_core::storage::etag_of_bytes(&body),
    }))
}

/// `GET /api/raw?path=` — the file's bytes with a content type, for the image
/// preview. Only known media types are served inline; the rest download.
#[instrument(skip(state))]
pub async fn raw(
    State(state): State<AppState>,
    Query(q): Query<PathQuery>,
) -> Result<Response, ApiError> {
    let abs = validate_path(&state.workspace_root, &q.path, AccessKind::Read)?;
    let bytes = tokio::fs::read(&abs)
        .await
        .map_err(|e| io_error("reading the file", &e))?;
    let content_type = content_type_for(&abs);
    let etag = latex_ai_core::storage::etag_of_bytes(&bytes);
    let mut res = bytes.into_response();
    let h = res.headers_mut();
    if let Ok(v) = HeaderValue::from_str(&format!("\"{etag}\"")) {
        h.insert(header::ETAG, v);
    }
    h.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    h.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    // An SVG can carry script; this keeps it inert even when opened directly.
    h.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(
            "sandbox; default-src 'none'; style-src 'unsafe-inline'; img-src data:",
        ),
    );
    if content_type == "application/octet-stream" {
        h.insert(
            header::CONTENT_DISPOSITION,
            HeaderValue::from_static("attachment"),
        );
    }
    Ok(res)
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectBody {
    pub main: Option<String>,
}

fn settings_path(state: &AppState) -> PathBuf {
    ProjectData::locate(state.settings.home(), &state.workspace_root).settings_path()
}

/// The persisted main file, if one was chosen and still exists.
pub(crate) async fn saved_main(state: &AppState) -> Option<String> {
    let settings = match ProjectSettings::load(&settings_path(state)) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(error = %e, "ignoring unreadable project settings");
            return None;
        }
    };
    let main = settings.main?;
    tokio::fs::metadata(state.workspace_root.join(&main))
        .await
        .is_ok_and(|m| m.is_file())
        .then_some(main)
}

#[derive(Debug, Serialize)]
pub struct ProjectInfo {
    /// The root document the user chose, if any.
    pub main: Option<String>,
    /// Folder name, for the title bar.
    pub name: String,
    /// Where the project lives; shown, never used to address files.
    pub root: String,
}

/// `GET /api/project` — the project's persisted choices and where it lives.
#[instrument(skip(state))]
pub async fn get_project(State(state): State<AppState>) -> Json<ProjectInfo> {
    let root = &state.workspace_root;
    Json(ProjectInfo {
        main: saved_main(&state).await,
        name: root
            .file_name()
            .map_or_else(|| root.to_string_lossy(), |n| n.to_string_lossy())
            .into_owned(),
        root: root.to_string_lossy().into_owned(),
    })
}

/// `PUT /api/project` — choose the main file (`null` goes back to detection).
#[instrument(skip(state))]
pub async fn put_project(
    State(state): State<AppState>,
    Json(body): Json<ProjectBody>,
) -> Result<Json<ProjectBody>, ApiError> {
    if let Some(main) = &body.main {
        let abs = validate_path(&state.workspace_root, main, AccessKind::Read)?;
        if !has_writable_extension(&abs) || extension(&abs) != "tex" {
            return Err(ApiError::Unprocessable {
                message: "the main file must be a .tex file".to_owned(),
            });
        }
    }
    let path = settings_path(&state);
    let settings = ProjectSettings {
        main: body.main.clone(),
    };
    settings.save(&path).await.map_err(|e| ApiError::Internal {
        message: format!("saving project settings: {e}"),
    })?;
    Ok(Json(body))
}
