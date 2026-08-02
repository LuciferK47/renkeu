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
        Command::Serve { port, host } => serve(&host, port).await,
        Command::Compile { file, out } => compile(&file, &out).await,
    }
}

async fn serve(host: &str, port: u16) -> anyhow::Result<()> {
    // Model must be one genai 0.1.23 recognizes (unknown names fall back to Ollama).
    let model =
        std::env::var("LATEX_AI_MODEL").unwrap_or_else(|_| "llama-3.3-70b-versatile".to_string());
    let planner: std::sync::Arc<dyn latex_ai_core::ai::EditPlanner> = std::sync::Arc::new(
        latex_ai_core::ai::LlmPlanner::new(model.clone())
            .with_vision_model(std::env::var("LATEX_AI_VISION_MODEL").ok()),
    );

    // The workspace root all API paths are sandboxed to (canonicalized so the
    // security checks compare like with like).
    let workspace_root =
        std::fs::canonicalize(std::env::current_dir()?).context("canonicalizing workspace root")?;

    // SQLite setup
    let db_path = workspace_root.join(".latex-ai.db");
    let db_url = format!("sqlite://{}?mode=rwc", db_path.display());
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(5)
        .connect(&db_url)
        .await
        .with_context(|| "Failed to connect to database")?;

    sqlx::migrate!("./migrations").run(&pool).await?;

    // SQLite leaves foreign keys off unless asked; compile_errors relies on
    // its FK and cascade (masterplan §12 M5 audit: data integrity).
    sqlx::query("PRAGMA foreign_keys = ON")
        .execute(&pool)
        .await?;

    // A job still marked running belongs to a process that died. Close those
    // out so the queue does not show phantom work forever.
    match latex_ai_core::storage::jobs::recover_abandoned(&pool).await {
        Ok(0) => {}
        Ok(n) => tracing::warn!(
            count = n,
            "recovered abandoned compile jobs from a previous run"
        ),
        Err(e) => tracing::warn!(error = %e, "compile-job recovery failed"),
    }

    let store = std::sync::Arc::new(latex_ai_core::storage::Store::new(
        pool,
        workspace_root.clone(),
    ));

    let engine = std::sync::Arc::new(latex_ai_compiler::Engine::new(10));
    let queue = std::sync::Arc::new(latex_ai_compiler::queue::CompilationQueue::new(engine));

    let state = latex_ai_server::AppState {
        planner,
        queue,
        store,
        jobs: std::sync::Arc::new(latex_ai_server::jobs::JobRegistry::default()),
        workspace_root,
        model,
        metrics: std::sync::Arc::new(latex_ai_core::metrics::Metrics::new()),
    };
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

async fn compile(file: &std::path::Path, out: &std::path::Path) -> anyhow::Result<()> {
    let latex = tokio::fs::read_to_string(file)
        .await
        .with_context(|| format!("reading {}", file.display()))?;
    let engine = Engine::new(10);
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
