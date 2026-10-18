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
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// Process-global pdfium binding. Stays `None` if binding failed, so we do not
/// retry (and re-fail) on every request.
static PDFIUM: OnceLock<Option<Pdfium>> = OnceLock::new();

/// Where the binding came from, for `doctor` and for the log line.
static SOURCE: OnceLock<String> = OnceLock::new();

/// Directories the application added before the first render, searched after
/// `LATEX_AI_PDFIUM_DIR`. The desktop shell adds its bundled-resources folder.
static EXTRA_DIRS: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// Environment variable naming a directory that holds the pdfium library.
pub const DIR_ENV: &str = "LATEX_AI_PDFIUM_DIR";

/// Add a directory to search for the pdfium library. Only effective before the
/// first call to [`renderer`] (the binding is made once per process).
pub fn add_search_dir(dir: impl Into<PathBuf>) {
    if let Ok(mut dirs) = EXTRA_DIRS.lock() {
        dirs.push(dir.into());
    }
}

/// Directories to look in, most specific first: the environment, anything the
/// application registered, then beside the executable (a portable install).
fn search_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(d) = std::env::var_os(DIR_ENV).filter(|d| !d.is_empty()) {
        dirs.push(PathBuf::from(d));
    }
    if let Ok(extra) = EXTRA_DIRS.lock() {
        dirs.extend(extra.iter().cloned());
    }
    if let Some(exe_dir) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
    {
        dirs.push(exe_dir.join("lib"));
        dirs.push(exe_dir);
    }
    dirs
}

fn bind() -> Option<Pdfium> {
    for dir in search_dirs() {
        let path = Pdfium::pdfium_platform_library_name_at_path(&dir);
        if !path.is_file() {
            continue;
        }
        match Pdfium::bind_to_library(&path) {
            Ok(bindings) => {
                let _ = SOURCE.set(path.display().to_string());
                return Some(Pdfium::new(bindings));
            }
            // Found but unusable (wrong architecture, missing dependency):
            // say so, and keep looking rather than hiding it.
            Err(e) => {
                tracing::warn!(path = %path.display(), error = ?e, "pdfium found but could not be loaded");
            }
        }
    }
    match Pdfium::bind_to_system_library().or_else(|_| Pdfium::bind_to_library("pdfium")) {
        Ok(bindings) => {
            let _ = SOURCE.set("the system library path".to_owned());
            Some(Pdfium::new(bindings))
        }
        Err(e) => {
            tracing::error!(error = ?e, "failed to bind pdfium; PDF rendering disabled");
            None
        }
    }
}

/// Borrow the shared renderer, binding pdfium on first use. Cheap to call.
///
/// # Errors
/// [`VisualError::Pdf`] if no pdfium library can be bound.
pub fn renderer() -> Result<PdfRenderer, VisualError> {
    let bound = PDFIUM.get_or_init(bind);

    bound.as_ref().map_or_else(
        || {
            Err(VisualError::Pdf(format!(
                "pdfium library unavailable: set {DIR_ENV} to the folder holding it, \
                 or install libpdfium (see the README)"
            )))
        },
        |pdfium| Ok(PdfRenderer { pdfium }),
    )
}

/// Bind pdfium now and say where it came from.
///
/// # Errors
/// As [`renderer`].
pub fn library_source() -> Result<String, VisualError> {
    renderer()?;
    Ok(SOURCE
        .get()
        .cloned()
        .unwrap_or_else(|| "an unknown location".to_owned()))
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

        let (target_w, target_h) = (px(page.width().value), px(page.height().value));
        // A document can declare any \paperwidth, so dpi alone does not bound
        // the bitmap: one hostile page at 300 dpi would otherwise ask pdfium
        // for gigabytes.
        check_pixel_budget(target_w, target_h)?;

        let render_config = PdfRenderConfig::new()
            .set_target_width(target_w)
            .set_maximum_height(target_h);

        let bitmap = page
            .render_with_config(&render_config)
            .map_err(|e| VisualError::Pdf(e.to_string()))?;

        bitmap
            .as_image()
            .map_err(|e| VisualError::Pdf(e.to_string()))
    }
}

/// Upper bound on one rendered page. A4 at the 300 dpi ceiling is ~8.7 M
/// pixels; 64 M leaves room for large posters while capping the RGBA bitmap
/// at 256 MB.
pub const MAX_RENDER_PIXELS: u32 = 64_000_000;

fn check_pixel_budget(width: i32, height: i32) -> Result<(), VisualError> {
    let w = u32::try_from(width).unwrap_or(u32::MAX);
    let h = u32::try_from(height).unwrap_or(u32::MAX);
    match w.checked_mul(h) {
        Some(total) if total <= MAX_RENDER_PIXELS => Ok(()),
        _ => Err(VisualError::TooLarge {
            width: w,
            height: h,
            max_pixels: MAX_RENDER_PIXELS,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a4_at_the_dpi_ceiling_fits_the_budget() {
        // 595x842 pt at 300 dpi.
        assert!(check_pixel_budget(2480, 3508).is_ok());
    }

    #[test]
    fn an_enormous_page_is_refused_instead_of_allocated() {
        assert!(matches!(
            check_pixel_budget(200_000, 200_000),
            Err(VisualError::TooLarge { .. })
        ));
        // The i32::MAX clamp used for absurd page sizes must not overflow.
        assert!(matches!(
            check_pixel_budget(i32::MAX, i32::MAX),
            Err(VisualError::TooLarge { .. })
        ));
    }

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
