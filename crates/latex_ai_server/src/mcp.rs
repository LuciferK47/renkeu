//! `latex-ai mcp`: the checker as a Model Context Protocol server on stdio, so
//! an agent (Claude Code, Codex, Cursor) can compile a paper, read the log and
//! look at the PDF through the same engine the editor uses.
//!
//! The server only reads. It compiles the folder it was started in and never
//! writes to it; an agent changes the source with its own file tools and asks
//! again. Every tool compiles first (an unchanged project is a cache hit), so
//! an answer is never about an older version of the files. Stdout carries the
//! protocol, so nothing else may print there.

use crate::check::Workbench;
use latex_ai_compiler::diagnostics::{Diagnostic, Severity};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock};
use rmcp::transport::stdio;
use rmcp::{ErrorData, ServerHandler, ServiceExt, schemars, tool, tool_handler, tool_router};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

const DEFAULT_DPI: u16 = 100;
const DPI_RANGE: std::ops::RangeInclusive<u16> = 30..=200;

#[derive(Clone)]
struct Checker {
    bench: Arc<Workbench>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
struct LayoutArgs {
    /// The page limit to check against, such as 8 for a venue that allows 8 pages.
    pages: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
struct DiagnosticsArgs {
    /// Only this severity: "error" or "warning". Both when omitted.
    severity: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
struct SourceArgs {
    /// Project-relative file, such as "main.tex" or "sections/intro.tex".
    file: String,
    /// 1-based line number.
    line: u32,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
struct PointArgs {
    /// 1-based page number.
    page: u32,
    /// Points from the page's left edge (72 points = 1 inch).
    x: f64,
    /// Points from the page's top edge.
    y: f64,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
struct PageArgs {
    /// 1-based page number.
    page: u32,
    /// Resolution, 30 to 200. 100 is readable and small.
    dpi: Option<u16>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
struct PreflightArgs {
    /// A profile id such as "arxiv", "double-blind" or "accessibility", or a
    /// path to a profile .toml. Call without it to list the ids.
    profile: Option<String>,
}

#[derive(Serialize)]
struct CompileSummary {
    main: String,
    success: bool,
    pages: Option<u32>,
    errors: usize,
    warnings: usize,
    /// The first error, as `file:line: message`.
    first_error: Option<String>,
}

fn reply<T: Serialize>(value: &T) -> Result<CallToolResult, ErrorData> {
    serde_json::to_value(value)
        .map(CallToolResult::structured)
        .map_err(|e| ErrorData::internal_error(e.to_string(), None))
}

/// A failure the agent can read and react to, not a protocol error.
fn fail(error: &anyhow::Error) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(format!("{error:#}"))])
}

#[tool_router]
impl Checker {
    #[tool(
        description = "Compile the project and say whether it worked: page count, error and warning counts, and the first error. Use this after every edit."
    )]
    async fn compile(&self) -> Result<CallToolResult, ErrorData> {
        match self.bench.check(None, false).await {
            Ok(report) => reply(&CompileSummary {
                main: report.main.clone(),
                success: report.success,
                pages: report.layout.as_ref().map(|l| l.pages),
                errors: report.errors,
                warnings: report.warnings,
                first_error: report
                    .diagnostics
                    .iter()
                    .find(|d| d.severity == Severity::Error)
                    .map(Diagnostic::headline),
            }),
            Err(e) => Ok(fail(&e)),
        }
    }

    #[tool(
        description = "Compile the project and list its errors and warnings, each with the file and line TeX reported."
    )]
    async fn diagnostics(
        &self,
        Parameters(args): Parameters<DiagnosticsArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let wanted = match args.severity.as_deref() {
            None => None,
            Some("error") => Some(Severity::Error),
            Some("warning") => Some(Severity::Warning),
            Some(other) => {
                return Ok(fail(&anyhow::anyhow!(
                    "severity is \"error\" or \"warning\", not \"{other}\""
                )));
            }
        };
        match self.bench.compile().await {
            Ok(build) => {
                let list: Vec<&Diagnostic> = build
                    .manifest
                    .diagnostics
                    .iter()
                    .filter(|d| wanted.is_none_or(|w| d.severity == w))
                    .take(100)
                    .collect();
                reply(&list)
            }
            Err(e) => Ok(fail(&e)),
        }
    }

    #[tool(
        description = "Measure the compiled PDF: page count, how full the last page is, where a page limit is crossed (file and line of the first line past it), overfull boxes, paragraphs ending in a very short line (the cheapest place to win back space), and fonts that are not embedded."
    )]
    async fn layout_report(
        &self,
        Parameters(args): Parameters<LayoutArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        if args.pages == Some(0) {
            return Ok(fail(&anyhow::anyhow!("a page limit is at least 1")));
        }
        match self.bench.check(args.pages, false).await {
            Ok(report) if !report.success => Ok(fail(&anyhow::anyhow!(
                "the document does not compile, so there is no PDF to measure: {}",
                report.problems.join("; ")
            ))),
            Ok(report) => reply(&serde_json::json!({
                "layout": report.layout,
                "fonts_not_embedded": report.fonts_not_embedded,
            })),
            Err(e) => Ok(fail(&e)),
        }
    }

    #[tool(
        description = "Check the project against a submission profile (arXiv packaging, double-blind anonymity, accessibility basics, or a profile file): file names, bibliography, unused files, author names, alt text, fonts, page limit. Returns findings with file and line. Without a profile, lists the available ones."
    )]
    async fn preflight(
        &self,
        Parameters(args): Parameters<PreflightArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let home = latex_ai_core::config::default_home();
        let Some(spec) = args.profile else {
            let list: Vec<_> = crate::profiles::all(Some(&home))
                .into_iter()
                .map(|p| serde_json::json!({"id": p.id, "name": p.name, "description": p.description, "source": p.source}))
                .collect();
            return reply(&list);
        };
        let profile = match crate::profiles::find(&spec, Some(&home)) {
            Ok(p) => p,
            Err(message) => return Ok(fail(&anyhow::anyhow!(message))),
        };
        let found = async {
            let build = self.bench.compile().await?;
            self.bench.preflight(&profile, &build).await
        }
        .await;
        match found {
            Ok(report) => reply(&report),
            Err(e) => Ok(fail(&e)),
        }
    }

    #[tool(
        description = "Where a source line landed in the PDF: page and rectangle in points from the page's top-left corner. Empty when the line produced no output (a comment, say)."
    )]
    async fn locate_in_pdf(
        &self,
        Parameters(args): Parameters<SourceArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let found = async {
            let build = self.bench.compile().await?;
            self.bench
                .source_to_pdf(&build, &args.file, args.line)
                .await
        }
        .await;
        match found {
            Ok(regions) => reply(&regions),
            Err(e) => Ok(fail(&e)),
        }
    }

    #[tool(
        description = "Which source file and line produced a point on a PDF page. Coordinates are points from the page's top-left corner."
    )]
    async fn locate_in_source(
        &self,
        Parameters(args): Parameters<PointArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let found = async {
            let build = self.bench.compile().await?;
            self.bench
                .pdf_to_source(&build, args.page, args.x, args.y)
                .await
        }
        .await;
        match found {
            Ok(Some(spot)) => reply(&spot),
            Ok(None) => Ok(fail(&anyhow::anyhow!(
                "nothing on that page maps to the source"
            ))),
            Err(e) => Ok(fail(&e)),
        }
    }

    #[tool(
        description = "Render one page of the compiled PDF as an image, to look at how it actually came out."
    )]
    async fn render_page(
        &self,
        Parameters(args): Parameters<PageArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let dpi = args
            .dpi
            .unwrap_or(DEFAULT_DPI)
            .clamp(*DPI_RANGE.start(), *DPI_RANGE.end());
        let drawn = async {
            let build = self.bench.compile().await?;
            anyhow::ensure!(
                build.manifest.success && build.manifest.has_pdf,
                "the document does not compile, so there is no PDF to show"
            );
            self.bench.render_png(&build, args.page, dpi).await
        }
        .await;
        match drawn {
            Ok(png) => {
                use base64::Engine as _;
                Ok(CallToolResult::success(vec![
                    ContentBlock::text(format!("Page {} at {dpi} dpi", args.page)),
                    ContentBlock::image(
                        base64::engine::general_purpose::STANDARD.encode(png),
                        "image/png",
                    ),
                ]))
            }
            Err(e) => Ok(fail(&e)),
        }
    }
}

// The macro writes the trait's async methods; some have nothing to await.
#[allow(clippy::unused_async_trait_impl)]
#[tool_handler(
    name = "latex-ai",
    instructions = "Compiles a LaTeX project on this machine and reports what the compiled PDF looks like. Edit the .tex files with your own tools, then call compile; use diagnostics for the errors, layout_report for page count and overfull boxes, locate_in_pdf and locate_in_source to move between a source line and the PDF, and render_page to see a page. This server never changes the project's files."
)]
impl ServerHandler for Checker {}

/// Serve `bench` over stdio until the client disconnects.
pub async fn serve(bench: Workbench) -> anyhow::Result<()> {
    use anyhow::Context as _;
    let running = Checker {
        bench: Arc::new(bench),
    }
    .serve(stdio())
    .await
    .context("starting the MCP server")?;
    running.waiting().await.context("running the MCP server")?;
    Ok(())
}
