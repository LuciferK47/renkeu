//! `latex-ai` binary entrypoint. M1 subcommands: `serve`, `compile`.
#![forbid(unsafe_code)]

use std::path::PathBuf;

use anyhow::Context;
use clap::{Parser, Subcommand};
use latex_ai_compiler::{CompileJob, Compiler, CompilerConfig};
use tokio_util::sync::CancellationToken;

#[derive(Parser)]
#[command(name = "latex-ai", version, about = "Autonomous LaTeX document engine")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run the HTTP server.
    Serve {
        /// TCP port to bind.
        #[arg(long, default_value_t = 3000)]
        port: u16,
        /// Address to bind. Defaults to loopback: the daemon has no
        /// authentication (masterplan §11.1), so it must not be exposed
        /// directly. Containers need `0.0.0.0` because Docker forwards a
        /// published port to the container's interface, not its loopback —
        /// isolation there comes from the host-side mapping instead.
        #[arg(long, env = "LATEX_AI_HOST", default_value = "127.0.0.1")]
        host: String,
    },
    /// Compile a LaTeX file to PDF in-process via Tectonic.
    Compile {
        /// Path to the main `.tex` file.
        file: PathBuf,
        /// Output PDF path.
        #[arg(short, long, default_value = "output.pdf")]
        out: PathBuf,
    },
    /// Probe a running server's `/health`, exiting non-zero if it is unwell.
    ///
    /// This exists so the container image needs no HTTP client of its own. The
    /// only thing curl did there was run `HEALTHCHECK`, and it dragged in a
    /// dependency tree far larger than the check was worth.
    Healthcheck {
        /// Port the server is listening on.
        #[arg(long, default_value_t = 3000)]
        port: u16,
    },
    /// Check that PDF rendering can start: bind libpdfium and say where it
    /// came from. Exits non-zero if it cannot.
    ///
    /// The container has no shell to inspect, and a missing library otherwise
    /// shows up only as a 500 on the first page request.
    Doctor,
    /// Compile a project and report on it: whether it builds, how many pages
    /// it has, overfull boxes, short last lines, unembedded fonts. Exits
    /// non-zero when it does not build or goes over `--pages`.
    ///
    /// Prints Markdown (or JSON with `--json`) on stdout, so it can be
    /// appended to a GitHub job summary. Nothing in the project is changed.
    Check {
        /// A `.tex` file, or a folder (its main file is found).
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Main file inside a folder, when it has more than one candidate.
        #[arg(long)]
        main: Option<String>,
        /// Fail when the PDF has more pages than this.
        #[arg(long, value_parser = clap::value_parser!(u32).range(1..=10_000))]
        pages: Option<u32>,
        /// Also fail on overfull boxes and fonts that are not embedded.
        #[arg(long)]
        strict: bool,
        /// Print the report as JSON.
        #[arg(long, conflicts_with = "github")]
        json: bool,
        /// For GitHub Actions: add the report to the job summary
        /// (`$GITHUB_STEP_SUMMARY`) and print workflow annotations that put
        /// the findings on the lines they are about.
        #[arg(long)]
        github: bool,
        /// Also run a preflight profile: an id (`arxiv`, `double-blind`,
        /// `accessibility`, or one in `<home>/profiles`) or the path to a
        /// profile `.toml`. Fails the check on its errors.
        #[arg(long)]
        profile: Option<String>,
        /// TeX bundle to load: a URL or a local path. Tectonic's default when omitted.
        #[arg(long)]
        bundle: Option<String>,
    },
    /// List the preflight profiles `check --profile` accepts.
    Profiles,
    /// Serve the checker to an AI agent over the Model Context Protocol
    /// (stdio). Tools: compile, diagnostics, `layout_report`, `locate_in_pdf`,
    /// `locate_in_source`, `render_page`. Reads the project, never writes to it.
    Mcp {
        /// A `.tex` file, or a folder (its main file is found).
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Main file inside a folder, when it has more than one candidate.
        #[arg(long)]
        main: Option<String>,
        /// TeX bundle to load: a URL or a local path.
        #[arg(long)]
        bundle: Option<String>,
    },
}

fn main() -> anyhow::Result<()> {
    // Compile workers are this same binary. This must run before anything
    // else (no runtime, no logging): a worker is a one-shot child process.
    latex_ai_compiler::worker::run_if_requested();
    run()
}

#[tokio::main]
async fn run() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        // Stdout belongs to the command's output (and, for `mcp`, to the
        // protocol); logs never go there.
        .with_writer(std::io::stderr)
        .init();

    match Cli::parse().command {
        Command::Serve { port, host } => serve(&host, port).await,
        Command::Compile { file, out } => compile(&file, &out).await,
        Command::Healthcheck { port } => healthcheck(port).await,
        Command::Doctor => doctor(),
        Command::Profiles => {
            profiles();
            Ok(())
        }
        Command::Check {
            path,
            main,
            pages,
            strict,
            json,
            github,
            profile,
            bundle,
        } => {
            let how = if json {
                Output::Json
            } else if github {
                Output::Github
            } else {
                Output::Markdown
            };
            check(
                &path,
                main.as_deref(),
                pages,
                strict,
                how,
                profile.as_deref(),
                bundle,
            )
            .await
        }
        Command::Mcp { path, main, bundle } => {
            let bench = latex_ai_server::check::Workbench::open(&path, main.as_deref(), bundle)?;
            tracing::info!(main = bench.main(), "MCP server ready on stdio");
            latex_ai_server::mcp::serve(bench).await
        }
    }
}

// The list is this command's output, not a log line.
#[allow(clippy::print_stdout)]
fn profiles() {
    for p in latex_ai_server::profiles::all(Some(&latex_ai_core::config::default_home())) {
        println!("{:<14} {} ({} rules)", p.id, p.name, p.rules.len());
        if !p.description.is_empty() {
            println!("{:<14} {}", "", p.description);
        }
        if let Some(source) = &p.source {
            println!("{:<14} rules from {source}", "");
        }
    }
}

// The report is this command's output, not a log line.
#[allow(clippy::print_stdout)]
fn doctor() -> anyhow::Result<()> {
    let source =
        latex_ai_server::bootstrap::check_pdfium().context("PDF rendering is unavailable")?;
    println!("pdfium: ok ({source})");
    Ok(())
}

#[derive(Clone, Copy)]
enum Output {
    Markdown,
    Json,
    Github,
}

// The report is this command's output, not a log line.
#[allow(clippy::print_stdout)]
async fn check(
    path: &std::path::Path,
    main: Option<&str>,
    pages: Option<u32>,
    strict: bool,
    output: Output,
    profile: Option<&str>,
    bundle: Option<String>,
) -> anyhow::Result<()> {
    let profile = profile
        .map(|spec| {
            latex_ai_server::profiles::find(spec, Some(&latex_ai_core::config::default_home()))
        })
        .transpose()
        .map_err(anyhow::Error::msg)?;
    let bench = latex_ai_server::check::Workbench::open(path, main, bundle)?;
    let report = bench.check_with(pages, strict, profile.as_ref()).await?;
    match output {
        Output::Json => println!("{}", serde_json::to_string_pretty(&report)?),
        Output::Markdown => print!("{}", report.to_markdown()),
        Output::Github => {
            use std::io::Write as _;
            for line in report.github_annotations() {
                println!("{line}");
            }
            match std::env::var_os("GITHUB_STEP_SUMMARY") {
                Some(file) => {
                    let mut summary = std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(&file)
                        .with_context(|| format!("opening {}", file.to_string_lossy()))?;
                    writeln!(summary, "{}", report.to_markdown())?;
                }
                None => print!("{}", report.to_markdown()),
            }
        }
    }
    // Drop removes the build store; `exit` would skip it.
    drop(bench);
    if !report.passed {
        std::process::exit(1);
    }
    Ok(())
}

async fn serve(host: &str, port: u16) -> anyhow::Result<()> {
    let opts = latex_ai_server::bootstrap::BootOptions::from_process()?;
    let state = latex_ai_server::bootstrap::build_state(opts)
        .await
        .context("starting up")?;
    let app = latex_ai_server::build_router(state).context("building the router")?;
    let addr = format!("{host}:{port}");
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .with_context(|| format!("binding {addr}"))?;
    if host != "127.0.0.1" && host != "localhost" && host != "::1" {
        tracing::warn!(
            %addr,
            "bound a non-loopback address; this daemon has no authentication — \
             ensure something in front of it restricts access"
        );
    }
    tracing::info!(%addr, "latex-ai serving");
    axum::serve(listener, app).await.context("axum serve")?;
    Ok(())
}

/// One HTTP/1.1 request over a raw socket, so no HTTP client crate is pulled in
/// for a job this small. `Connection: close` makes the server hang up after
/// responding, which is what lets the read terminate.
async fn healthcheck(port: u16) -> anyhow::Result<()> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

    // Always loopback: this runs inside the container, probing its own server.
    let connect = tokio::net::TcpStream::connect(("127.0.0.1", port));
    let mut stream = tokio::time::timeout(TIMEOUT, connect)
        .await
        .context("timed out connecting to the server")?
        .context("connecting to the server")?;

    stream
        .write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .context("sending the health request")?;

    let mut response = Vec::new();
    tokio::time::timeout(TIMEOUT, stream.read_to_end(&mut response))
        .await
        .context("timed out reading the health response")?
        .context("reading the health response")?;

    let text = String::from_utf8_lossy(&response);
    let status = text.lines().next().unwrap_or("(no status line)");
    anyhow::ensure!(status.contains(" 200"), "unhealthy: {status}");
    Ok(())
}

async fn compile(file: &std::path::Path, out: &std::path::Path) -> anyhow::Result<()> {
    let file = tokio::fs::canonicalize(file)
        .await
        .with_context(|| format!("reading {}", file.display()))?;
    let root = file
        .parent()
        .context("the file has no directory")?
        .to_path_buf();
    let main = file
        .file_name()
        .context("the path has no file name")?
        .to_string_lossy()
        .into_owned();

    let work = std::env::temp_dir().join(format!(
        "latex-ai-cli-{}",
        latex_ai_core::fsutil::unique_id()
    ));
    let compiler = Compiler::new(CompilerConfig::new(std::env::current_exe()?, work.clone()));
    let result = compiler
        .compile_scratch(
            &CompileJob {
                root,
                main,
                synctex: false,
                bundle: None,
                timeout: std::time::Duration::from_secs(120),
            },
            &CancellationToken::new(),
        )
        .await;
    let _ = tokio::fs::remove_dir_all(&work).await;
    let result = result.context("tectonic compile")?;

    anyhow::ensure!(
        result.success,
        "compile failed: {}",
        latex_ai_compiler::diagnostics::summarize(&result.diagnostics)
            .unwrap_or_else(|| "no details".to_owned())
    );
    tokio::fs::write(out, &result.pdf)
        .await
        .with_context(|| format!("writing {}", out.display()))?;
    tracing::info!(out = %out.display(), bytes = result.pdf.len(), "compiled");
    Ok(())
}
