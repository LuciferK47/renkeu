//! `latex-ai` binary entrypoint. M1 subcommands: `serve`, `compile`.
#![forbid(unsafe_code)]

use std::path::PathBuf;

use anyhow::Context;
use clap::{Parser, Subcommand};
use latex_ai_compiler::Engine;
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
        /// TCP port to bind on 127.0.0.1.
        #[arg(long, default_value_t = 3000)]
        port: u16,
    },
    /// Compile a LaTeX file to PDF in-process via Tectonic.
    Compile {
        /// Path to the main `.tex` file.
        file: PathBuf,
        /// Output PDF path.
        #[arg(short, long, default_value = "output.pdf")]
        out: PathBuf,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    match Cli::parse().command {
        Command::Serve { port } => serve(port).await,
        Command::Compile { file, out } => compile(&file, &out).await,
    }
}

async fn serve(port: u16) -> anyhow::Result<()> {
    let app = latex_ai_server::build_router();
    let addr = format!("127.0.0.1:{port}");
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .with_context(|| format!("binding {addr}"))?;
    tracing::info!(%addr, "latex-ai serving");
    axum::serve(listener, app).await.context("axum serve")?;
    Ok(())
}

async fn compile(file: &std::path::Path, out: &std::path::Path) -> anyhow::Result<()> {
    let latex = tokio::fs::read_to_string(file)
        .await
        .with_context(|| format!("reading {}", file.display()))?;
    let engine = Engine::default();
    let result = engine
        .compile(&latex, &CancellationToken::new())
        .await
        .context("tectonic compile")?;
    anyhow::ensure!(result.success, "compile produced empty PDF");
    tokio::fs::write(out, result.pdf_bytes.as_ref())
        .await
        .with_context(|| format!("writing {}", out.display()))?;
    tracing::info!(out = %out.display(), bytes = result.pdf_bytes.len(), "compiled");
    Ok(())
}
