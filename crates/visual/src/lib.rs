//! PDF rasterisation: full-page render and bbox crop via `pdfium`. Sole owner
//! of the `pdfium-render` dependency (masterplan.md §5 rule 6).
#![forbid(unsafe_code)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod crop;
pub mod paper;
pub mod render;

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum VisualError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("PDF processing error: {0}")]
    Pdf(String),
    #[error("Image processing error: {0}")]
    Image(String),
    #[error("Page not found in PDF")]
    PageNotFound,
    #[error("page would render at {width}x{height}px, over the {max_pixels}-pixel limit")]
    TooLarge {
        width: u32,
        height: u32,
        max_pixels: u32,
    },
}
