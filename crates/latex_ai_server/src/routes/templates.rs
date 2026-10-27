//! Starter templates: list them, start a project from one, and re-render the
//! front-matter block of a document made from one.

use crate::AppState;
use crate::error::ApiError;
use axum::Json;
use axum::extract::{Path as UrlPath, State};
use axum::http::{HeaderValue, header};
use axum::response::{IntoResponse, Response};
use latex_ai_core::security::resolve_new_path;
use latex_ai_core::storage::Precondition;
use latex_ai_core::templates::{self, Fields, TemplateError, TemplateInfo};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tracing::instrument;

fn template_error(e: &TemplateError) -> ApiError {
    match e {
        TemplateError::Unknown(_) => ApiError::NotFound {
            message: e.to_string(),
        },
        TemplateError::Broken(..) => ApiError::Internal {
            message: e.to_string(),
        },
    }
}

#[derive(Serialize)]
pub struct TemplateList {
    pub templates: Vec<TemplateInfo>,
}

/// `GET /api/templates`
#[instrument]
pub async fn list_templates() -> Json<TemplateList> {
    Json(TemplateList {
        templates: templates::list(),
    })
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemplateBody {
    pub template: String,
    #[serde(default)]
    pub fields: Fields,
}

#[derive(Serialize)]
pub struct CreatedFile {
    pub path: String,
    pub etag: String,
}

#[derive(Serialize)]
pub struct StartedProject {
    pub main: String,
    pub files: Vec<CreatedFile>,
}

/// `POST /api/project/template` — write a template's files into this project.
///
/// Only an empty project (no `.tex` file yet) accepts one: starting from a
/// template must never overwrite or mix into work that exists. Every file is
/// created with `CreateNew`, so a name collision is a 409, not a replacement.
#[instrument(skip(state, body), fields(template = %body.template))]
pub async fn start_from_template(
    State(state): State<AppState>,
    Json(body): Json<TemplateBody>,
) -> Result<Json<StartedProject>, ApiError> {
    let rendered =
        templates::render(&body.template, &body.fields).map_err(|e| template_error(&e))?;

    let root = state.workspace_root.clone();
    let existing =
        tokio::task::spawn_blocking(move || latex_ai_compiler::project::project_files(&root))
            .await
            .map_err(|e| ApiError::Internal {
                message: e.to_string(),
            })?
            .map_err(|e| ApiError::Internal {
                message: format!("reading the project: {e}"),
            })?;
    if existing
        .iter()
        .any(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("tex")))
    {
        return Err(ApiError::Conflict {
            message: "this project already has a .tex file; open an empty folder to start from a template"
                .to_owned(),
        });
    }

    let mut created = Vec::new();
    for file in rendered {
        let abs = resolve_new_path(&state.workspace_root, file.path)?;
        let rel: PathBuf = abs
            .strip_prefix(&state.workspace_root)
            .map(Path::to_path_buf)
            .map_err(|_| ApiError::InvalidPath {
                message: "path leaves the workspace".to_owned(),
            })?;
        if let Some(parent) = abs.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| ApiError::Internal {
                    message: format!("creating the folder: {e}"),
                })?;
        }
        let outcome = state
            .store
            .save_with_etag_history(
                &rel,
                &file.content,
                Precondition::CreateNew,
                "template",
                "create",
            )
            .await?;
        created.push(CreatedFile {
            path: file.path.to_owned(),
            etag: outcome.new_etag,
        });
    }
    Ok(Json(StartedProject {
        main: "main.tex".to_owned(),
        files: created,
    }))
}

#[derive(Serialize)]
pub struct Block {
    pub block: String,
}

/// `POST /api/templates/frontmatter` — the front-matter block for these
/// values. The editor swaps it into the open document, so the change is an
/// ordinary, undoable edit.
#[instrument(skip(body), fields(template = %body.template))]
pub async fn render_frontmatter(Json(body): Json<TemplateBody>) -> Result<Json<Block>, ApiError> {
    let block = templates::frontmatter_block(&body.template, &body.fields)
        .map_err(|e| template_error(&e))?;
    Ok(Json(Block { block }))
}

/// `GET /api/templates/{id}/thumbnail` — a PNG of the template's first page.
/// It only changes with a new build, so a day of caching is safe.
pub async fn thumbnail(UrlPath(id): UrlPath<String>) -> Result<Response, ApiError> {
    let png = templates::thumbnail(&id).ok_or_else(|| ApiError::NotFound {
        message: format!("no template called `{id}`"),
    })?;
    let mut res = png.into_response();
    let h = res.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_static("image/png"));
    h.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=86400"),
    );
    h.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    Ok(res)
}
