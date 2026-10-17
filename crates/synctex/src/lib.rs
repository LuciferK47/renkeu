//! `SyncTeX` source-map reader: parses `.synctex(.gz)` and maps between PDF
//! coordinates and source positions. Format parsing only — this crate has zero
//! knowledge of `LaTeX` semantics (masterplan.md §5 rule 7).
//!
//! # Coordinates
//!
//! `SyncTeX` stores lengths in TeX **scaled points** (sp), with the page origin
//! at the **top-left** and `y` growing downward. A node's reference point is
//! its baseline-left corner, so its box spans `y - height ..= y + depth`
//! vertically and `x ..= x + width` horizontally.
//!
//! Callers work in PDF **points** (bp, 1/72 inch) — the same unit `pdfium`
//! renders from — and [`SyncTexData`] converts. The factor is
//! `65536 sp/pt × 72.27 pt/inch ÷ 72 bp/inch`; a document's 1-inch margin
//! shows up as `4736287 sp`, which is exactly 72 bp, confirming it.
#![forbid(unsafe_code)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

pub mod parser;
pub mod search;

pub use search::{PdfHit, SourceHit};

/// Scaled points per PDF point (big point). See the module docs.
pub const SP_PER_BP: f64 = 65536.0 * 72.27 / 72.0;

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SyncTexError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("parse error: {0}")]
    Parse(String),
    #[error("file not found in SyncTeX data")]
    FileNotFound,
}

/// What kind of typeset object a [`Node`] came from. Kept because boxes carry
/// real extents while glue/kern/math are effectively points — search weights
/// them differently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum NodeKind {
    VBox,
    HBox,
    VoidVBox,
    VoidHBox,
    Glue,
    Kern,
    Math,
    Current,
}

impl NodeKind {
    /// Boxes bound a region; the rest mark a single position.
    #[must_use]
    pub fn is_box(self) -> bool {
        matches!(
            self,
            Self::VBox | Self::HBox | Self::VoidVBox | Self::VoidHBox
        )
    }
}

/// One typeset object, in scaled points with `unit`/offsets already applied.
#[derive(Debug, Clone)]
pub struct Node {
    /// Input file tag; index into [`SyncTexData::inputs`].
    pub tag: u32,
    /// 1-based source line.
    pub line: u32,
    /// 1-based source column when the engine recorded one (`xetex` does not).
    pub column: Option<u32>,
    pub kind: NodeKind,
    pub x: i64,
    pub y: i64,
    pub width: i64,
    pub height: i64,
    pub depth: i64,
}

impl Node {
    /// Bounding box in sp as `(left, top, right, bottom)`, y growing downward.
    #[must_use]
    pub fn bounds(&self) -> (i64, i64, i64, i64) {
        (
            self.x,
            self.y - self.height,
            self.x + self.width,
            self.y + self.depth,
        )
    }

    /// Area in sp², saturating. Used to prefer the tightest enclosing box.
    #[must_use]
    pub fn area(&self) -> i128 {
        let (left, top, right, bottom) = self.bounds();
        i128::from(right - left).saturating_mul(i128::from(bottom - top))
    }

    /// Whether the point (sp) lies inside this node's box.
    #[must_use]
    pub fn contains(&self, x: i64, y: i64) -> bool {
        let (left, top, right, bottom) = self.bounds();
        x >= left && x <= right && y >= top && y <= bottom
    }

    /// Squared distance in sp² from the point to this node's box (0 inside).
    #[must_use]
    pub fn distance_sq(&self, x: i64, y: i64) -> i128 {
        let (left, top, right, bottom) = self.bounds();
        let dx = if x < left {
            left - x
        } else if x > right {
            x - right
        } else {
            0
        };
        let dy = if y < top {
            top - y
        } else if y > bottom {
            y - bottom
        } else {
            0
        };
        i128::from(dx).saturating_mul(i128::from(dx))
            + i128::from(dy).saturating_mul(i128::from(dy))
    }
}

/// A parsed `SyncTeX` map.
#[derive(Debug, Clone)]
pub struct SyncTexData {
    pub version: u32,
    /// TeX `\mag`; 1000 means unscaled.
    pub magnification: i64,
    pub unit: i64,
    pub x_offset: i64,
    pub y_offset: i64,
    /// Input tag -> recorded name. Tectonic records the primary input as
    /// `texput`, so names are not usable paths for single-file compiles.
    pub inputs: HashMap<u32, String>,
    /// Page number (1-based) -> flattened nodes on that page.
    pub pages: BTreeMap<u32, Vec<Node>>,
}

/// Turn an `Input:` name from a map into a project-relative path.
///
/// When a document is compiled from a file on disk, Tectonic records inputs
/// by their **absolute** path (`/work/proj/chapters/a.tex`); bundle files
/// (`article.cls`, fonts) are recorded with an empty name. Documents
/// compiled from an in-memory buffer record the placeholder `texput`. The
/// answer is `None` for anything that is not a file inside `root`: empty
/// names, the placeholder, paths elsewhere on disk, and anything that tries
/// to climb out with `..`. Callers treat `None` as "the main file".
#[must_use]
pub fn resolve_input_name(name: &str, root: &Path) -> Option<std::path::PathBuf> {
    use std::path::Component;

    let name = name.trim();
    if name.is_empty() || name == "texput" || name == "texput.tex" {
        return None;
    }
    let path = Path::new(name);
    let relative = if path.is_absolute() {
        path.strip_prefix(root).ok()?
    } else {
        path
    };

    let mut out = std::path::PathBuf::new();
    for component in relative.components() {
        match component {
            Component::Normal(p) => out.push(p),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    (!out.as_os_str().is_empty()).then_some(out)
}

impl SyncTexData {
    /// Parse a `.synctex` or `.synctex.gz` file.
    ///
    /// # Errors
    /// [`SyncTexError::Io`] if unreadable or not valid gzip.
    pub fn parse(path: &Path) -> Result<Self, SyncTexError> {
        parser::parse_file(path)
    }

    /// Parse from already-decompressed bytes.
    ///
    /// # Errors
    /// [`SyncTexError::Io`] if the gzip stream is invalid.
    pub fn parse_gz_bytes(bytes: &[u8]) -> Result<Self, SyncTexError> {
        parser::parse_gz_bytes(bytes)
    }

    /// Scaled points -> PDF points.
    #[must_use]
    pub fn sp_to_bp(&self, sp: i64) -> f64 {
        #[allow(clippy::cast_precision_loss)]
        let scaled = sp as f64 * self.mag_factor();
        scaled / SP_PER_BP
    }

    /// PDF points -> scaled points.
    #[must_use]
    pub fn bp_to_sp(&self, bp: f64) -> i64 {
        let sp = bp * SP_PER_BP / self.mag_factor();
        #[allow(clippy::cast_possible_truncation)]
        {
            sp.round() as i64
        }
    }

    /// `\mag / 1000`; 1.0 for virtually every document.
    fn mag_factor(&self) -> f64 {
        if self.magnification <= 0 {
            1.0
        } else {
            #[allow(clippy::cast_precision_loss)]
            {
                self.magnification as f64 / 1000.0
            }
        }
    }

    /// The primary input tag: the lowest tag with a non-empty name. For
    /// single-document compiles every meaningful node carries this tag.
    #[must_use]
    pub fn primary_tag(&self) -> Option<u32> {
        self.inputs
            .iter()
            .filter(|(_, name)| !name.trim().is_empty())
            .map(|(tag, _)| *tag)
            .min()
    }

    /// PDF position -> source position. `x`/`y` are PDF points from the page's
    /// top-left corner. Returns `None` when the page is absent or has no
    /// usable nodes.
    #[must_use]
    pub fn backward_search(&self, page: u32, x_bp: f64, y_bp: f64) -> Option<SourceHit> {
        search::backward_search(self, page, x_bp, y_bp)
    }

    /// Source position -> PDF positions, best match first. `line` is 1-based.
    #[must_use]
    pub fn forward_search(&self, tag: u32, line: u32) -> Vec<PdfHit> {
        search::forward_search(self, tag, line)
    }
}

#[cfg(test)]
mod resolve_tests {
    use super::*;

    #[test]
    fn absolute_names_inside_the_root_become_relative() {
        let root = Path::new("/work/proj");
        assert_eq!(
            resolve_input_name("/work/proj/main.tex", root),
            Some("main.tex".into())
        );
        assert_eq!(
            resolve_input_name("/work/proj/chapters/a.tex", root),
            Some("chapters/a.tex".into())
        );
        assert_eq!(
            resolve_input_name("/work/proj/./chapters/../b.tex", root),
            None,
            "a parent component is refused even if it would stay inside"
        );
    }

    #[test]
    fn relative_names_are_kept_if_they_stay_inside() {
        let root = Path::new("/work/proj");
        assert_eq!(
            resolve_input_name("./chapters/a.tex", root),
            Some("chapters/a.tex".into())
        );
        assert_eq!(
            resolve_input_name("chapters/a.tex", root),
            Some("chapters/a.tex".into())
        );
    }

    #[test]
    fn everything_else_means_the_main_file() {
        let root = Path::new("/work/proj");
        for name in [
            "",
            "  ",
            "texput",
            "texput.tex",
            "/etc/passwd",
            "../up.tex",
            "/work/other/x.tex",
            "/work/proj",
            "/",
        ] {
            assert_eq!(resolve_input_name(name, root), None, "{name:?}");
        }
    }
}
