//! Tauri desktop shell (masterplan.md §4.7).
//!
//! Runs the same axum router the daemon does, on an ephemeral loopback port,
//! and opens the SPA in a webview.
//!
//! The server is spawned on **Tauri's** async runtime, not via `#[tokio::main]`
//! and `tokio::spawn`. `tauri::run()` installs its own runtime and blocks; tasks
//! spawned on a separate `#[tokio::main]` runtime are orphaned the moment it
//! does, which silently killed the embedded server (the process kept running
//! with the listener closed). `tauri-plugin-axum` was evaluated and rejected —
//! see masterplan §15.1.
#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use anyhow::Context;

fn main() -> anyhow::Result<()> {
    // Compile workers are this same binary: run as one before anything else
    // (no logging, no webview) if that is what we were started as.
    latex_ai_compiler::worker::run_if_requested();

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
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![pick_project])
        .setup(move |handle| {
            // The bundle carries its own libpdfium (see tauri.conf.json); tell
            // the renderer where, before the first page is asked for.
            if let Ok(resources) = tauri::Manager::path(handle).resource_dir() {
                latex_ai_server::bootstrap::add_pdfium_search_dir(resources.join("pdfium"));
            }
            tauri::WebviewWindowBuilder::new(
                handle,
                "main",
                tauri::WebviewUrl::App("index.html".into()),
            )
            .title("Renkeu")
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

/// Let the user choose a project folder, remember it and restart into it.
///
/// A restart is the whole switch: the server, its history database, the build
/// store and every open tab belong to one project, and tearing those down and
/// rebuilding them in place would be a second, less-tested way to start. Resolves
/// `false` when the dialog is cancelled; on success the process restarts and
/// this never resolves.
#[tauri::command]
async fn pick_project(app: tauri::AppHandle) -> Result<bool, String> {
    use tauri_plugin_dialog::DialogExt;

    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog().file().pick_folder(move |picked| {
        // A closed receiver means the window went away; nothing to report.
        let _ = tx.send(picked);
    });
    let Some(picked) = rx.await.map_err(|e| e.to_string())? else {
        return Ok(false);
    };
    let folder = picked
        .into_path()
        .map_err(|e| format!("that location is not a folder on this computer: {e}"))?;
    let home = latex_ai_core::config::default_home();
    latex_ai_server::bootstrap::remember_project(&home, &folder).map_err(|e| format!("{e:#}"))?;
    app.restart()
}

/// Build the app state, bind the loopback listener, and start serving on
/// Tauri's runtime. Returns the port the OS chose.
async fn start_api() -> anyhow::Result<u16> {
    let opts = latex_ai_server::bootstrap::BootOptions::for_desktop()?;
    let state = latex_ai_server::bootstrap::build_state(opts)
        .await
        .context("starting up")?;

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
