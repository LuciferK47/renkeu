//! Visual grounding (masterplan.md §6.4, §12 M3): the user drags a box over
//! the rendered PDF, `SyncTeX` maps it back to the source line that produced
//! it, and a vision model proposes bounded edits.
//!
//! The proposal is never auto-applied — the user accepts it through
//! `POST /api/agent/apply-selected` (pillar 3, deterministic diff control).

use crate::AppState;
use crate::error::ApiError;
use axum::Json;
use axum::extract::{Query, State};
use axum::http::{StatusCode, header};
use axum::response::IntoResponse;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use latex_ai_core::ai::VisualFixRequest as PlannerRequest;
use latex_ai_core::security::{AccessKind, validate_path};
use serde::{Deserialize, Serialize};
use tracing::instrument;

/// Resolution the region sent to the model is rasterized at. Independent of
/// whatever zoom the viewer is at: boxes arrive in PDF points, which do not
/// change with zoom, device pixel ratio or the DPI a page image was served at.
const CROP_DPI: u16 = 200;

/// DPI a page image is served at when the request does not say.
const DEFAULT_DPI: u16 = 150;

/// Source context sent with the crop, in lines either side of the hit
/// (masterplan §6.4).
const SNIPPET_CONTEXT_LINES: u32 = 30;

/// Guard against absurd crops long before pdfium sees them. A page larger
/// than this in either direction is not a page.
const MAX_BBOX_PT: f64 = 5_000.0;

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct VisualFixPayload {
    /// The source file the user is looking at; used to find the latest build
    /// when `build` is not given.
    pub file_path: String,
    /// The build the page image came from. Pass it: the document may have
    /// been recompiled since, and a box drawn on one build means nothing on
    /// another.
    #[serde(default)]
    pub build: Option<String>,
    /// 1-based page number.
    pub page: u32,
    /// Bounding box in **PDF points** (1/72 inch) from the page's top-left
    /// corner: the unit `SyncTeX` answers in, and one that does not depend on
    /// how the page was displayed.
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    /// What the user wants changed. Optional: an empty box still means "fix
    /// whatever is wrong here".
    #[serde(default)]
    pub instruction: Option<String>,
}

#[derive(Serialize, Debug)]
pub struct VisualFixResponse {
    /// Workspace-relative source file the hit resolved to.
    pub file: String,
    /// 1-based source line.
    pub line: u32,
    /// `false` when the bbox fell outside every typeset box and the nearest
    /// one was used instead.
    pub exact: bool,
    /// The source lines sent to the model as context.
    pub snippet: String,
    pub snippet_start_line: u32,
    /// `ETag` of `file` as the proposal saw it: send it back as
    /// `expected_etag` when applying.
    pub etag: String,
    /// The build the box was drawn on.
    pub build: String,
    /// Base64 PNG of the crop, so the UI can show exactly what was analysed.
    pub base64_crop: String,
    pub edits: Vec<latex_ai_core::compile::AgentEdit>,
}

/// `POST /api/visual-fix`
#[instrument(skip(state))]
pub async fn visual_fix(
    State(state): State<AppState>,
    Json(req): Json<VisualFixPayload>,
) -> Result<Json<VisualFixResponse>, ApiError> {
    super::agent::ensure_ai_available(&state)?;
    validate_path(&state.workspace_root, &req.file_path, AccessKind::Read)?;
    validate_bbox(&req)?;

    let build = resolve_build(&state, req.build.as_deref(), Some(&req.file_path)).await?;
    ensure_build_is_current(&state, &build).await?;
    let pdf_path = build.pdf_path();
    let synctex_path = build.synctex_path();
    if !build.manifest.has_synctex || !synctex_path.exists() {
        return Err(ApiError::NotFound {
            message: "this build has no SyncTeX map — recompile to generate one".to_owned(),
        });
    }

    // --- reverse search -------------------------------------------------
    let data = tokio::task::spawn_blocking(move || synctex::SyncTexData::parse(&synctex_path))
        .await
        .map_err(|e| ApiError::Internal {
            message: e.to_string(),
        })?
        .map_err(|e| ApiError::Internal {
            message: format!("reading the SyncTeX map: {e}"),
        })?;

    // Probe the centre of the box — that is what the user pointed at.
    let probe_x = req.x + req.w / 2.0;
    let probe_y = req.y + req.h / 2.0;

    let hit = data
        .backward_search(req.page, probe_x, probe_y)
        .ok_or(ApiError::SyncTexMiss {
            page: req.page,
            x: probe_x,
            y: probe_y,
        })?;

    // SyncTeX records Tectonic's internal name for the primary input, not a
    // usable path, so single-document hits resolve to the file we compiled.
    // Anything else is re-validated against the workspace before we read it
    // (masterplan §12 M3 audit: security).
    let (rel_path, abs_hit_path) = resolve_hit_path(&state, &build.manifest.main, &hit)?;

    // --- source snippet -------------------------------------------------
    let source = tokio::fs::read_to_string(&abs_hit_path)
        .await
        .map_err(|e| ApiError::Internal {
            message: format!("reading the source file: {e}"),
        })?;
    let (snippet, snippet_start_line) = snippet_around(&source, hit.line, SNIPPET_CONTEXT_LINES);

    // --- crop -----------------------------------------------------------
    let base64_crop = render_crop(pdf_path, &req).await?;

    // --- ask the model --------------------------------------------------
    let instruction = sanitized_instruction(req.instruction.as_deref())?;

    state.metrics.record_llm_call();
    let mut edits = state
        .planner
        .visual_fix(
            PlannerRequest {
                instruction: &instruction,
                png_base64: &base64_crop,
                file_path: Some(&rel_path),
                line: Some(hit.line),
                snippet: &snippet,
            },
            tokio_util::sync::CancellationToken::new(),
        )
        .await
        .map_err(|e| match e {
            latex_ai_core::ai::AiError::Parse(err) => {
                tracing::warn!(error = %err, "VLM returned an unusable shape");
                ApiError::VlmParseError {
                    message: err.to_string(),
                }
            }
            other => ApiError::Internal {
                message: other.to_string(),
            },
        })?;

    latex_ai_core::policy::lint_edits(&edits, &source).map_err(|v| ApiError::Unprocessable {
        message: format!("The AI proposed an unsafe edit: {v}"),
    })?;

    // Same stamping the agent routes do, so the proposal is applicable through
    // POST /api/agent/apply-selected (masterplan §12 M3 audit: integration).
    crate::agent::stamp_hunk_ids(&mut edits);

    Ok(Json(VisualFixResponse {
        file: rel_path,
        line: hit.line,
        exact: hit.exact,
        snippet,
        snippet_start_line,
        etag: latex_ai_core::storage::etag_of(&source),
        build: build.id().to_owned(),
        base64_crop,
        edits,
    }))
}

/// Reject nonsensical boxes up front.
fn validate_bbox(req: &VisualFixPayload) -> Result<(), ApiError> {
    let sane = |v: f64| v.is_finite() && (0.0..=MAX_BBOX_PT).contains(&v);
    if !(sane(req.x) && sane(req.y) && sane(req.w) && sane(req.h)) {
        return Err(ApiError::Unprocessable {
            message: format!("bounding box must be finite and within 0..{MAX_BBOX_PT} points"),
        });
    }
    if req.w <= 0.0 || req.h <= 0.0 {
        return Err(ApiError::Unprocessable {
            message: "bounding box has zero width or height".to_owned(),
        });
    }
    if req.page == 0 {
        return Err(ApiError::Unprocessable {
            message: "page numbers are 1-based".to_owned(),
        });
    }
    Ok(())
}

/// Points -> pixels at [`CROP_DPI`], rounded; at least 1 for sizes.
fn pt_to_px(pt: f64) -> u32 {
    let px = (pt * f64::from(CROP_DPI) / 72.0).round();
    // Bounded by MAX_BBOX_PT * 200/72, far below u32::MAX.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    {
        px.max(0.0) as u32
    }
}

/// Rasterize the page and crop the bbox, returning base64 PNG. The crop is
/// capped at 512px on its long edge inside `visual::crop` (masterplan §12 M3
/// audit: security).
// x, y, w, h are the bounding box; naming them in full would only add noise.
#[allow(clippy::many_single_char_names)]
async fn render_crop(
    pdf_path: std::path::PathBuf,
    req: &VisualFixPayload,
) -> Result<String, ApiError> {
    let page = req.page;
    let (x, y) = (pt_to_px(req.x), pt_to_px(req.y));
    let (w, h) = (pt_to_px(req.w).max(1), pt_to_px(req.h).max(1));

    let crop = tokio::task::spawn_blocking(
        move || -> Result<std::sync::Arc<[u8]>, ::visual::VisualError> {
            let renderer = ::visual::render::renderer()?;
            let img = renderer.render_page(&pdf_path, page - 1, CROP_DPI)?;
            // Deliberately not tone-mapped. The model is being asked what is
            // wrong with the document as it will print, so it gets the true
            // colours; toning is a reading comfort for the human viewer only.
            ::visual::crop::crop_image(&img, x, y, w, h)
        },
    )
    .await
    .map_err(|e| ApiError::Internal {
        message: e.to_string(),
    })?
    .map_err(|e| match e {
        ::visual::VisualError::PageNotFound => ApiError::NotFound {
            message: format!("page {page} is out of range"),
        },
        // A box outside the page is the caller's mistake, not a server fault.
        ::visual::VisualError::Image(message) => ApiError::Unprocessable { message },
        e @ ::visual::VisualError::TooLarge { .. } => ApiError::Unprocessable {
            message: e.to_string(),
        },
        other => ApiError::Internal {
            message: other.to_string(),
        },
    })?;

    Ok(STANDARD.encode(&*crop))
}

/// An absent or blank instruction still means "fix what is wrong here".
fn sanitized_instruction(raw: Option<&str>) -> Result<String, ApiError> {
    let instruction = raw
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("Fix the layout problem visible in this region.");

    latex_ai_core::prompt_safety::sanitize(instruction, 10_000).map_err(|e| {
        ApiError::Unprocessable {
            message: e.to_string(),
        }
    })
}

/// Decide which source file a hit refers to, re-validating anything that did
/// not come from us.
///
/// `SyncTeX` records absolute paths for the project's files; a name that is
/// empty, a placeholder, or outside the workspace means "the main file".
pub(crate) fn resolve_hit_path(
    state: &AppState,
    main: &str,
    hit: &synctex::SourceHit,
) -> Result<(String, std::path::PathBuf), ApiError> {
    let rel = synctex::resolve_input_name(&hit.file, &state.workspace_root)
        .map_or_else(|| main.to_owned(), |p| latex_ai_core::fsutil::to_slash(&p));
    let abs = validate_path(&state.workspace_root, &rel, AccessKind::Read)?;
    Ok((rel, abs))
}

/// A box drawn on a PDF means something only for the source it was compiled
/// from. If the project has changed since (the build id is a fingerprint of
/// every input), `SyncTeX` would map the box to the wrong line and the model
/// would be asked to "fix" the wrong text: refuse, and say to recompile.
async fn ensure_build_is_current(
    state: &AppState,
    build: &latex_ai_compiler::builds::Build,
) -> Result<(), ApiError> {
    if build_is_current(state, build).await? {
        Ok(())
    } else {
        Err(ApiError::Conflict {
            message: "the document changed since this PDF was compiled; compile again, then select the region"
                .to_owned(),
        })
    }
}

/// Whether the project still hashes to this build's id.
pub(crate) async fn build_is_current(
    state: &AppState,
    build: &latex_ai_compiler::builds::Build,
) -> Result<bool, ApiError> {
    let (root, main) = (state.workspace_root.clone(), build.manifest.main.clone());
    let current = tokio::task::spawn_blocking(move || {
        latex_ai_compiler::project::fingerprint(&root, &main, true)
    })
    .await
    .map_err(|e| ApiError::Internal {
        message: e.to_string(),
    })?
    .map_err(|e| ApiError::Internal {
        message: format!("reading the project: {e}"),
    })?;
    Ok(current == build.id())
}

/// Find the build a request is about: the one it names, else the newest
/// successful build of the project `file` belongs to.
pub(crate) async fn resolve_build(
    state: &AppState,
    build: Option<&str>,
    file: Option<&str>,
) -> Result<latex_ai_compiler::builds::Build, ApiError> {
    use latex_ai_compiler::builds::is_valid_id;

    let builds = state.compiler.builds();
    if let Some(id) = build {
        if !is_valid_id(id) {
            return Err(ApiError::BadRequest {
                message: "malformed build id".to_owned(),
            });
        }
        return builds.get(id).await.ok_or_else(|| ApiError::NotFound {
            message: "that build is gone — compile again".to_owned(),
        });
    }
    let file = file.ok_or_else(|| ApiError::BadRequest {
        message: "give a `build` or a `file`".to_owned(),
    })?;
    validate_path(&state.workspace_root, file, AccessKind::Read)?;
    let main = super::compile::resolve_main(state, file, None).await?;
    builds
        .latest_with_pdf(&main)
        .await
        .ok_or_else(|| ApiError::NotFound {
            message: "no compiled PDF for this file — compile first".to_owned(),
        })
}

/// `context` lines either side of `line` (1-based). Returns the text and the
/// 1-based number of its first line.
fn snippet_around(source: &str, line: u32, context: u32) -> (String, u32) {
    let lines: Vec<&str> = source.lines().collect();
    if lines.is_empty() {
        return (String::new(), 1);
    }
    let total = u32::try_from(lines.len()).unwrap_or(u32::MAX);
    let target = line.clamp(1, total);

    let start = target.saturating_sub(context).max(1);
    let end = target.saturating_add(context).min(total);

    let from = usize::try_from(start - 1).unwrap_or(0);
    let to = usize::try_from(end).unwrap_or(lines.len()).min(lines.len());
    (lines[from..to].join("\n"), start)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PageQuery {
    /// The build to render (preferred; the URL then never goes stale).
    #[serde(default)]
    pub build: Option<String>,
    /// Or: the source file whose latest build to render.
    #[serde(default)]
    pub file: Option<String>,
    pub n: u32,
    pub dpi: Option<u16>,
    /// Paper lightness, 0.6..=1.0. Omitted means the server default; `1.0`
    /// means send the page exactly as it will print, which is what the
    /// hold-to-reveal control asks for.
    pub paper: Option<f32>,
    /// Warm shift for the paper surface, 0..=40.
    pub warmth: Option<f32>,
}

/// `GET /api/pdf/page?file=&n=&dpi=&paper=&warmth=` — rasterize one page of the
/// compiled PDF (masterplan §8.3; replaces PDF.js).
///
/// `paper` and `warmth` tone the page for a dark UI without touching hue; see
/// [`visual::paper`] for why that is done here rather than with a CSS filter.
#[instrument(skip(state))]
pub async fn get_pdf_page(
    State(state): State<AppState>,
    Query(query): Query<PageQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let build = resolve_build(&state, query.build.as_deref(), query.file.as_deref()).await?;
    if !build.manifest.has_pdf {
        return Err(ApiError::NotFound {
            message: "that build produced no PDF".to_owned(),
        });
    }
    let pdf_path = build.pdf_path();

    let dpi = query.dpi.unwrap_or(DEFAULT_DPI).clamp(36, 300);
    let page_index = query.n.saturating_sub(1);

    let default_tone = ::visual::paper::PaperTone::default();
    let tone = ::visual::paper::PaperTone::new(
        query.paper.unwrap_or(default_tone.lightness),
        query.warmth.unwrap_or(default_tone.warmth),
    );

    let png = tokio::task::spawn_blocking(move || -> Result<Vec<u8>, ::visual::VisualError> {
        let renderer = ::visual::render::renderer()?;
        let img = renderer.render_page(&pdf_path, page_index, dpi)?;
        let img = ::visual::paper::tone_map(img, tone);
        let mut buf = Vec::new();
        let mut cursor = std::io::Cursor::new(&mut buf);
        img.write_to(&mut cursor, image::ImageFormat::Png)
            .map_err(|e| ::visual::VisualError::Image(e.to_string()))?;
        Ok(buf)
    })
    .await
    .map_err(|e| ApiError::Internal {
        message: e.to_string(),
    })?
    // A page number past the end is the caller's mistake, not a server fault.
    .map_err(|e| match e {
        ::visual::VisualError::PageNotFound => ApiError::NotFound {
            message: format!("page {} is out of range", query.n),
        },
        e @ ::visual::VisualError::TooLarge { .. } => ApiError::Unprocessable {
            message: e.to_string(),
        },
        other => ApiError::Internal {
            message: other.to_string(),
        },
    })?;

    // A page of a named build never changes, so it may be cached for good.
    // Addressing by file means "whatever is newest", which may.
    let cache = if query.build.is_some() {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "image/png"),
            (header::CACHE_CONTROL, cache),
        ],
        png,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn points_convert_to_crop_pixels() {
        // One inch is 72 points and CROP_DPI pixels.
        assert_eq!(pt_to_px(72.0), u32::from(CROP_DPI));
        assert_eq!(pt_to_px(0.0), 0);
        assert_eq!(pt_to_px(-5.0), 0, "negative never wraps");
    }

    fn payload(x: f64, y: f64, w: f64, h: f64, page: u32) -> VisualFixPayload {
        VisualFixPayload {
            file_path: "main.tex".into(),
            build: None,
            page,
            x,
            y,
            w,
            h,
            instruction: None,
        }
    }

    #[test]
    fn boxes_must_be_finite_positive_and_bounded() {
        assert!(validate_bbox(&payload(10.0, 10.0, 50.0, 50.0, 1)).is_ok());
        for bad in [
            payload(10.0, 10.0, 0.0, 50.0, 1),
            payload(10.0, 10.0, 50.0, 0.0, 1),
            payload(10.0, 10.0, 50.0, 50.0, 0),
            payload(f64::NAN, 10.0, 50.0, 50.0, 1),
            payload(10.0, f64::INFINITY, 50.0, 50.0, 1),
            payload(-1.0, 10.0, 50.0, 50.0, 1),
            payload(10.0, 10.0, MAX_BBOX_PT + 1.0, 50.0, 1),
        ] {
            assert!(validate_bbox(&bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn snippet_is_centred_and_clamped_to_the_file() {
        let source = (1..=100)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");

        let (snippet, start) = snippet_around(&source, 50, 5);
        assert_eq!(start, 45);
        assert_eq!(snippet.lines().next(), Some("line 45"));
        assert_eq!(snippet.lines().last(), Some("line 55"));

        // Near the top: clamps at line 1 rather than underflowing.
        let (snippet, start) = snippet_around(&source, 2, 10);
        assert_eq!(start, 1);
        assert_eq!(snippet.lines().next(), Some("line 1"));

        // Near the end: clamps at the last line.
        let (snippet, start) = snippet_around(&source, 99, 10);
        assert_eq!(start, 89);
        assert_eq!(snippet.lines().last(), Some("line 100"));
    }

    #[test]
    fn snippet_handles_empty_and_out_of_range_input() {
        assert_eq!(snippet_around("", 5, 10), (String::new(), 1));
        let (snippet, start) = snippet_around("only line", 999, 3);
        assert_eq!((snippet.as_str(), start), ("only line", 1));
    }
}
