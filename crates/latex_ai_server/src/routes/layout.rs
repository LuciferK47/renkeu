//! `GET /api/layout` — layout facts for a finished build: page count, how
//! full the last page is, where a page budget is crossed, boxes the log calls
//! overfull or underfull, short last lines, and fonts left unembedded. All of
//! it comes from the build's `SyncTeX` map, log and PDF: no AI is involved.

use super::synctex::load_map;
use super::visual::{build_is_current, resolve_build};
use crate::AppState;
use crate::error::ApiError;
use axum::Json;
use axum::extract::{Query, State};
use layout::LayoutReport;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::instrument;

/// A budget beyond this is a typo, not a page limit.
const MAX_BUDGET: u32 = 10_000;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayoutQuery {
    pub build: String,
    /// The page budget to check against, if the document has one.
    #[serde(default)]
    pub pages: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct LayoutResponse {
    pub build: String,
    /// The project has changed since this build was compiled.
    pub stale: bool,
    #[serde(flatten)]
    pub report: LayoutReport,
    /// Fonts the PDF uses without embedding them; `None` when the PDF could
    /// not be inspected (no pdfium).
    pub fonts_not_embedded: Option<Vec<String>>,
}

/// `GET /api/layout?build=&pages=`
#[instrument(skip(state))]
pub async fn layout(
    State(state): State<AppState>,
    Query(q): Query<LayoutQuery>,
) -> Result<Json<LayoutResponse>, ApiError> {
    if q.pages.is_some_and(|p| p == 0 || p > MAX_BUDGET) {
        return Err(ApiError::Unprocessable {
            message: format!("a page budget is between 1 and {MAX_BUDGET}"),
        });
    }
    let build = resolve_build(&state, Some(&q.build), None).await?;
    let data = load_map(&build).await?;
    let outdated = !build_is_current(&state, &build).await?;

    let root = state.workspace_root.clone();
    let main = build.manifest.main.clone();
    let diagnostics = build.manifest.diagnostics.clone();
    let page_limit = q.pages;
    let analysed = tokio::task::spawn_blocking(move || {
        crate::check::analyse_layout(&data, &diagnostics, &root, &main, page_limit)
    })
    .await
    .map_err(|e| ApiError::Internal {
        message: e.to_string(),
    })?;

    let pdf: PathBuf = build.pdf_path();
    let fonts_not_embedded = tokio::task::spawn_blocking(move || {
        visual::render::renderer()
            .ok()
            .and_then(|r| r.unembedded_fonts(&pdf).ok())
    })
    .await
    .map_err(|e| ApiError::Internal {
        message: e.to_string(),
    })?;

    Ok(Json(LayoutResponse {
        build: build.id().to_owned(),
        stale: outdated,
        report: analysed,
        fonts_not_embedded,
    }))
}
