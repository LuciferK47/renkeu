//! Tauri desktop shell (masterplan.md §4.7).
//!
//! M5 scope. Today it runs the same axum router on a loopback TCP port and
//! opens the SPA in a webview. Moving non-streaming routes onto
//! `tauri-plugin-axum`'s custom protocol (keeping SSE on TCP per Tauri issue
//! #1404) is tracked as M5 work.
#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use anyhow::Context;
use std::sync::Arc;

/// Loopback port the embedded API listens on; the SPA talks to it directly
/// because the custom protocol cannot stream SSE.
const API_PORT: u16 = 3000;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

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
    };

    let app = latex_ai_server::build_router(state);
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", API_PORT))
        .await
        .with_context(|| format!("binding 127.0.0.1:{API_PORT}"))?;
    tracing::info!(port = API_PORT, "embedded API listening");

    tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            tracing::error!(error = %e, "embedded API stopped");
        }
    });

    tauri::Builder::default()
        .run(tauri::generate_context!())
        .context("running the Tauri application")?;

    Ok(())
}
