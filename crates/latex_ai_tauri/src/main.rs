#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use axum::Router;
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // SQLite setup
    let db_path = std::env::current_dir()?.join(".latex-ai.db");
    let db_url = format!("sqlite://{}?mode=rwc", db_path.display());
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(5)
        .connect(&db_url)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to connect to database: {}", e))?;
        
    // Create history table if not exists
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS history (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            file_path TEXT NOT NULL,
            command TEXT NOT NULL,
            new_text TEXT NOT NULL,
            diff TEXT NOT NULL,
            edit_type TEXT NOT NULL,
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP
        )"
    ).execute(&pool).await.map_err(|e| anyhow::anyhow!("Failed to run migrations: {}", e))?;
    
    let store = Arc::new(latex_ai_core::storage::Store::new(
        pool,
        std::env::current_dir()?,
    ));

    let planner = Arc::new(latex_ai_core::ai::GeminiPlanner::new("gemini-2.5-flash"));
    let compiler = latex_ai_compiler::Engine::new(10);
    let queue = Arc::new(latex_ai_compiler::queue::CompilationQueue::new(Arc::new(compiler.clone())));
    let state = latex_ai_server::AppState { planner, compiler, queue, store };
    
    let app = latex_ai_server::build_router(state);
    
    // Fallback: Just spawn axum on a local port if tauri-plugin-axum isn't actually installed/setup
    // In a real scenario we'd use tauri-plugin-axum here.
    let addr = "127.0.0.1:3000";
    let listener = tokio::net::TcpListener::bind(addr).await?;
    
    tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            tracing::error!("Server error: {}", e);
        }
    });

    tauri::Builder::default()
        .run(tauri::generate_context!())
        .map_err(|e| anyhow::anyhow!("error while running tauri application: {}", e))?;
        
    Ok(())
}
