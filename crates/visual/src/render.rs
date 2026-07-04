use crate::VisualError;
use pdfium_render::prelude::*;
use std::path::Path;

pub struct PdfRenderer {
    pdfium: Pdfium,
}

impl PdfRenderer {
    pub fn new() -> Result<Self, VisualError> {
        let bindings = Pdfium::bind_to_system_library()
            .or_else(|_| Pdfium::bind_to_library("pdfium"))
            .map_err(|e| VisualError::Pdf(format!("Failed to bind to pdfium: {:?}", e)))?;
        
        let pdfium = Pdfium::new(bindings);
        Ok(Self { pdfium })
    }

    pub fn render_page(&self, pdf_path: &Path, page_index: u32, dpi: u16) -> Result<image::DynamicImage, VisualError> {
        let document = self.pdfium.load_pdf_from_file(pdf_path, None)
            .map_err(|e| VisualError::Pdf(e.to_string()))?;
        
        let pages = document.pages();
        if i32::from(page_index as u16) >= pages.len() as i32 {
            return Err(VisualError::PageNotFound);
        }

        let page = pages.get((page_index as u16).into())
            .map_err(|e| VisualError::Pdf(e.to_string()))?;
            
        let render_config = PdfRenderConfig::new()
            .set_target_width((page.width().value * dpi as f32 / 72.0) as i32)
            .set_maximum_height((page.height().value * dpi as f32 / 72.0) as i32);
            
        let bitmap = page.render_with_config(&render_config)
            .map_err(|e| VisualError::Pdf(e.to_string()))?;
            
        let image = bitmap.as_image().map_err(|e| VisualError::Pdf(e.to_string()))?;
        
        Ok(image)
    }
}
