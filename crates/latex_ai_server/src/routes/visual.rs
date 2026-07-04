use axum::{
    extract::State,
    Json,
};
use serde::{Deserialize, Serialize};
use tracing::instrument;
use crate::AppState;
use latex_ai_core::ai::EditPlanner;

#[derive(Deserialize, Debug)]
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

#[instrument(skip(_state))]
pub async fn visual_fix(
    State(_state): State<AppState>,
    Json(req): Json<VisualFixRequest>,
) -> Json<VisualFixResponse> {
    let pdf_path = match std::env::current_dir() {
        Ok(dir) => dir.join(&req.file_path).with_extension("pdf"),
        Err(_) => return Json(VisualFixResponse { file: None, line: None, base64_crop: None, edits: vec![] }),
    };

    let synctex_path = pdf_path.with_extension("synctex.gz");

    let mut hit_file = None;
    let mut hit_line = None;
    let mut b64_crop = None;

    if let Ok(data) = synctex::SyncTexData::parse(&synctex_path) {
        if let Some(hit) = data.backward_search(req.page, req.x as i32, req.y as i32) {
            hit_file = Some(hit.file);
            hit_line = Some(hit.line);
        }
    }

    if let Ok(renderer) = ::visual::render::PdfRenderer::new() {
        if let Ok(img) = renderer.render_page(&pdf_path, req.page.saturating_sub(1), 150) {
            if let Ok(cropped) = ::visual::crop::crop_image(&img, req.x, req.y, req.w, req.h) {
                use base64::{Engine as _, engine::general_purpose::STANDARD};
                b64_crop = Some(STANDARD.encode(&*cropped));
            }
        }
    }

    let b64_crop_ref = b64_crop.as_deref().unwrap_or("");
    let hit_file_ref = hit_file.as_deref();
    
    // Call VLM with the crop
    let edits = match _state.planner.visual_fix(
        "Fix the layout issue shown in this bounding box.",
        b64_crop_ref,
        hit_file_ref,
        hit_line,
        tokio_util::sync::CancellationToken::new(),
    ).await {
        Ok(e) => e,
        Err(err) => {
            tracing::error!("VLM error: {:?}", err);
            vec![]
        }
    };

    Json(VisualFixResponse {
        file: hit_file,
        line: hit_line,
        base64_crop: b64_crop,
        edits,
    })
}

use axum::extract::Query;
use axum::response::IntoResponse;
use axum::http::{StatusCode, header};

#[derive(Deserialize)]
pub struct PageQuery {
    pub file: String,
    pub n: u32,
    pub dpi: Option<u16>,
}

pub async fn get_pdf_page(Query(query): Query<PageQuery>) -> impl IntoResponse {
    let pdf_path = match std::env::current_dir() {
        Ok(dir) => dir.join(&query.file).with_extension("pdf"),
        Err(_) => return (StatusCode::NOT_FOUND, "Not found".to_string()).into_response(),
    };

    if let Ok(renderer) = ::visual::render::PdfRenderer::new() {
        if let Ok(img) = renderer.render_page(&pdf_path, query.n.saturating_sub(1), query.dpi.unwrap_or(150)) {
            let mut buf = Vec::new();
            let mut cursor = std::io::Cursor::new(&mut buf);
            if img.write_to(&mut cursor, image::ImageFormat::Png).is_ok() {
                return (
                    StatusCode::OK,
                    [(header::CONTENT_TYPE, "image/png")],
                    buf,
                ).into_response();
            }
        }
    }
    
    (StatusCode::INTERNAL_SERVER_ERROR, "Failed to render".to_string()).into_response()
}
