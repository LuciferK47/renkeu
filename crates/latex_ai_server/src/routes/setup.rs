//! First-run support: whether the welcome screen has been dealt with, which
//! models Ollama has on this computer, and a one-off compile that fetches the
//! TeX support files so the first real compile is not the slow one.
//!
//! The "done" marker lives in the app's home directory, not in the browser: the
//! desktop app serves the page from a new loopback port on every launch, so
//! anything kept in `localStorage` would be forgotten each time.

use crate::AppState;
use crate::error::ApiError;
use axum::{Json, extract::State};
use latex_ai_compiler::CompileJob;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use tracing::instrument;

const DONE_FILE: &str = "first-run.done";
const PROBE_DIR: &str = "setup-probe";
const PROBE_DOC: &str = "\\documentclass{article}\n\\begin{document}\nReady.\n\\end{document}\n";
/// The first compile downloads the TeX bundle, which can be slow on a poor link.
const PREPARE_TIMEOUT: Duration = Duration::from_secs(600);

/// Overrides where Ollama is looked for, so tests can use a local stand-in.
const OLLAMA_ENV: &str = "LATEX_AI_OLLAMA_BASE";
const OLLAMA_DEFAULT: &str = "http://localhost:11434";
const PROBE_TIMEOUT: Duration = Duration::from_millis(1500);

#[derive(Serialize)]
pub struct SetupState {
    /// The welcome screen was completed or skipped.
    pub done: bool,
}

/// `GET /api/setup`
#[instrument(skip(state))]
pub async fn get_setup(State(state): State<AppState>) -> Json<SetupState> {
    let done = tokio::fs::try_exists(state.settings.home().join(DONE_FILE))
        .await
        .unwrap_or(false);
    Json(SetupState { done })
}

/// `PUT /api/setup`: remember that the welcome screen is done.
///
/// # Errors
/// 500 when the marker cannot be written.
#[instrument(skip(state))]
pub async fn put_setup(State(state): State<AppState>) -> Result<Json<SetupState>, ApiError> {
    let home = state.settings.home();
    let write = async {
        tokio::fs::create_dir_all(home).await?;
        tokio::fs::write(home.join(DONE_FILE), b"").await
    };
    write.await.map_err(|e| ApiError::Internal {
        message: format!("saving the setup state: {e}"),
    })?;
    Ok(Json(SetupState { done: true }))
}

#[derive(Serialize)]
pub struct Prepared {
    pub ok: bool,
    /// Why it failed, in words a person can act on.
    pub message: Option<String>,
}

/// `POST /api/setup/prepare`: compile a one-line document, which downloads the
/// TeX support files on a first run and is instant afterwards. A failure is a
/// result, not an HTTP error, so the screen can show it and offer a retry.
#[instrument(skip(state))]
pub async fn prepare(State(state): State<AppState>) -> Json<Prepared> {
    let outcome = prepare_inner(&state).await;
    Json(match outcome {
        Ok(()) => Prepared {
            ok: true,
            message: None,
        },
        Err(message) => Prepared {
            ok: false,
            message: Some(message),
        },
    })
}

async fn prepare_inner(state: &AppState) -> Result<(), String> {
    let dir = state.settings.home().join(PROBE_DIR);
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| format!("could not create a working folder: {e}"))?;
    tokio::fs::write(dir.join("main.tex"), PROBE_DOC)
        .await
        .map_err(|e| format!("could not write the test document: {e}"))?;
    let root = latex_ai_core::fsutil::canonical(&dir)
        .map_err(|e| format!("could not open the working folder: {e}"))?;
    let bundle = state.settings.snapshot().config.compile.bundle.clone();
    let job = CompileJob {
        root,
        main: "main.tex".to_owned(),
        synctex: false,
        bundle,
        timeout: PREPARE_TIMEOUT,
    };
    let out = state
        .compiler
        .compile_scratch(&job, &CancellationToken::new())
        .await
        .map_err(|e| e.to_string())?;
    if out.success {
        Ok(())
    } else {
        let first = out
            .diagnostics
            .first()
            .map_or_else(|| "the test document did not compile".to_owned(), |d| d.message.clone());
        Err(format!(
            "The TeX support files could not be prepared. Check your internet connection and try again. ({first})"
        ))
    }
}

#[derive(Serialize)]
pub struct LocalModel {
    pub name: String,
    pub size_bytes: u64,
}

#[derive(Serialize)]
pub struct LocalModels {
    /// Ollama answered.
    pub running: bool,
    pub models: Vec<LocalModel>,
}

#[derive(Deserialize)]
struct Tags {
    #[serde(default)]
    models: Vec<Tag>,
}

#[derive(Deserialize)]
struct Tag {
    name: String,
    #[serde(default)]
    size: u64,
}

/// `GET /api/ai/local`: is Ollama running on this computer, and which models
/// does it have? Asks only localhost; never an error, since "not running" is
/// the answer a new user most often gets.
#[instrument]
pub async fn local_models() -> Json<LocalModels> {
    let base = std::env::var(OLLAMA_ENV).unwrap_or_else(|_| OLLAMA_DEFAULT.to_owned());
    Json(local_models_at(&base).await)
}

/// [`local_models`] against a given address, so a test can use a stand-in.
pub async fn local_models_at(base: &str) -> LocalModels {
    let down = LocalModels {
        running: false,
        models: Vec::new(),
    };
    let Ok(client) = reqwest::Client::builder().timeout(PROBE_TIMEOUT).build() else {
        return down;
    };
    let url = format!("{}/api/tags", base.trim_end_matches('/'));
    let Ok(res) = client.get(url).send().await else {
        return down;
    };
    if !res.status().is_success() {
        return down;
    }
    let Ok(tags) = res.json::<Tags>().await else {
        return down;
    };
    let mut models: Vec<LocalModel> = tags
        .models
        .into_iter()
        .map(|t| LocalModel {
            name: t.name,
            size_bytes: t.size,
        })
        .collect();
    models.sort_by(|a, b| a.name.cmp(&b.name));
    LocalModels {
        running: true,
        models,
    }
}
