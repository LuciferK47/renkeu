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

    /// Names of the fonts the document uses without embedding them (the
    /// standard 14 included: a venue that wants every font embedded wants
    /// those too). Sorted and de-duplicated.
    ///
    /// # Errors
    /// [`VisualError::Pdf`] when the document cannot be loaded.
    pub fn unembedded_fonts(self, pdf_path: &Path) -> Result<Vec<String>, VisualError> {
        let document = self
            .pdfium
            .load_pdf_from_file(pdf_path, None)
            .map_err(|e| VisualError::Pdf(e.to_string()))?;
        let mut names = std::collections::BTreeSet::new();
        for page in document.pages().iter() {
            for object in page.objects().iter() {
                if let Some(text) = object.as_text_object() {
                    let font = text.font();
                    // An unreadable flag counts as embedded: report only what we know.
                    if !font.is_embedded().unwrap_or(true) {
                        names.insert(font.name());
                    }
                }
            }
        }
        Ok(names.into_iter().collect())
    }

    /// The document's Author field, `None` if it has none.
    ///
    /// # Errors
    /// [`VisualError::Pdf`] when the document cannot be loaded.
    pub fn pdf_author(self, pdf_path: &Path) -> Result<Option<String>, VisualError> {
        let document = self
            .pdfium
            .load_pdf_from_file(pdf_path, None)
            .map_err(|e| VisualError::Pdf(e.to_string()))?;
        Ok(document
            .metadata()
            .get(PdfDocumentMetadataTagType::Author)
            .map(|tag| tag.value().to_owned())
            .filter(|v| !v.trim().is_empty()))
    }

    /// How the body text sits on the pages, measured from the PDF itself.
    ///
    /// This is what a page-limit check can trust: it does not matter whether
    /// the text got smaller through `\small`, a macro in another file or a
    /// class option, the PDF shows it. `None` when the document has no text.
    ///
    /// # Errors
    /// [`VisualError::Pdf`] when the document cannot be loaded.
    pub fn body_metrics(self, pdf_path: &Path) -> Result<Option<BodyMetrics>, VisualError> {
        let document = self
            .pdfium
            .load_pdf_from_file(pdf_path, None)
            .map_err(|e| VisualError::Pdf(e.to_string()))?;
        let total = document.pages().len();
        let mut sizes: Vec<(f32, usize)> = Vec::new();
        let mut pitches: Vec<f32> = Vec::new();
        let mut pages: Vec<PageText> = Vec::new();
        let (mut images, mut chars) = (0, 0);

        for (index, page) in document.pages().iter().enumerate() {
            let last = i32::try_from(index).is_ok_and(|i| i + 1 == total);
            let mut seen = PageText::default();
            for object in page.objects().iter() {
                if object.as_image_object().is_some() {
                    images += 1;
                }
                let Some(text) = object.as_text_object() else {
                    continue;
                };
                let n = text.text().chars().filter(|c| !c.is_whitespace()).count();
                if n == 0 {
                    continue;
                }
                chars += n;
                let Ok(quad) = object.bounds() else { continue };
                let rect = quad.to_rect();
                sizes.push((text.scaled_font_size().value, n));
                seen.add(&rect);
            }
            // The last page is partly empty by nature; it says nothing about
            // how full a page is set.
            if !last && seen.baselines.len() >= MIN_BODY_LINES {
                pitches.extend(seen.pitches());
                pages.push(seen);
            }
        }
        if chars == 0 {
            return Ok(None);
        }
        let widths: Vec<f32> = pages.iter().map(PageText::width).collect();
        let heights: Vec<f32> = pages.iter().map(PageText::height).collect();
        Ok(Some(BodyMetrics {
            font_size_pt: weighted_median(&mut sizes),
            line_pitch_pt: median(&mut pitches),
            text_width_pt: median(&mut widths.clone()),
            text_height_pt: median(&mut heights.clone()),
            images,
            chars,
            body_pages: pages.len(),
        }))
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

/// Fewest lines a page needs to count as a page of body text.
const MIN_BODY_LINES: usize = 15;

/// What the body text of a document looks like on its pages.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BodyMetrics {
    /// The font size most text is set in, in points.
    pub font_size_pt: f32,
    /// Distance between the baselines of neighbouring lines, in points.
    pub line_pitch_pt: f32,
    /// Width and height of the text block on a full page, in points.
    pub text_width_pt: f32,
    pub text_height_pt: f32,
    /// Images placed on all pages together.
    pub images: usize,
    /// Characters of text, spaces excluded.
    pub chars: usize,
    /// Pages the width, height and pitch were measured over; 0 means the
    /// document is too short to say.
    pub body_pages: usize,
}

#[derive(Default)]
struct PageText {
    left: f32,
    right: f32,
    top: f32,
    bottom: f32,
    started: bool,
    baselines: std::collections::BTreeSet<i32>,
}

impl PageText {
    fn add(&mut self, rect: &PdfRect) {
        let (l, r) = (rect.left().value, rect.right().value);
        let (t, b) = (rect.top().value, rect.bottom().value);
        if self.started {
            self.left = self.left.min(l);
            self.right = self.right.max(r);
            self.top = self.top.max(t);
            self.bottom = self.bottom.min(b);
        } else {
            (self.left, self.right, self.top, self.bottom, self.started) = (l, r, t, b, true);
        }
        // Half-point steps: words on one line differ by a hair.
        #[allow(clippy::cast_possible_truncation)]
        self.baselines.insert((b * 2.0).round() as i32);
    }

    fn width(&self) -> f32 {
        self.right - self.left
    }

    fn height(&self) -> f32 {
        self.top - self.bottom
    }

    /// Gaps between neighbouring baselines that look like line spacing (not
    /// the larger gaps around headings and displays).
    fn pitches(&self) -> impl Iterator<Item = f32> + '_ {
        let lines: Vec<i32> = self.baselines.iter().copied().collect();
        lines
            .windows(2)
            .filter_map(|w| {
                #[allow(clippy::cast_precision_loss)]
                let gap = (w[1] - w[0]) as f32 / 2.0;
                (6.0..=30.0).contains(&gap).then_some(gap)
            })
            .collect::<Vec<_>>()
            .into_iter()
    }
}

fn median(values: &mut [f32]) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(f32::total_cmp);
    values[values.len() / 2]
}

/// The value that half of all the characters are at or below.
fn weighted_median(values: &mut [(f32, usize)]) -> f32 {
    values.sort_by(|a, b| a.0.total_cmp(&b.0));
    let total: usize = values.iter().map(|v| v.1).sum();
    let mut seen = 0;
    for (value, weight) in values.iter() {
        seen += weight;
        if seen * 2 >= total {
            return *value;
        }
    }
    0.0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// pdfium is not thread-safe; tests that use it take turns.
    static PDFIUM: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn one_at_a_time() -> std::sync::MutexGuard<'static, ()> {
        PDFIUM
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

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

    /// A hand-made one-page PDF that sets text in Helvetica without embedding it.
    const HELVETICA_PDF: &str = "%PDF-1.4\n1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 200 200]/Contents 4 0 R/Resources<</Font<</F1 5 0 R>>>>>>endobj\n4 0 obj<</Length 44>>stream\nBT /F1 18 Tf 20 100 Td (Hello) Tj ET\nendstream\nendobj\n5 0 obj<</Type/Font/Subtype/Type1/BaseFont/Helvetica>>endobj\ntrailer<</Root 1 0 R/Size 6>>\n%%EOF\n";

    #[test]
    fn a_font_that_is_not_embedded_is_reported_by_name() {
        let _turn = one_at_a_time();
        let dir = std::env::temp_dir().join(format!("visual-fonts-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("helvetica.pdf");
        std::fs::write(&path, HELVETICA_PDF).unwrap();
        let fonts = renderer().unwrap().unembedded_fonts(&path).unwrap();
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(fonts, vec!["Helvetica".to_owned()]);
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

    /// A PDF of `pages` pages, each with `lines` lines of text at `size` pt,
    /// `pitch` pt apart, starting `left` pt from the edge.
    fn text_pdf(pages: u32, lines: u32, size: u32, pitch: u32, left: u32) -> String {
        use std::fmt::Write as _;
        let kids: Vec<String> = (0..pages).map(|i| format!("{} 0 R", 4 + i * 2)).collect();
        let mut objects = vec![
            "<</Type/Catalog/Pages 2 0 R>>".to_owned(),
            format!("<</Type/Pages/Kids[{}]/Count {pages}>>", kids.join(" ")),
            "<</Type/Font/Subtype/Type1/BaseFont/Helvetica>>".to_owned(),
        ];
        for i in 0..pages {
            let mut content = String::new();
            for n in 0..lines {
                let y = 700 - n * pitch;
                let _ = writeln!(
                    content,
                    "BT /F1 {size} Tf {left} {y} Td (line {n} of words) Tj ET"
                );
            }
            objects.push(format!(
                "<</Type/Page/Parent 2 0 R/MediaBox[0 0 612 792]/Contents {} 0 R/Resources<</Font<</F1 3 0 R>>>>>>",
                5 + i * 2
            ));
            objects.push(format!(
                "<</Length {}>>stream\n{content}endstream",
                content.len()
            ));
        }
        let mut out = String::from("%PDF-1.4\n");
        for (i, o) in objects.iter().enumerate() {
            let _ = writeln!(out, "{} 0 obj\n{o}\nendobj", i + 1);
        }
        out.push_str("trailer<</Root 1 0 R/Size 99>>\n%%EOF\n");
        out
    }

    fn measured(name: &str, pdf: &str) -> BodyMetrics {
        let _turn = one_at_a_time();
        let dir =
            std::env::temp_dir().join(format!("visual-metrics-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.pdf");
        std::fs::write(&path, pdf).unwrap();
        let m = renderer()
            .unwrap()
            .body_metrics(&path)
            .expect("loads")
            .expect("has text");
        std::fs::remove_dir_all(&dir).ok();
        m
    }

    #[test]
    fn the_body_text_is_measured_from_the_pdf() {
        let m = measured("base", &text_pdf(3, 30, 12, 14, 72));
        assert!((m.font_size_pt - 12.0).abs() < 0.01, "{m:?}");
        assert!((m.line_pitch_pt - 14.0).abs() < 0.6, "{m:?}");
        assert_eq!(m.body_pages, 2, "the last page is not a body page");
        assert!(m.text_height_pt > 400.0 && m.text_width_pt > 40.0, "{m:?}");
        assert_eq!(m.images, 0);
    }

    #[test]
    fn smaller_type_and_tighter_lines_show_in_the_numbers_whatever_caused_them() {
        let base = measured("a", &text_pdf(3, 30, 12, 14, 72));
        let small = measured("b", &text_pdf(3, 30, 10, 14, 72));
        let tight = measured("c", &text_pdf(3, 30, 12, 12, 72));
        assert!(small.font_size_pt < base.font_size_pt * 0.9);
        assert!(tight.line_pitch_pt < base.line_pitch_pt * 0.9);
        assert!(tight.text_height_pt < base.text_height_pt);
    }
}
