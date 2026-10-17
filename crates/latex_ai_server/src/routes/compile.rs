//! Compile-job routes: `POST /api/compile` starts a background job for a
//! project, `GET /api/compile/stream/{id}` follows it over SSE using the
//! locked §8.2 taxonomy, `POST /api/compile/{id}/cancel` stops it. Finished
//! compiles are stored as *builds* outside the project and addressed by id.

use crate::AppState;
use crate::error::ApiError;
use crate::jobs::{JobPhase, JobState};
use crate::sse;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use latex_ai_compiler::builds::{Build, is_valid_id};
use latex_ai_compiler::diagnostics::summarize;
use latex_ai_compiler::project::{MainError, detect_main};
use latex_ai_compiler::{CompileError, CompileJob};
use latex_ai_core::security::{AccessKind, validate_path};
use serde::Deserialize;
use serde_json::json;
use std::convert::Infallible;
use std::path::PathBuf;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use tracing::instrument;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompileRequest {
    /// The file the user is looking at. It does not have to be the document
    /// root: a chapter compiles its project's main file.
    pub file_path: String,
    /// Compile this file instead of detecting the root.
    #[serde(default)]
    pub main: Option<String>,
}

/// Work out which file to compile for `active`. Blocking filesystem reads.
pub(crate) async fn resolve_main(
    state: &AppState,
    active: &str,
    explicit: Option<&str>,
) -> Result<String, ApiError> {
    let root = state.workspace_root.clone();
    let (active, explicit) = (active.to_owned(), explicit.map(str::to_owned));
    let found =
        tokio::task::spawn_blocking(move || detect_main(&root, &active, explicit.as_deref()))
            .await
            .map_err(|e| ApiError::Internal {
                message: e.to_string(),
            })?;
    found.map_err(|e| match e {
        MainError::Ambiguous(candidates) => ApiError::MainFileAmbiguous { candidates },
        MainError::NotFound(_) | MainError::NoRoot => ApiError::Unprocessable {
            message: e.to_string(),
        },
        other => ApiError::Internal {
            message: other.to_string(),
        },
    })
}

/// `POST /api/compile` — validate, register a job, compile in the background.
/// The job is intentionally NOT bound to the caller's connection: closing the
/// tab must not kill a compile another view may be waiting on. Use
/// `POST /api/compile/{id}/cancel` to stop one.
#[instrument(skip(state))]
pub async fn compile(
    State(state): State<AppState>,
    Json(req): Json<CompileRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    validate_path(&state.workspace_root, &req.file_path, AccessKind::Read)?;
    let main = resolve_main(&state, &req.file_path, req.main.as_deref()).await?;
    // The main file is read by the engine, so it gets the same sandbox check.
    validate_path(&state.workspace_root, &main, AccessKind::Read)?;

    state.metrics.record_compile_started();
    let job_id = Uuid::new_v4().to_string();
    let (job_tx, cancel) = state.jobs.create(&job_id);

    // Durable audit row alongside the in-memory registry, so a job
    // interrupted by a crash is recoverable (masterplan §12 M5).
    if let Err(e) =
        latex_ai_core::storage::jobs::record_started(state.store.pool(), &job_id, &main).await
    {
        tracing::warn!(error = %e, "could not persist the compile job");
    }

    tokio::spawn(run_compile_job(
        state,
        main.clone(),
        job_tx,
        cancel,
        job_id.clone(),
    ));

    Ok(Json(
        json!({ "job_id": job_id, "status": "queued", "main_file": main }),
    ))
}

/// Close a job's durable row out. A failure to record must not fail the job
/// itself — the compile already happened either way.
async fn close_job_record(
    pool: &sqlx::SqlitePool,
    job_id: &str,
    status: latex_ai_core::storage::jobs::JobStatus,
    error: Option<&str>,
) {
    if let Err(e) = latex_ai_core::storage::jobs::record_finished(pool, job_id, status, error).await
    {
        tracing::warn!(error = %e, "could not close the compile job record");
    }
}

async fn run_compile_job(
    state: AppState,
    main: String,
    job_tx: tokio::sync::watch::Sender<JobState>,
    cancel: CancellationToken,
    job_id: String,
) {
    use latex_ai_core::storage::jobs::JobStatus;

    let pool = state.store.pool().clone();
    let finish_failed = |summary: String, build: Option<&Build>| {
        state.metrics.record_compile_finished(false);
        job_tx.send_replace(JobState {
            phase: JobPhase::Failed,
            message: "compile failed".to_owned(),
            progress: 1.0,
            error_summary: Some(summary),
            build_id: build.map(|b| b.id().to_owned()),
            main_file: Some(main.clone()),
            diagnostics: build
                .map(|b| b.manifest.diagnostics.clone())
                .unwrap_or_default(),
            ..JobState::queued()
        });
    };

    job_tx.send_replace(JobState {
        phase: JobPhase::Compiling,
        message: "Compiling with Tectonic...".to_owned(),
        progress: 0.3,
        main_file: Some(main.clone()),
        ..JobState::queued()
    });

    // SyncTeX is requested on every job: it is what makes click-to-source
    // work, and asking for it after the fact would mean recompiling.
    let job = CompileJob {
        root: state.workspace_root.clone(),
        main: main.clone(),
        synctex: true,
        timeout: Duration::from_secs(state.settings.snapshot().config.compile.timeout_secs),
    };

    let build = match state.compiler.compile(&job, &cancel).await {
        Ok(build) => build,
        Err(e) => {
            let summary = match &e {
                CompileError::Cancelled => "compile cancelled".to_owned(),
                other => other.to_string(),
            };
            close_job_record(&pool, &job_id, JobStatus::Failed, Some(&summary)).await;
            return finish_failed(summary, None);
        }
    };

    if !build.manifest.success {
        let summary = summarize(&build.manifest.diagnostics)
            .unwrap_or_else(|| "the document did not compile".to_owned());
        close_job_record(&pool, &job_id, JobStatus::Failed, Some(&summary)).await;
        return finish_failed(summary, Some(&build));
    }

    let page_count = count_pages(build.pdf_path()).await;
    if !build.manifest.has_synctex {
        tracing::warn!("compile produced no SyncTeX map");
    }

    state.metrics.record_compile_finished(true);
    close_job_record(&pool, &job_id, JobStatus::Succeeded, None).await;
    job_tx.send_replace(JobState {
        phase: JobPhase::Done,
        message: "Compiled".to_owned(),
        progress: 1.0,
        page_count,
        build_id: Some(build.id().to_owned()),
        main_file: Some(main),
        diagnostics: build.manifest.diagnostics.clone(),
        ..JobState::queued()
    });
}

/// Page count via pdfium (blocking bind + parse on the blocking pool).
/// `None` when pdfium is unavailable — the viewer then falls back to 1 page.
async fn count_pages(pdf_path: PathBuf) -> Option<u16> {
    let counted = tokio::task::spawn_blocking(move || {
        visual::render::renderer().ok()?.page_count(&pdf_path).ok()
    })
    .await;
    match counted {
        Ok(n) => n,
        Err(e) => {
            tracing::warn!(error = %e, "page-count task failed");
            None
        }
    }
}

/// `POST /api/compile/{id}/cancel` — stop a running job. The worker process
/// is killed; the job's stream ends with a failed `compiled` event.
#[instrument(skip(state))]
pub async fn cancel_compile(
    Path(id): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if state.jobs.cancel(&id) {
        Ok(Json(json!({ "job_id": id, "cancelled": true })))
    } else {
        Err(ApiError::NotFound {
            message: format!("unknown compile job {id}"),
        })
    }
}

/// `GET /api/compile/stream/{id}` — follow a job as taxonomy SSE:
/// `progress`* -> `compiled` -> `done`.
#[instrument(skip(state))]
pub async fn compile_stream(
    Path(id): Path<String>,
    State(state): State<AppState>,
) -> Result<Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let mut rx = state
        .jobs
        .subscribe(&id)
        .ok_or_else(|| ApiError::NotFound {
            message: format!("unknown compile job {id}"),
        })?;

    let (tx, out) = tokio::sync::mpsc::channel(16);
    tokio::spawn(async move {
        loop {
            let snapshot = rx.borrow_and_update().clone();
            match snapshot.phase {
                JobPhase::Queued | JobPhase::Compiling => {
                    if !sse::send(
                        &tx,
                        "progress",
                        &json!({ "message": snapshot.message, "progress": snapshot.progress }),
                    )
                    .await
                    {
                        return;
                    }
                }
                JobPhase::Done | JobPhase::Failed => {
                    let success = snapshot.phase == JobPhase::Done;
                    sse::send(
                        &tx,
                        "compiled",
                        &json!({
                            "success": success,
                            "page_count": snapshot.page_count,
                            "error_summary": snapshot.error_summary,
                            "build_id": snapshot.build_id,
                            "main_file": snapshot.main_file,
                            "diagnostics": snapshot.diagnostics,
                        }),
                    )
                    .await;
                    sse::send(&tx, "done", &json!({ "success": success })).await;
                    return;
                }
            }
            if rx.changed().await.is_err() {
                // Job task dropped its sender without a terminal state.
                sse::send(&tx, "error", &json!({ "message": "compile job vanished" })).await;
                return;
            }
        }
    });

    Ok(Sse::new(tokio_stream::wrappers::ReceiverStream::new(out)).keep_alive(KeepAlive::default()))
}

async fn find_build(state: &AppState, id: &str) -> Result<Build, ApiError> {
    if !is_valid_id(id) {
        return Err(ApiError::BadRequest {
            message: "malformed build id".to_owned(),
        });
    }
    state
        .compiler
        .builds()
        .get(id)
        .await
        .ok_or_else(|| ApiError::NotFound {
            message: format!("no build {id} (it may have been pruned — compile again)"),
        })
}

/// `GET /api/builds/{id}` — what the compile found: success, main file and
/// every diagnostic with its file and line.
#[instrument(skip(state))]
pub async fn get_build(
    Path(id): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<latex_ai_compiler::builds::Manifest>, ApiError> {
    Ok(Json(find_build(&state, &id).await?.manifest))
}

/// `GET /api/builds/{id}/pdf` — the finished PDF as a download. This is the
/// "Export PDF" action: the PDF is not written next to the sources.
#[instrument(skip(state))]
pub async fn export_pdf(
    Path(id): Path<String>,
    State(state): State<AppState>,
) -> Result<Response, ApiError> {
    let build = find_build(&state, &id).await?;
    if !build.manifest.has_pdf {
        return Err(ApiError::NotFound {
            message: "this build produced no PDF".to_owned(),
        });
    }
    let bytes = tokio::fs::read(build.pdf_path())
        .await
        .map_err(|e| ApiError::Internal {
            message: format!("reading the PDF: {e}"),
        })?;
    // The file name comes from the main file's stem, reduced to characters
    // that cannot break out of the header.
    let stem = std::path::Path::new(&build.manifest.main)
        .file_stem()
        .map_or_else(
            || "document".to_owned(),
            |s| s.to_string_lossy().into_owned(),
        );
    let safe: String = stem
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect();

    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "application/pdf".to_owned()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{safe}.pdf\""),
            ),
            // A build id names these exact bytes forever.
            (
                header::CACHE_CONTROL,
                "public, max-age=31536000, immutable".to_owned(),
            ),
        ],
        bytes,
    )
        .into_response())
}
