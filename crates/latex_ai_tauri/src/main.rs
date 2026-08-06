//! Tauri desktop shell (masterplan.md §4.7).
//!
//! Runs the same axum router the daemon does, on an ephemeral loopback port,
//! and opens the SPA in a webview.
//!
//! The server is spawned on **Tauri's** async runtime, not via `#[tokio::main]`
//! + `tokio::spawn`. `tauri::run()` installs its own runtime and blocks; tasks
//! spawned on a separate `#[tokio::main]` runtime are orphaned the moment it
//! does, which silently killed the embedded server (the process kept running
//! with the listener closed). `tauri-plugin-axum` was evaluated and rejected —
//! see masterplan §15.1.
#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use anyhow::Context;
use std::sync::Arc;

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    // Everything async runs on Tauri's runtime so the spawned server shares
    // its lifetime with the GUI.
    let api_port = tauri::async_runtime::block_on(start_api())?;

    // The SPA is served from `tauri://localhost`, so a relative `/api/...`
    // resolves against THAT origin and never reaches the server. The window is
    // therefore built in code rather than from tauri.conf.json, so the real
    // base URL can be injected before any page script runs.
    let api_base = format!("http://127.0.0.1:{api_port}/api");

    tauri::Builder::default()
        .setup(move |handle| {
            tauri::WebviewWindowBuilder::new(
                handle,
                "main",
                tauri::WebviewUrl::App("index.html".into()),
            )
            .title("LaTeX-AI")
            .inner_size(1200.0, 800.0)
            .initialization_script(format!(
                "globalThis.__LATEX_AI_API__ = {};",
                serde_json::to_string(&api_base)?
            ))
            .build()?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .context("running the Tauri application")?;

    Ok(())
}

/// Build the app state, bind the loopback listener, and start serving on
/// Tauri's runtime. Returns the port the OS chose.
async fn start_api() -> anyhow::Result<u16> {
    let workspace_root =
        std::fs::canonicalize(std::env::current_dir()?).context("canonicalizing workspace root")?;

    let db_path = workspace_root.join(".latex-ai.db");
    let db_url = format!("sqlite://{}?mode=rwc", db_path.display());
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(5)
        .connect(&db_url)
        .await
        .context("connecting to the history database")?;

    // Same migration set the daemon runs — no hand-rolled CREATE TABLE.
    sqlx::migrate!("../latex_ai_server/migrations")
        .run(&pool)
        .await
        .context("running migrations")?;

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

    let store = Arc::new(latex_ai_core::storage::Store::new(
        pool,
        workspace_root.clone(),
    ));

    let model =
        std::env::var("LATEX_AI_MODEL").unwrap_or_else(|_| "llama-3.3-70b-versatile".to_string());
    let planner: Arc<dyn latex_ai_core::ai::EditPlanner> = Arc::new(
        latex_ai_core::ai::LlmPlanner::new(model.clone())
            .with_vision_model(std::env::var("LATEX_AI_VISION_MODEL").ok()),
    );
    let engine = Arc::new(latex_ai_compiler::Engine::new(10));
    let queue = Arc::new(latex_ai_compiler::queue::CompilationQueue::new(engine));

    let state = latex_ai_server::AppState {
        planner,
        queue,
        store,
        jobs: Arc::new(latex_ai_server::jobs::JobRegistry::default()),
        workspace_root,
        model,
        metrics: std::sync::Arc::new(latex_ai_core::metrics::Metrics::new()),
    };

    let app = latex_ai_server::build_router(state).context("building the router")?;

    // Port 0: the OS picks a free one. A fixed port would collide with a dev
    // server and be guessable by anything else on the machine.
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .context("binding the loopback API port")?;
    let api_port = listener
        .local_addr()
        .context("reading the bound API port")?
        .port();
    tracing::info!(port = api_port, "embedded API listening");

    // Spawned on Tauri's runtime (we are inside its `block_on`), so the task
    // outlives this function and survives `tauri::run()`.
    tauri::async_runtime::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            tracing::error!(error = %e, "embedded API stopped");
        }
    });

    Ok(api_port)
}
