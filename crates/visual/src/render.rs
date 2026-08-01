//! Full-page PDF rasterization via system `libpdfium`.
//!
//! pdfium can only be bound **once per process** — a second
//! `bind_to_system_library()` fails with
//! `PdfiumLibraryBindingsAlreadyInitialized`, so every render after the first
//! would break. The binding therefore lives in a `OnceLock` and callers share
//! it via [`renderer`]. The `thread_safe` feature makes `Pdfium` `Sync` by
//! serializing calls internally (masterplan.md §2.1).

use crate::VisualError;
use pdfium_render::prelude::*;
use std::path::Path;
use std::sync::OnceLock;

/// Process-global pdfium binding. Stays `None` if binding failed, so we do not
/// retry (and re-fail) on every request.
static PDFIUM: OnceLock<Option<Pdfium>> = OnceLock::new();

/// Borrow the shared renderer, binding pdfium on first use. Cheap to call.
///
/// # Errors
/// [`VisualError::Pdf`] if no pdfium library can be bound.
pub fn renderer() -> Result<PdfRenderer, VisualError> {
    let bound = PDFIUM.get_or_init(|| {
        match Pdfium::bind_to_system_library().or_else(|_| Pdfium::bind_to_library("pdfium")) {
            Ok(bindings) => Some(Pdfium::new(bindings)),
            Err(e) => {
                tracing::error!(error = ?e, "failed to bind pdfium; PDF rendering disabled");
                None
            }
        }
    });

    bound.as_ref().map_or_else(
        || {
            Err(VisualError::Pdf(
                "pdfium library unavailable (install libpdfium)".to_owned(),
            ))
        },
        |pdfium| Ok(PdfRenderer { pdfium }),
    )
}

/// Renders pages from PDF files. Obtain one with [`renderer`]; it borrows the
/// process-wide binding rather than creating a new one.
#[derive(Clone, Copy)]
pub struct PdfRenderer {
    pdfium: &'static Pdfium,
}

impl PdfRenderer {
    /// Number of pages in the document.
    ///
    /// # Errors
    /// [`VisualError::Pdf`] when the document cannot be loaded.
    pub fn page_count(self, pdf_path: &Path) -> Result<u16, VisualError> {
        let document = self
            .pdfium
            .load_pdf_from_file(pdf_path, None)
            .map_err(|e| VisualError::Pdf(e.to_string()))?;
        u16::try_from(document.pages().len())
            .map_err(|_| VisualError::Pdf("page count out of range".to_owned()))
    }

    /// Render `page_index` (0-based) at `dpi`.
    ///
    /// # Errors
    /// [`VisualError::PageNotFound`] for an out-of-range page,
    /// [`VisualError::Pdf`] for load/render failures.
    pub fn render_page(
        self,
        pdf_path: &Path,
        page_index: u32,
        dpi: u16,
    ) -> Result<image::DynamicImage, VisualError> {
        let document = self
            .pdfium
            .load_pdf_from_file(pdf_path, None)
            .map_err(|e| VisualError::Pdf(e.to_string()))?;

        let pages = document.pages();
        let index = i32::try_from(page_index).map_err(|_| VisualError::PageNotFound)?;
        if index >= pages.len() {
            return Err(VisualError::PageNotFound);
        }

        let page = pages
            .get(index)
            .map_err(|e| VisualError::Pdf(e.to_string()))?;

        // Points (1/72 in) -> pixels at the requested DPI. Pages stay far below
        // the i32 pixel range at any sane DPI; clamp rather than wrap.
        let scale = f32::from(dpi) / 72.0;
        let px = |points: f32| -> i32 {
            let v = (points * scale).round();
            if v >= 2_147_483_647.0 {
                i32::MAX
            } else if v <= 1.0 {
                1
            } else {
                #[allow(clippy::cast_possible_truncation)]
                {
                    v as i32
                }
            }
        };

        let render_config = PdfRenderConfig::new()
            .set_target_width(px(page.width().value))
            .set_maximum_height(px(page.height().value));

        let bitmap = page
            .render_with_config(&render_config)
            .map_err(|e| VisualError::Pdf(e.to_string()))?;

        bitmap
            .as_image()
            .map_err(|e| VisualError::Pdf(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression: binding twice used to fail with
    /// `PdfiumLibraryBindingsAlreadyInitialized`, breaking every render after
    /// the first in a long-running server.
    #[test]
    fn renderer_is_reusable_across_calls() {
        let first = renderer();
        let second = renderer();
        assert_eq!(
            first.is_ok(),
            second.is_ok(),
            "second renderer() must not fail when the first succeeded"
        );
    }
}
