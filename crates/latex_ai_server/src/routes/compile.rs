//! Compile-job routes: `POST /api/compile` starts a background job for a
//! workspace file, `GET /api/compile/stream/{id}` follows it over SSE using
//! the locked §8.2 taxonomy. Replaces the M2-era stub that ignored the job id
//! and hardcoded `examples/sample.tex`.

use crate::AppState;
use crate::error::ApiError;
use crate::jobs::{JobPhase, JobState};
use crate::sse;
use axum::Json;
use axum::extract::{Path, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use latex_ai_core::security::{AccessKind, validate_path};
use serde::Deserialize;
use serde_json::json;
use std::convert::Infallible;
use std::path::PathBuf;
use tokio_util::sync::CancellationToken;
use tracing::instrument;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompileRequest {
    pub file_path: String,
}

/// `POST /api/compile` — validate, register a job, compile in the background.
/// The job is intentionally NOT bound to the caller's connection: closing the
/// tab must not kill a compile another view may be waiting on.
#[instrument(skip(state))]
pub async fn compile(
    State(state): State<AppState>,
    Json(req): Json<CompileRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let abs_path = validate_path(&state.workspace_root, &req.file_path, AccessKind::Read)?;

    state.metrics.record_compile_started();
    let job_id = Uuid::new_v4().to_string();
    let job_tx = state.jobs.create(&job_id);

    // Durable audit row alongside the in-memory registry, so a job
    // interrupted by a crash is recoverable (masterplan §12 M5).
    if let Err(e) =
        latex_ai_core::storage::jobs::record_started(state.store.pool(), &job_id, &req.file_path)
            .await
    {
        tracing::warn!(error = %e, "could not persist the compile job");
    }

    tokio::spawn(run_compile_job(state, abs_path, job_tx, job_id.clone()));

    Ok(Json(json!({ "job_id": job_id, "status": "queued" })))
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
    abs_path: PathBuf,
    job_tx: tokio::sync::watch::Sender<JobState>,
    job_id: String,
) {
    use latex_ai_core::storage::jobs::JobStatus;

    let metrics = state.metrics.clone();
    let pool = state.store.pool().clone();
    let fail = |job_tx: &tokio::sync::watch::Sender<JobState>, summary: String| {
        metrics.record_compile_finished(false);
        job_tx.send_replace(JobState {
            phase: JobPhase::Failed,
            message: "compile failed".to_owned(),
            progress: 1.0,
            page_count: None,
            error_summary: Some(summary),
        });
    };

    let content = match tokio::fs::read_to_string(&abs_path).await {
        Ok(c) => c,
        Err(e) => {
            let summary = format!("read failed: {e}");
            close_job_record(&pool, &job_id, JobStatus::Failed, Some(&summary)).await;
            return fail(&job_tx, summary);
        }
    };

    job_tx.send_replace(JobState {
        phase: JobPhase::Compiling,
        message: "Compiling with Tectonic...".to_owned(),
        progress: 0.3,
        page_count: None,
        error_summary: None,
    });

    // SyncTeX is requested on every job: it is what makes click-to-source
    // work, and asking for it after the fact would mean recompiling.
    let tex_name = abs_path.file_name().map_or_else(
        || "document.tex".to_owned(),
        |n| n.to_string_lossy().into_owned(),
    );

    let result = match state
        .queue
        .submit_with_synctex(&content, &tex_name, &CancellationToken::new())
        .await
    {
        Ok(r) if r.success => r,
        Ok(_) => {
            close_job_record(&pool, &job_id, JobStatus::Failed, Some("empty PDF")).await;
            return fail(&job_tx, "empty PDF produced".to_owned());
        }
        Err(e) => {
            let summary = e.to_string();
            close_job_record(&pool, &job_id, JobStatus::Failed, Some(&summary)).await;
            return fail(&job_tx, summary);
        }
    };

    // Persist next to the source so GET /api/pdf/page can rasterize it.
    let pdf_path = abs_path.with_extension("pdf");
    if let Err(e) = latex_ai_core::fsutil::atomic_write(&pdf_path, result.pdf_bytes.as_ref()).await {
        let summary = format!("could not write PDF: {e}");
        close_job_record(&pool, &job_id, JobStatus::Failed, Some(&summary)).await;
        return fail(&job_tx, summary);
    }

    // …and the source map beside it, for POST /api/visual-fix.
    let synctex_path = abs_path.with_extension("synctex.gz");
    if let Some(gz) = &result.synctex_gz {
        if let Err(e) = latex_ai_core::fsutil::atomic_write(&synctex_path, gz.as_ref()).await {
            // Not fatal: the PDF is still usable, only click-to-source is.
            tracing::warn!(error = %e, "could not write the SyncTeX map");
        }
    } else {
        // A stale map would silently point at the wrong lines.
        if let Err(e) = tokio::fs::remove_file(&synctex_path).await {
            if e.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(error = %e, "could not remove the stale SyncTeX map");
            }
        }
        tracing::warn!("compile produced no SyncTeX map");
    }

    let page_count = count_pages(pdf_path).await;

    metrics.record_compile_finished(true);
    close_job_record(&pool, &job_id, JobStatus::Succeeded, None).await;
    job_tx.send_replace(JobState {
        phase: JobPhase::Done,
        message: "Compiled".to_owned(),
        progress: 1.0,
        page_count,
        error_summary: None,
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
                JobPhase::Done => {
                    sse::send(
                        &tx,
                        "compiled",
                        &json!({ "success": true, "page_count": snapshot.page_count }),
                    )
                    .await;
                    sse::send(&tx, "done", &json!({ "success": true })).await;
                    return;
                }
                JobPhase::Failed => {
                    sse::send(
                        &tx,
                        "compiled",
                        &json!({ "success": false, "error_summary": snapshot.error_summary }),
                    )
                    .await;
                    sse::send(&tx, "done", &json!({ "success": false })).await;
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
