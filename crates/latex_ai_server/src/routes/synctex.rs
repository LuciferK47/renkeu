//! `SyncTeX` both ways: a source line to the PDF region it produced, and a
//! point on the PDF to the source line behind it.
//!
//! Coordinates are PDF points from the page's top-left corner, the same unit
//! the visual-fix box uses; the viewer scales them to whatever zoom it is at.
//! Both answers say whether the project has changed since the build: the
//! mapping is still correct for the PDF being looked at, but line numbers may
//! have moved in the editor since.

use super::visual::{build_is_current, resolve_build, resolve_hit_path};
use crate::AppState;
use crate::error::ApiError;
use axum::Json;
use axum::extract::{Query, State};
use latex_ai_compiler::builds::Build;
use latex_ai_core::security::{AccessKind, validate_path};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use synctex::SyncTexData;
use tracing::instrument;

/// Parsed maps kept in memory. A map is rebuilt from a gzip file of a few
/// hundred KB; interactive use asks about the same build over and over.
const CACHE_SLOTS: usize = 4;

/// Largest coordinate accepted, in points. Rejects nonsense before searching.
const MAX_COORD_PT: f64 = 100_000.0;

type CacheEntry = (PathBuf, Arc<SyncTexData>);

/// Keyed by the map's path. Builds are content-addressed and immutable, so a
/// path never changes meaning; it also embeds the project, so two projects
/// with identical content do not share a map (their recorded input names
/// differ).
fn cache() -> &'static Mutex<Vec<CacheEntry>> {
    static CACHE: OnceLock<Mutex<Vec<CacheEntry>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(Vec::new()))
}

async fn load_map(build: &Build) -> Result<Arc<SyncTexData>, ApiError> {
    let path = build.synctex_path();
    if !build.manifest.has_synctex || !path.exists() {
        return Err(ApiError::NotFound {
            message: "this build has no SyncTeX map — recompile to generate one".to_owned(),
        });
    }
    let hit = cache().lock().ok().and_then(|c| {
        c.iter()
            .find(|(p, _)| *p == path)
            .map(|(_, data)| Arc::clone(data))
    });
    if let Some(data) = hit {
        return Ok(data);
    }
    let parse_path = path.clone();
    let data = tokio::task::spawn_blocking(move || SyncTexData::parse(&parse_path))
        .await
        .map_err(|e| ApiError::Internal {
            message: e.to_string(),
        })?
        .map_err(|e| ApiError::Internal {
            message: format!("reading the SyncTeX map: {e}"),
        })?;
    let data = Arc::new(data);
    if let Ok(mut c) = cache().lock() {
        c.retain(|(p, _)| *p != path);
        c.push((path, Arc::clone(&data)));
        if c.len() > CACHE_SLOTS {
            c.remove(0);
        }
    }
    Ok(data)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ForwardQuery {
    /// Project-relative source file the cursor is in.
    pub file: String,
    /// 1-based line.
    pub line: u32,
    /// The build being viewed. Without it the newest build of the file's
    /// project is used.
    #[serde(default)]
    pub build: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Region {
    pub page: u32,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    /// The line that produced this region (the nearest one after the request
    /// when the requested line typeset nothing, e.g. a comment).
    pub line: u32,
}

/// Smallest region worth showing, in points. `SyncTeX` records many objects as
/// bare points (kerns, glue) with no height; flashing one of those would draw
/// a hairline nobody can see.
const MIN_REGION_PT: f64 = 12.0;

/// The one rectangle to show for a search: everything the best-matching line
/// produced on the first hit's page, grown to at least [`MIN_REGION_PT`] tall
/// around its centre. A paragraph that wraps over several lines comes out as
/// one box covering all of them.
fn line_region(hits: &[synctex::PdfHit]) -> Option<Region> {
    let first = hits.first()?;
    let same = || {
        hits.iter()
            .filter(|h| h.page == first.page && h.line == first.line)
    };
    let left = same().map(|h| h.x).fold(f64::INFINITY, f64::min);
    let top = same().map(|h| h.y).fold(f64::INFINITY, f64::min);
    let right = same()
        .map(|h| h.x + h.width)
        .fold(f64::NEG_INFINITY, f64::max);
    let bottom = same()
        .map(|h| h.y + h.height)
        .fold(f64::NEG_INFINITY, f64::max);

    let (mut y, mut height) = (top, bottom - top);
    if height < MIN_REGION_PT {
        y -= (MIN_REGION_PT - height) / 2.0;
        height = MIN_REGION_PT;
    }
    Some(Region {
        page: first.page,
        x: left,
        y: y.max(0.0),
        width: (right - left).max(MIN_REGION_PT),
        height,
        line: first.line,
    })
}

#[derive(Debug, Serialize)]
pub struct ForwardResponse {
    pub build: String,
    /// The project has changed since this build was compiled.
    pub stale: bool,
    /// What to highlight: the best line's boxes merged into one rectangle.
    /// `None` when the line produced no output at all.
    pub region: Option<Region>,
    /// Best match first; empty when the line produced no output at all.
    pub hits: Vec<Region>,
}

/// `GET /api/synctex/forward?file=&line=&build=`
#[instrument(skip(state))]
pub async fn forward(
    State(state): State<AppState>,
    Query(q): Query<ForwardQuery>,
) -> Result<Json<ForwardResponse>, ApiError> {
    let abs = validate_path(&state.workspace_root, &q.file, AccessKind::Read)?;
    let rel = abs
        .strip_prefix(&state.workspace_root)
        .map_err(|_| ApiError::InvalidPath {
            message: "path leaves the workspace".to_owned(),
        })?
        .to_path_buf();
    if q.line == 0 {
        return Err(ApiError::BadRequest {
            message: "line numbers are 1-based".to_owned(),
        });
    }

    let build = resolve_build(&state, q.build.as_deref(), Some(&q.file)).await?;
    let data = load_map(&build).await?;
    let outdated = !build_is_current(&state, &build).await?;

    let root = state.workspace_root.clone();
    let main = PathBuf::from(&build.manifest.main);
    let line = q.line;
    let hits = tokio::task::spawn_blocking(move || {
        data.tag_for_file(&rel, &root, &main)
            .map(|tag| data.forward_search(tag, line))
    })
    .await
    .map_err(|e| ApiError::Internal {
        message: e.to_string(),
    })?
    .ok_or_else(|| ApiError::NotFound {
        message: format!("{} is not part of that PDF", q.file),
    })?;

    Ok(Json(ForwardResponse {
        build: build.id().to_owned(),
        stale: outdated,
        region: line_region(&hits),
        hits: hits
            .into_iter()
            .take(16)
            .map(|h| Region {
                page: h.page,
                x: h.x,
                y: h.y,
                width: h.width,
                height: h.height,
                line: h.line,
            })
            .collect(),
    }))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackwardQuery {
    pub build: String,
    /// 1-based page.
    pub page: u32,
    /// Points from the page's top-left corner.
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Serialize)]
pub struct BackwardResponse {
    /// Project-relative file.
    pub file: String,
    pub line: u32,
    pub column: Option<u32>,
    /// The point fell inside the matched box rather than merely near it.
    pub exact: bool,
    pub stale: bool,
}

/// `GET /api/synctex/backward?build=&page=&x=&y=`
#[instrument(skip(state))]
pub async fn backward(
    State(state): State<AppState>,
    Query(q): Query<BackwardQuery>,
) -> Result<Json<BackwardResponse>, ApiError> {
    let sane = |v: f64| v.is_finite() && (0.0..=MAX_COORD_PT).contains(&v);
    if q.page == 0 || !sane(q.x) || !sane(q.y) {
        return Err(ApiError::Unprocessable {
            message: "page is 1-based and the point must lie on the page".to_owned(),
        });
    }

    let build = resolve_build(&state, Some(&q.build), None).await?;
    let data = load_map(&build).await?;
    let outdated = !build_is_current(&state, &build).await?;

    let (page, x, y) = (q.page, q.x, q.y);
    let hit = tokio::task::spawn_blocking(move || data.backward_search(page, x, y))
        .await
        .map_err(|e| ApiError::Internal {
            message: e.to_string(),
        })?
        .ok_or(ApiError::SyncTexMiss { page, x, y })?;

    let (file, _) = resolve_hit_path(&state, &build.manifest.main, &hit)?;
    Ok(Json(BackwardResponse {
        file: relative_slashes(Path::new(&file)),
        line: hit.line,
        column: hit.column,
        exact: hit.exact,
        stale: outdated,
    }))
}

fn relative_slashes(path: &Path) -> String {
    path.components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(page: u32, x: f64, y: f64, width: f64, height: f64, line: u32) -> synctex::PdfHit {
        synctex::PdfHit {
            page,
            x,
            y,
            width,
            height,
            line,
        }
    }

    #[test]
    fn a_wrapped_paragraph_becomes_one_box_over_all_its_lines() {
        let region = line_region(&[
            hit(2, 100.0, 200.0, 300.0, 10.0, 7),
            hit(2, 100.0, 214.0, 280.0, 10.0, 7),
            hit(2, 100.0, 228.0, 120.0, 10.0, 7),
            // Another line, another page: not part of it.
            hit(2, 10.0, 10.0, 5.0, 5.0, 8),
            hit(3, 10.0, 10.0, 5.0, 5.0, 7),
        ])
        .unwrap();
        assert_eq!(region.page, 2);
        assert_eq!(region.line, 7);
        assert!((region.x - 100.0).abs() < 1e-9 && (region.width - 300.0).abs() < 1e-9);
        assert!((region.y - 200.0).abs() < 1e-9 && (region.height - 38.0).abs() < 1e-9);
    }

    #[test]
    fn a_point_with_no_height_is_grown_to_something_visible_around_itself() {
        let region = line_region(&[hit(1, 158.0, 156.0, 3.0, 0.0, 4)]).unwrap();
        assert!((region.height - MIN_REGION_PT).abs() < 1e-9);
        assert!((region.y - (156.0 - MIN_REGION_PT / 2.0)).abs() < 1e-9);
        assert!(region.width >= MIN_REGION_PT);
    }

    #[test]
    fn growing_never_pushes_a_region_above_the_page() {
        let region = line_region(&[hit(1, 5.0, 1.0, 40.0, 0.0, 1)]).unwrap();
        assert!(region.y >= 0.0);
    }

    #[test]
    fn no_hits_means_no_region() {
        assert!(line_region(&[]).is_none());
    }
}
