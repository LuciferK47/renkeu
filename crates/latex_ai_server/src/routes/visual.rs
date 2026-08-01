//! Visual-fix + PDF page routes. M3 scaffolding: coordinate-space conversion
//! and the full §12-M3 contract land with that milestone; paths are already
//! sandboxed and the planner call is real.

use crate::AppState;
use crate::error::ApiError;
use axum::Json;
use axum::extract::{Query, State};
use axum::http::{StatusCode, header};
use axum::response::IntoResponse;
use latex_ai_core::security::{AccessKind, validate_path};
use serde::{Deserialize, Serialize};
use tracing::instrument;

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct VisualFixRequest {
    pub file_path: String,
    pub page: u32,
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

#[derive(Serialize, Debug)]
pub struct VisualFixResponse {
    pub file: Option<String>,
    pub line: Option<u32>,
    pub base64_crop: Option<String>,
    pub edits: Vec<latex_ai_core::compile::AgentEdit>,
}

/// `POST /api/visual-fix` — bbox on the rendered PDF -> `SyncTeX` source hit +
/// crop -> planner proposal (never auto-applied).
#[instrument(skip(state))]
pub async fn visual_fix(
    State(state): State<AppState>,
    Json(req): Json<VisualFixRequest>,
) -> Result<Json<VisualFixResponse>, ApiError> {
    let abs_source = validate_path(&state.workspace_root, &req.file_path, AccessKind::Read)?;
    let pdf_path = abs_source.with_extension("pdf");
    let synctex_path = abs_source.with_extension("synctex.gz");

    let mut hit_file = None;
    let mut hit_line = None;
    let mut b64_crop = None;

    if let Ok(data) = synctex::SyncTexData::parse(&synctex_path) {
        if let Some(hit) = data.backward_search(
            req.page,
            i32::try_from(req.x).unwrap_or(i32::MAX),
            i32::try_from(req.y).unwrap_or(i32::MAX),
        ) {
            hit_file = Some(hit.file);
            hit_line = Some(hit.line);
        }
    }

    if let Ok(renderer) = ::visual::render::renderer() {
        if let Ok(img) = renderer.render_page(&pdf_path, req.page.saturating_sub(1), 150) {
            if let Ok(cropped) = ::visual::crop::crop_image(&img, req.x, req.y, req.w, req.h) {
                use base64::{Engine as _, engine::general_purpose::STANDARD};
                b64_crop = Some(STANDARD.encode(&*cropped));
            }
        }
    }

    let b64_crop_ref = b64_crop.as_deref().unwrap_or("");
    let edits = match state
        .planner
        .visual_fix(
            "Fix the layout issue shown in this bounding box.",
            b64_crop_ref,
            hit_file.as_deref(),
            hit_line,
            tokio_util::sync::CancellationToken::new(),
        )
        .await
    {
        Ok(e) => e,
        Err(err) => {
            tracing::error!(error = ?err, "VLM error");
            vec![]
        }
    };

    Ok(Json(VisualFixResponse {
        file: hit_file,
        line: hit_line,
        base64_crop: b64_crop,
        edits,
    }))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PageQuery {
    pub file: String,
    pub n: u32,
    pub dpi: Option<u16>,
}

/// `GET /api/pdf/page?file=&n=&dpi=` — rasterize one page of the compiled PDF
/// (masterplan §8.3; replaces PDF.js).
#[instrument(skip(state))]
pub async fn get_pdf_page(
    State(state): State<AppState>,
    Query(query): Query<PageQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let abs_source = validate_path(&state.workspace_root, &query.file, AccessKind::Read)?;
    let pdf_path = abs_source.with_extension("pdf");
    if !pdf_path.exists() {
        return Err(ApiError::NotFound {
            message: "no compiled PDF for this file — compile first".to_owned(),
        });
    }

    let dpi = query.dpi.unwrap_or(150).clamp(36, 300);
    let page_index = query.n.saturating_sub(1);

    let png = tokio::task::spawn_blocking(move || -> Result<Vec<u8>, ::visual::VisualError> {
        let renderer = ::visual::render::renderer()?;
        let img = renderer.render_page(&pdf_path, page_index, dpi)?;
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
        other => ApiError::Internal {
            message: other.to_string(),
        },
    })?;

    Ok((StatusCode::OK, [(header::CONTENT_TYPE, "image/png")], png))
}
