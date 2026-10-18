//! Workspace file routes: list, read (with `ETag`), save (with `If-Match`
//! optimistic concurrency + history). Every path goes through
//! `core::security::validate_path`.

use crate::AppState;
use crate::error::ApiError;
use axum::Json;
use axum::extract::{Query, State};
use axum::http::HeaderMap;
use latex_ai_core::security::{AccessKind, has_writable_extension, is_denied, validate_path};
use latex_ai_core::storage::{Precondition, etag_of};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;
use tracing::instrument;

/// Workspace source files (allowlisted extensions), relative to `root`,
/// skipping dotdirs / `target` / `node_modules`. What `/api/search` ranks.
/// Blocking — call from `spawn_blocking`.
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

/// One entry of the project tree.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct TreeNode {
    pub name: String,
    /// Project-relative, `/`-separated.
    pub path: String,
    pub kind: NodeKind,
    /// Whether the editor may open and save this file as text.
    pub editable: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<TreeNode>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    Dir,
    File,
}

#[derive(Serialize)]
pub struct Tree {
    pub tree: Vec<TreeNode>,
}

/// Turn a sorted list of project-relative file paths into a nested tree,
/// directories first, each level alphabetical (case-insensitive).
pub(crate) fn build_tree(files: &[PathBuf]) -> Vec<TreeNode> {
    fn insert(level: &mut Vec<TreeNode>, parts: &[String], prefix: &str, editable: bool) {
        let Some((first, rest)) = parts.split_first() else {
            return;
        };
        let path = if prefix.is_empty() {
            first.clone()
        } else {
            format!("{prefix}/{first}")
        };
        if rest.is_empty() {
            level.push(TreeNode {
                name: first.clone(),
                path,
                kind: NodeKind::File,
                editable,
                children: Vec::new(),
            });
            return;
        }
        let existing = level
            .iter()
            .position(|n| n.kind == NodeKind::Dir && n.name == *first);
        let at = existing.unwrap_or_else(|| {
            level.push(TreeNode {
                name: first.clone(),
                path: path.clone(),
                kind: NodeKind::Dir,
                editable: false,
                children: Vec::new(),
            });
            level.len() - 1
        });
        insert(&mut level[at].children, rest, &path, editable);
    }

    fn order(level: &mut Vec<TreeNode>) {
        level.sort_by(|a, b| {
            (a.kind == NodeKind::File)
                .cmp(&(b.kind == NodeKind::File))
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        for node in level {
            order(&mut node.children);
        }
    }

    let mut root = Vec::new();
    for file in files {
        let parts: Vec<String> = file
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        insert(&mut root, &parts, "", has_writable_extension(file));
    }
    order(&mut root);
    root
}

/// `GET /api/tree` — every file the project holds (secrets, dot-directories
/// and symlinks excluded) as a nested tree. Unlike `/api/files` this includes
/// images and other non-text files, marked `editable: false`.
#[instrument(skip(state))]
pub async fn get_tree(State(state): State<AppState>) -> Result<Json<Tree>, ApiError> {
    let root = state.workspace_root.clone();
    let files =
        tokio::task::spawn_blocking(move || latex_ai_compiler::project::project_files(&root))
            .await
            .map_err(|e| ApiError::Internal {
                message: e.to_string(),
            })?
            .map_err(|e| ApiError::Internal {
                message: format!("reading the project: {e}"),
            })?;
    Ok(Json(Tree {
        tree: build_tree(&files),
    }))
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
    /// The editor saved on its own timer. Such saves are folded into one
    /// history row per burst instead of adding a row each.
    #[serde(default)]
    pub autosave: bool,
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
            if req.autosave {
                latex_ai_core::storage::AUTOSAVE
            } else {
                "manual-save"
            },
        )
        .await?;

    Ok(Json(WriteFileResponse {
        etag: outcome.new_etag,
        history_id: outcome.history_id,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(list: &[&str]) -> Vec<PathBuf> {
        list.iter().map(PathBuf::from).collect()
    }

    #[test]
    fn files_nest_under_their_directories_dirs_first_then_alphabetical() {
        let tree = build_tree(&paths(&[
            "b.tex",
            "A.tex",
            "chapters/two.tex",
            "chapters/One.tex",
            "chapters/figs/a.png",
            "zeta/x.tex",
        ]));

        let names: Vec<&str> = tree.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, ["chapters", "zeta", "A.tex", "b.tex"]);

        let chapters = &tree[0];
        assert_eq!(chapters.kind, NodeKind::Dir);
        assert_eq!(chapters.path, "chapters");
        let inner: Vec<&str> = chapters.children.iter().map(|n| n.path.as_str()).collect();
        assert_eq!(
            inner,
            ["chapters/figs", "chapters/One.tex", "chapters/two.tex"]
        );
        assert_eq!(chapters.children[0].children[0].path, "chapters/figs/a.png");
    }

    #[test]
    fn only_source_files_are_editable() {
        let tree = build_tree(&paths(&["main.tex", "fig.png", "refs.bib", "notes.txt"]));
        let by_name = |n: &str| tree.iter().find(|x| x.name == n).unwrap().editable;
        assert!(by_name("main.tex") && by_name("refs.bib"));
        assert!(!by_name("fig.png") && !by_name("notes.txt"));
    }

    #[test]
    fn an_empty_project_is_an_empty_tree() {
        assert_eq!(build_tree(&[]).len(), 0);
    }
}
