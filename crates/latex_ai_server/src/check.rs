//! A project checked without the daemon: compile it, read what the build says
//! about it, and answer questions about the PDF. The `check` command and the
//! MCP server are both thin shells over [`Workbench`]; nothing here writes to
//! the project.

use anyhow::Context;
use latex_ai_compiler::builds::Build;
use latex_ai_compiler::diagnostics::{self, Diagnostic, Severity};
use latex_ai_compiler::{CompileJob, Compiler, CompilerConfig, project};
use layout::{LayoutReport, Options, Resolver};
use serde::Serialize;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use synctex::{SyncTexData, resolve_input_name};
use tokio_util::sync::CancellationToken;

/// Diagnostics listed in a report; the counts still cover all of them.
const MAX_DIAGNOSTICS: usize = 50;

/// A project on disk and a private build store to compile it into.
pub struct Workbench {
    root: PathBuf,
    main: String,
    bundle: Option<String>,
    timeout: Duration,
    compiler: Compiler,
    work: PathBuf,
    /// The map of the last build asked about; one slot is enough, since every
    /// question follows a compile of the same project.
    map: Mutex<Option<(PathBuf, Arc<SyncTexData>)>>,
}

impl Drop for Workbench {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.work);
    }
}

/// Where the first line past the page budget is, or any other spot in the PDF.
#[derive(Debug, Clone, Serialize)]
pub struct PdfRegion {
    pub page: u32,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub line: u32,
}

/// A spot on the PDF traced back to the source.
#[derive(Debug, Clone, Serialize)]
pub struct SourceSpot {
    pub file: String,
    pub line: u32,
    pub column: Option<u32>,
    /// The point fell inside the matched box rather than merely near it.
    pub exact: bool,
}

#[derive(Debug, Serialize)]
pub struct CheckReport {
    pub main: String,
    pub success: bool,
    pub errors: usize,
    pub warnings: usize,
    pub diagnostics: Vec<Diagnostic>,
    pub layout: Option<LayoutReport>,
    /// `None` when the PDF could not be inspected (no pdfium).
    pub fonts_not_embedded: Option<Vec<String>>,
    /// Why the check did not pass; empty when it did.
    pub problems: Vec<String>,
    pub passed: bool,
}

impl Workbench {
    /// Open `target`: a `.tex` file (its folder is the project), or a folder
    /// (the main file is found, or is `main`).
    ///
    /// # Errors
    /// The path is missing, or no main file can be chosen.
    pub fn open(target: &Path, main: Option<&str>, bundle: Option<String>) -> anyhow::Result<Self> {
        let target = std::fs::canonicalize(target)
            .with_context(|| format!("reading {}", target.display()))?;
        let (root, main) = if target.is_file() {
            anyhow::ensure!(
                main.is_none(),
                "--main only applies when checking a folder; {} is a file",
                target.display()
            );
            let root = target
                .parent()
                .context("the file has no folder")?
                .to_path_buf();
            let name = target
                .file_name()
                .context("the path has no file name")?
                .to_string_lossy()
                .into_owned();
            (root, name)
        } else {
            let found = project::detect_main(&target, "", main)
                .map_err(|e| anyhow::anyhow!("choosing the main file: {e}"))?;
            (target, found)
        };

        let work = std::env::temp_dir().join(format!(
            "latex-ai-check-{}",
            latex_ai_core::fsutil::unique_id()
        ));
        let compiler = Compiler::new(CompilerConfig::new(
            std::env::current_exe().context("finding this program")?,
            work.clone(),
        ));
        Ok(Self {
            root,
            main,
            bundle,
            timeout: Duration::from_secs(120),
            compiler,
            work,
            map: Mutex::new(None),
        })
    }

    #[must_use]
    pub fn main(&self) -> &str {
        &self.main
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Compile the project as it is on disk now. An unchanged project is a
    /// cache hit, so asking again is cheap.
    pub async fn compile(&self) -> anyhow::Result<Build> {
        self.compiler
            .compile(
                &CompileJob {
                    root: self.root.clone(),
                    main: self.main.clone(),
                    synctex: true,
                    bundle: self.bundle.clone(),
                    timeout: self.timeout,
                },
                &CancellationToken::new(),
            )
            .await
            .context("compiling")
    }

    async fn map_of(&self, build: &Build) -> anyhow::Result<Arc<SyncTexData>> {
        let path = build.synctex_path();
        anyhow::ensure!(
            build.manifest.has_synctex && path.exists(),
            "this build has no SyncTeX map"
        );
        let hit = self.map.lock().ok().and_then(|slot| {
            slot.as_ref()
                .filter(|(p, _)| *p == path)
                .map(|(_, data)| Arc::clone(data))
        });
        if let Some(data) = hit {
            return Ok(data);
        }
        let parse_path = path.clone();
        let data = tokio::task::spawn_blocking(move || SyncTexData::parse(&parse_path))
            .await
            .context("reading the SyncTeX map")?
            .context("reading the SyncTeX map")?;
        let data = Arc::new(data);
        if let Ok(mut slot) = self.map.lock() {
            *slot = Some((path, Arc::clone(&data)));
        }
        Ok(data)
    }

    /// Compile and measure: the whole `check` verdict.
    ///
    /// `strict` also fails the check on overfull boxes and unembedded fonts.
    pub async fn check(&self, pages: Option<u32>, strict: bool) -> anyhow::Result<CheckReport> {
        let build = self.compile().await?;
        let all = &build.manifest.diagnostics;
        let errors = all.iter().filter(|d| d.severity == Severity::Error).count();
        let warnings = all.len() - errors;
        let success = build.manifest.success && build.manifest.has_pdf;

        let (layout, fonts_not_embedded) = if success {
            let (report, fonts) = self.layout(&build, pages).await?;
            (Some(report), fonts)
        } else {
            (None, None)
        };

        let mut problems = Vec::new();
        if !success {
            problems.push(format!(
                "the document does not compile: {}",
                diagnostics::summarize(all).unwrap_or_else(|| "no details".to_owned())
            ));
        }
        if let Some(budget) = layout.as_ref().and_then(|l| l.budget.as_ref())
            && budget.over
        {
            let mut line = format!(
                "{} pages, over the limit of {} by {} line{} of text",
                budget.pages,
                budget.limit,
                budget.lines_over,
                if budget.lines_over == 1 { "" } else { "s" }
            );
            if let Some(at) = &budget.first_over {
                let _ = write!(line, "; the first one past it is {}:{}", at.file, at.line);
            }
            problems.push(line);
        }
        if strict {
            if let Some(report) = &layout {
                let overfull = report.boxes.iter().filter(|b| b.overfull).count();
                if overfull > 0 {
                    problems.push(format!("{overfull} overfull box(es)"));
                }
            }
            if let Some(fonts) = fonts_not_embedded.as_ref().filter(|f| !f.is_empty()) {
                problems.push(format!("fonts not embedded: {}", fonts.join(", ")));
            }
        }

        Ok(CheckReport {
            main: self.main.clone(),
            success,
            errors,
            warnings,
            diagnostics: all.iter().take(MAX_DIAGNOSTICS).cloned().collect(),
            layout,
            fonts_not_embedded,
            passed: problems.is_empty(),
            problems,
        })
    }

    /// Layout facts for `build`, and the fonts its PDF leaves unembedded
    /// (`None` when it could not be inspected).
    pub async fn layout(
        &self,
        build: &Build,
        pages: Option<u32>,
    ) -> anyhow::Result<(LayoutReport, Option<Vec<String>>)> {
        let data = self.map_of(build).await?;
        let (root, main) = (self.root.clone(), self.main.clone());
        let found = build.manifest.diagnostics.clone();
        let report =
            tokio::task::spawn_blocking(move || analyse_layout(&data, &found, &root, &main, pages))
                .await
                .context("measuring the layout")?;
        let pdf = build.pdf_path();
        let fonts = tokio::task::spawn_blocking(move || {
            visual::render::renderer()
                .ok()
                .and_then(|r| r.unembedded_fonts(&pdf).ok())
        })
        .await
        .context("inspecting the fonts")?;
        Ok((report, fonts))
    }

    /// Where `file:line` landed in the PDF, best match first.
    pub async fn source_to_pdf(
        &self,
        build: &Build,
        file: &str,
        line: u32,
    ) -> anyhow::Result<Vec<PdfRegion>> {
        anyhow::ensure!(line > 0, "line numbers are 1-based");
        let data = self.map_of(build).await?;
        let (root, main) = (self.root.clone(), PathBuf::from(&self.main));
        let rel = PathBuf::from(file);
        let hits = tokio::task::spawn_blocking(move || {
            data.tag_for_file(&rel, &root, &main)
                .map(|tag| data.forward_search(tag, line))
        })
        .await
        .context("searching the SyncTeX map")?
        .with_context(|| format!("{file} is not part of that PDF"))?;
        Ok(hits
            .into_iter()
            .take(16)
            .map(|h| PdfRegion {
                page: h.page,
                x: h.x,
                y: h.y,
                width: h.width,
                height: h.height,
                line: h.line,
            })
            .collect())
    }

    /// The source behind a point on the PDF (points from the page's top-left).
    pub async fn pdf_to_source(
        &self,
        build: &Build,
        page: u32,
        x: f64,
        y: f64,
    ) -> anyhow::Result<Option<SourceSpot>> {
        let sane = |v: f64| v.is_finite() && (0.0..=100_000.0).contains(&v);
        anyhow::ensure!(
            page > 0 && sane(x) && sane(y),
            "page is 1-based and the point must lie on the page"
        );
        let data = self.map_of(build).await?;
        let root = self.root.clone();
        let main = self.main.clone();
        let hit = tokio::task::spawn_blocking(move || data.backward_search(page, x, y))
            .await
            .context("searching the SyncTeX map")?;
        Ok(hit.map(|h| SourceSpot {
            file: resolve_input_name(&h.file, &root).map_or(main, |p| slashes(&p)),
            line: h.line,
            column: h.column,
            exact: h.exact,
        }))
    }

    /// One page of the PDF as PNG bytes.
    pub async fn render_png(&self, build: &Build, page: u32, dpi: u16) -> anyhow::Result<Vec<u8>> {
        anyhow::ensure!(page > 0, "pages are numbered from 1");
        let pdf = build.pdf_path();
        tokio::task::spawn_blocking(move || {
            let image = visual::render::renderer()
                .context("PDF rendering is unavailable (run `latex-ai doctor`)")?
                .render_page(&pdf, page - 1, dpi)
                .map_err(|e| anyhow::anyhow!("rendering page {page}: {e}"))?;
            let mut png = Vec::new();
            image
                .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
                .context("encoding the page")?;
            Ok(png)
        })
        .await
        .context("rendering the page")?
    }
}

/// The layout of a finished build; shared with `GET /api/layout`.
pub(crate) fn analyse_layout(
    data: &SyncTexData,
    diagnostics: &[Diagnostic],
    root: &Path,
    main: &str,
    page_limit: Option<u32>,
) -> LayoutReport {
    let file_of = |tag: u32| {
        data.inputs
            .get(&tag)
            .and_then(|name| resolve_input_name(name, root))
            .map_or_else(|| main.to_owned(), |p| slashes(&p))
    };
    let tag_of = |file: &str| data.tag_for_file(Path::new(file), root, Path::new(main));
    layout::analyze(
        data,
        diagnostics,
        &Resolver {
            file_of: &file_of,
            tag_of: &tag_of,
        },
        &Options { page_limit },
    )
}

pub(crate) fn slashes(path: &Path) -> String {
    path.components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

impl CheckReport {
    /// The report as Markdown: readable in a terminal, and rendered as is in a
    /// GitHub job summary.
    #[must_use]
    pub fn to_markdown(&self) -> String {
        let mut out = String::new();
        let verdict = if self.passed { "passed" } else { "failed" };
        let _ = writeln!(out, "## Renkeu check: {} {verdict}\n", self.main);

        for problem in &self.problems {
            let _ = writeln!(out, "- **{problem}**");
        }
        if !self.problems.is_empty() {
            out.push('\n');
        }

        let _ = writeln!(
            out,
            "- Compile: {} ({} error{}, {} warning{})",
            if self.success { "ok" } else { "failed" },
            self.errors,
            plural(self.errors),
            self.warnings,
            plural(self.warnings)
        );
        if let Some(layout) = &self.layout {
            match &layout.budget {
                Some(b) => {
                    let _ = writeln!(out, "- Pages: {} of {} allowed", b.pages, b.limit);
                }
                None => {
                    let _ = writeln!(out, "- Pages: {}", layout.pages);
                }
            }
            if let Some(ratio) = layout.fill.ratio {
                let _ = writeln!(
                    out,
                    "- Last page: {:.0}% as full as the fullest",
                    ratio * 100.0
                );
            }
            let overfull = layout.boxes.iter().filter(|b| b.overfull).count();
            let _ = writeln!(
                out,
                "- Boxes the log complains about: {} ({overfull} overfull listed)",
                layout.boxes_total
            );
            let _ = writeln!(
                out,
                "- Paragraphs ending in a very short line: {}",
                layout.short_last_lines_total
            );
        }
        match &self.fonts_not_embedded {
            Some(f) if f.is_empty() => {
                let _ = writeln!(out, "- Fonts: all embedded");
            }
            Some(f) => {
                let _ = writeln!(out, "- Fonts not embedded: {}", f.join(", "));
            }
            None if self.layout.is_some() => {
                let _ = writeln!(out, "- Fonts: not checked (libpdfium was not found)");
            }
            None => {}
        }

        let errors: Vec<&Diagnostic> = self
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        if !errors.is_empty() {
            out.push_str("\n### Errors\n\n");
            for d in errors.iter().take(20) {
                let _ = writeln!(out, "- `{}`", d.headline());
            }
        }
        if let Some(layout) = &self.layout {
            let spots: Vec<_> = layout
                .boxes
                .iter()
                .filter(|b| b.overfull)
                .filter_map(|b| b.place.as_ref().map(|p| (b, p)))
                .take(10)
                .collect();
            if !spots.is_empty() {
                out.push_str("\n### Overfull boxes\n\n");
                for (b, p) in spots {
                    let by = b
                        .amount_pt
                        .map_or(String::new(), |a| format!(" by {a:.1}pt"));
                    let _ = writeln!(out, "- `{}:{}` (page {}){by}", p.file, p.line, p.page);
                }
            }
            if !layout.short_last_lines.is_empty() {
                out.push_str("\n### Short last lines\n\n");
                for p in layout.short_last_lines.iter().take(10) {
                    let _ = writeln!(out, "- `{}:{}` (page {})", p.file, p.line, p.page);
                }
            }
        }
        out
    }
}

/// Escape a value for a GitHub workflow command (`::error ...::message`).
fn command_escape(text: &str, property: bool) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '%' => out.push_str("%25"),
            '\r' => out.push_str("%0D"),
            '\n' => out.push_str("%0A"),
            ':' if property => out.push_str("%3A"),
            ',' if property => out.push_str("%2C"),
            c => out.push(c),
        }
    }
    out
}

fn annotation(level: &str, place: Option<(&str, u32)>, message: &str) -> String {
    let location = place.map_or_else(String::new, |(file, line)| {
        format!(" file={},line={line}", command_escape(file, true))
    });
    format!("::{level}{location}::{}", command_escape(message, false))
}

impl CheckReport {
    /// GitHub workflow commands that put the findings on the files and lines
    /// they are about: errors, the first line past the page limit, and the
    /// overfull boxes.
    #[must_use]
    pub fn github_annotations(&self) -> Vec<String> {
        let mut out = Vec::new();
        for d in self
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
        {
            let place = d.file.as_deref().zip(d.line);
            out.push(annotation("error", place, &d.message));
        }
        let over = self
            .layout
            .as_ref()
            .and_then(|l| l.budget.as_ref())
            .filter(|b| b.over);
        if let Some(budget) = over {
            let place = budget
                .first_over
                .as_ref()
                .map(|p| (p.file.as_str(), p.line));
            out.push(annotation(
                "error",
                place,
                &format!(
                    "{} pages, over the limit of {}: this is the first line past it",
                    budget.pages, budget.limit
                ),
            ));
        }
        // Problems with no line of their own, such as a failed run that left
        // no parsable error; otherwise the lines above already say it.
        if out.is_empty() {
            for problem in &self.problems {
                out.push(annotation("error", None, problem));
            }
        }
        if let Some(layout) = &self.layout {
            for b in layout.boxes.iter().filter(|b| b.overfull).take(10) {
                let place = b.place.as_ref().map(|p| (p.file.as_str(), p.line));
                out.push(annotation("warning", place, &b.message));
            }
        }
        out
    }
}

const fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn annotation_text_cannot_break_out_of_the_command() {
        assert_eq!(
            annotation("error", Some(("a,b:c.tex", 7)), "100% bad\nsecond"),
            "::error file=a%2Cb%3Ac.tex,line=7::100%25 bad%0Asecond"
        );
        assert_eq!(annotation("warning", None, "plain"), "::warning::plain");
    }
}
