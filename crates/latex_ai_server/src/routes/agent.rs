//! AI agent routes (masterplan.md §6.2, §6.3, §8.2).
//!
//! Every stream emits ONLY events from the locked §8.2 taxonomy, always with
//! JSON object payloads. Every stream is wrapped in
//! [`crate::sse::CancelOnDisconnect`], so a client disconnect cancels the
//! in-flight planner call and Tectonic compile through one token.

use crate::AppState;
use crate::error::ApiError;
use crate::sse::{self, CancelOnDisconnect};
use axum::Json;
use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use latex_ai_compiler::CompileError;
use latex_ai_compiler::scratchpad::ScratchPad;
use latex_ai_core::compile::AgentEdit;
use latex_ai_core::security::{AccessKind, validate_path};
use latex_ai_core::storage::etag_of;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::convert::Infallible;
use std::path::PathBuf;
use tokio_stream::Stream;
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::sync::CancellationToken;
use tracing::instrument;

const MAX_INSTRUCTION_LEN: usize = 10_000;
/// Keep compiler noise fed back to the model (and the UI) bounded.
const MAX_ERROR_SUMMARY_LEN: usize = 2_000;

fn default_max_iterations() -> u8 {
    3
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanRequest {
    pub file_path: String,
    pub instruction: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentLoopRequest {
    pub file_path: String,
    pub instruction: String,
    /// `false` (default): stop after `proposal` + `awaiting_approval` for
    /// review-first UX. `true`: heal-and-save loop per masterplan §6.3.
    #[serde(default)]
    pub auto_apply: bool,
    #[serde(default = "default_max_iterations")]
    pub max_iterations: u8,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApplySelectedRequest {
    pub file_path: String,
    pub edits: Vec<AgentEdit>,
    pub selected_hunk_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct ApplySelectedResponse {
    pub success: bool,
    pub history_id: i64,
    pub new_etag: String,
    pub diff: String,
}

/// Validate + sanitize the shared preamble of both streaming endpoints.
fn prepare(
    state: &AppState,
    file_path: &str,
    instruction: &str,
) -> Result<(PathBuf, String), ApiError> {
    let abs = validate_path(&state.workspace_root, file_path, AccessKind::Read)?;
    let safe_instruction = latex_ai_core::prompt_safety::sanitize(instruction, MAX_INSTRUCTION_LEN)
        .map_err(|e| ApiError::Unprocessable {
            message: e.to_string(),
        })?;
    Ok((abs, safe_instruction))
}

/// Stamp server-computed hunk ids so a proposal can be applied per hunk
/// through `POST /api/agent/apply-selected`.
pub(crate) fn stamp_hunk_ids(edits: &mut [AgentEdit]) {
    for edit in edits.iter_mut() {
        edit.hunk_id = Some(edit.hunk_id().0);
    }
}

/// `POST /api/plan/stream` — one-shot plan, no compile (masterplan §6.2).
/// Emits: `progress` -> `proposal` -> `done` | `error`.
#[instrument(skip(state, req))]
pub async fn plan_stream(
    State(state): State<AppState>,
    Json(req): Json<PlanRequest>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let (abs_path, instruction) = prepare(&state, &req.file_path, &req.instruction)?;

    let cancel = CancellationToken::new();
    let (tx, rx) = tokio::sync::mpsc::channel(32);

    let task_cancel = cancel.clone();
    tokio::spawn(async move {
        let content = match tokio::fs::read_to_string(&abs_path).await {
            Ok(c) => c,
            Err(e) => {
                sse::send(
                    &tx,
                    "error",
                    &json!({ "message": format!("read failed: {e}") }),
                )
                .await;
                return;
            }
        };

        if !sse::send(
            &tx,
            "progress",
            &json!({ "message": "Planning edits...", "progress": 0.1 }),
        )
        .await
        {
            return;
        }

        // Counted before the call so an in-flight or failed request is
        // still visible (§12 M5 audit: integration).
        state.metrics.record_llm_call();
        match state
            .planner
            .plan_edits(&instruction, &content, task_cancel)
            .await
        {
            Ok(mut edits) => {
                stamp_hunk_ids(&mut edits);
                sse::send(
                    &tx,
                    "proposal",
                    &json!({
                        "summary": "AI proposed edits based on your request.",
                        "edit_count": edits.len(),
                        "edits": edits,
                    }),
                )
                .await;
                sse::send(&tx, "done", &json!({ "success": true })).await;
            }
            Err(e) => {
                sse::send(&tx, "error", &json!({ "message": e.to_string() })).await;
            }
        }
    });

    Ok(
        Sse::new(CancelOnDisconnect::new(ReceiverStream::new(rx), cancel))
            .keep_alive(KeepAlive::default()),
    )
}

/// `POST /api/agent/stream` — the self-healing loop (masterplan §6.3).
///
/// Per iteration: `thinking` -> `proposal` -> (`awaiting_approval` and stop,
/// unless `auto_apply`) -> `applying` -> `compiled` -> `done` on success or
/// loop with the compile errors folded into the next instruction.
#[instrument(skip(state, req))]
pub async fn agent_loop_stream(
    State(state): State<AppState>,
    Json(req): Json<AgentLoopRequest>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let (abs_path, instruction) = prepare(&state, &req.file_path, &req.instruction)?;
    let max_iterations = req.max_iterations.clamp(1, 5);

    let cancel = CancellationToken::new();
    let (tx, rx) = tokio::sync::mpsc::channel(64);

    let loop_state = state.clone();
    let rel_path = PathBuf::from(req.file_path.clone());
    let task_cancel = cancel.clone();
    tokio::spawn(async move {
        run_agent_loop(
            loop_state,
            abs_path,
            rel_path,
            instruction,
            req.auto_apply,
            max_iterations,
            task_cancel,
            tx,
        )
        .await;
    });

    Ok(
        Sse::new(CancelOnDisconnect::new(ReceiverStream::new(rx), cancel))
            .keep_alive(KeepAlive::default()),
    )
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
async fn run_agent_loop(
    state: AppState,
    abs_path: PathBuf,
    rel_path: PathBuf,
    original_instruction: String,
    auto_apply: bool,
    max_iterations: u8,
    cancel: CancellationToken,
    tx: tokio::sync::mpsc::Sender<Result<Event, Infallible>>,
) {
    let Ok(initial_content) = tokio::fs::read_to_string(&abs_path).await else {
        sse::send(&tx, "error", &json!({ "message": "could not read file" })).await;
        return;
    };
    let initial_etag = etag_of(&initial_content);

    state.metrics.record_agent_run();

    let mut content = initial_content;
    let mut instruction = original_instruction.clone();
    // Diagnostics from every failed iteration, attached to the history row
    // once the loop finally succeeds (masterplan §7.1, §12 M5).
    let mut failures: Vec<latex_ai_core::storage::jobs::CompileErrorRecord> = Vec::new();

    for iteration in 1..=u32::from(max_iterations) {
        state.metrics.record_agent_iteration();
        if !sse::send(
            &tx,
            "thinking",
            &json!({ "message": "Planning edits...", "iteration": iteration }),
        )
        .await
        {
            return; // client gone; CancelOnDisconnect already fired the token
        }

        state.metrics.record_llm_call();
        let mut edits = match state
            .planner
            .plan_edits(&instruction, &content, cancel.clone())
            .await
        {
            Ok(edits) => edits,
            Err(e) => {
                sse::send(&tx, "error", &json!({ "message": e.to_string() })).await;
                return;
            }
        };
        if edits.is_empty() {
            sse::send(&tx, "error", &json!({ "message": "no edits proposed" })).await;
            return;
        }
        stamp_hunk_ids(&mut edits);

        if !sse::send(
            &tx,
            "proposal",
            &json!({
                "summary": "AI proposed edits based on your request.",
                "edit_count": edits.len(),
                "edits": edits,
                "iteration": iteration,
            }),
        )
        .await
        {
            return;
        }

        if !auto_apply {
            sse::send(
                &tx,
                "awaiting_approval",
                &json!({ "message": "Review the proposal, then apply selected hunks." }),
            )
            .await;
            return;
        }

        sse::send(
            &tx,
            "applying",
            &json!({ "message": "Applying edits to scratchpad..." }),
        )
        .await;

        let session_id = uuid::Uuid::new_v4().to_string();
        let scratch = match ScratchPad::create_for(&session_id, &abs_path) {
            Ok(s) => s,
            Err(e) => {
                sse::send(&tx, "error", &json!({ "message": e.to_string() })).await;
                return;
            }
        };

        if let Err(e) = scratch.apply_edits(&edits).await {
            let summary = truncate(&e.to_string(), MAX_ERROR_SUMMARY_LEN);
            sse::send(
                &tx,
                "compiled",
                &json!({ "success": false, "error_summary": summary }),
            )
            .await;
            instruction = format!(
                "{original_instruction}\n\nThe previous edits failed to apply: {summary}\n\
                 Propose corrected edits (the \"text\" field must match the file verbatim)."
            );
            scratch.cleanup();
            continue;
        }

        let healed = match scratch.read_main().await {
            Ok(c) => c,
            Err(e) => {
                sse::send(&tx, "error", &json!({ "message": e.to_string() })).await;
                scratch.cleanup();
                return;
            }
        };

        match state.queue.submit(&healed, &cancel).await {
            Ok(result) if result.success => {
                scratch.cleanup();
                match state
                    .store
                    .save_with_etag_history(
                        &rel_path,
                        &healed,
                        Some(&initial_etag),
                        &original_instruction,
                        "agent-apply",
                    )
                    .await
                {
                    Ok(outcome) => {
                        // The errors that were healed on the way here are part
                        // of the audit trail for this edit.
                        if !failures.is_empty() {
                            if let Err(e) = latex_ai_core::storage::jobs::record_compile_errors(
                                state.store.pool(),
                                outcome.history_id,
                                &failures,
                            )
                            .await
                            {
                                tracing::warn!(error = %e, "could not record compile errors");
                            }
                        }
                        sse::send(&tx, "compiled", &json!({ "success": true })).await;
                        sse::send(
                            &tx,
                            "done",
                            &json!({
                                "success": true,
                                "history_id": outcome.history_id,
                                "new_etag": outcome.new_etag,
                                "iterations": iteration,
                            }),
                        )
                        .await;
                    }
                    Err(e) => {
                        sse::send(
                            &tx,
                            "error",
                            &json!({ "message": format!("compiled but save failed: {e}") }),
                        )
                        .await;
                    }
                }
                return;
            }
            Ok(_) => {
                scratch.cleanup();
                sse::send(
                    &tx,
                    "compiled",
                    &json!({ "success": false, "error_summary": "empty PDF produced" }),
                )
                .await;
                instruction = format!(
                    "{original_instruction}\n\nThe previous attempt produced an empty PDF. Fix the document."
                );
            }
            Err(CompileError::Tectonic(msg)) => {
                scratch.cleanup();
                let summary = truncate(&msg, MAX_ERROR_SUMMARY_LEN);
                failures.push(
                    latex_ai_core::storage::jobs::CompileErrorRecord::engine_failure(
                        iteration,
                        summary.clone(),
                    ),
                );
                sse::send(
                    &tx,
                    "compiled",
                    &json!({ "success": false, "error_summary": summary }),
                )
                .await;
                instruction = format!(
                    "{original_instruction}\n\nCompilation errors so far:\n{summary}\nFix them."
                );
                content = healed;
            }
            Err(CompileError::Cancelled) => {
                scratch.cleanup();
                return; // disconnect: token already cancelled, just stop
            }
            Err(e) => {
                scratch.cleanup();
                sse::send(&tx, "error", &json!({ "message": e.to_string() })).await;
                return;
            }
        }
    }

    sse::send(
        &tx,
        "done",
        &json!({ "success": false, "message": "Max iterations reached" }),
    )
    .await;
}

/// Structural validation for every edit in a request, before any filesystem
/// work (masterplan §12 M4 audit: security).
fn validate_edits(edits: &[AgentEdit]) -> Result<(), ApiError> {
    for edit in edits {
        edit.validate().map_err(|e| ApiError::Unprocessable {
            message: e.to_string(),
        })?;
    }
    Ok(())
}

/// `POST /api/agent/apply-selected` — write the reviewed subset atomically
/// (masterplan §6.5). Hunk ids are recomputed server-side; the client's copy
/// is never trusted.
#[instrument(skip(state, req))]
pub async fn apply_selected(
    State(state): State<AppState>,
    Json(req): Json<ApplySelectedRequest>,
) -> Result<Json<ApplySelectedResponse>, ApiError> {
    validate_path(&state.workspace_root, &req.file_path, AccessKind::Write)?;
    validate_edits(&req.edits)?;

    let selected: Vec<&AgentEdit> = req
        .edits
        .iter()
        .filter(|edit| req.selected_hunk_ids.contains(&edit.hunk_id().0))
        .collect();

    if selected.is_empty() {
        return Err(ApiError::BadRequest {
            message: "no edits match the selected hunk ids".to_owned(),
        });
    }

    let outcome = state
        .store
        .atomic_apply_selected(
            std::path::Path::new(&req.file_path),
            &selected,
            "apply-selected",
        )
        .await?;

    state.metrics.record_edits_applied(selected.len() as u64);

    Ok(Json(ApplySelectedResponse {
        success: true,
        history_id: outcome.history_id,
        new_etag: outcome.new_etag,
        diff: outcome.diff,
    }))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreviewRequest {
    pub file_path: String,
    pub edits: Vec<AgentEdit>,
}

/// What one proposed hunk would do to the file as it stands.
#[derive(Debug, Serialize)]
pub struct HunkPreview {
    pub hunk_id: String,
    /// Unified diff of applying this hunk alone. Empty when it cannot apply.
    pub diff: String,
    /// `false` when the hunk no longer matches the file — the anchor text is
    /// gone, a range is out of bounds, an earlier accepted hunk changed it.
    pub applies: bool,
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PreviewResponse {
    /// Current `ETag`; echo it back as `expected_etag` to make the apply
    /// conditional on the file not having moved underneath the review.
    pub etag: String,
    pub hunks: Vec<HunkPreview>,
}

/// `POST /api/agent/preview` — per-hunk diffs, writing nothing.
///
/// Each hunk is applied to the *current* file independently, so one broken
/// hunk does not hide the others and the diff shown is exactly what
/// `apply-selected` would produce for that hunk. Sharing `core::apply` is the
/// point: a preview computed by different code would eventually drift from
/// what actually happens.
#[instrument(skip(state, req))]
pub async fn preview(
    State(state): State<AppState>,
    Json(req): Json<PreviewRequest>,
) -> Result<Json<PreviewResponse>, ApiError> {
    validate_path(&state.workspace_root, &req.file_path, AccessKind::Read)?;
    validate_edits(&req.edits)?;

    let (content, etag) = state
        .store
        .read_with_etag(std::path::Path::new(&req.file_path))
        .await?;

    let header = req.file_path.as_str();
    let hunks = latex_ai_core::compile::dedupe_hunks(&req.edits.iter().collect::<Vec<_>>())
        .into_iter()
        .map(
            |edit| match latex_ai_core::apply::apply_edit(&content, edit) {
                Ok(updated) => HunkPreview {
                    hunk_id: edit.hunk_id().0,
                    diff: similar::TextDiff::from_lines(&content, &updated)
                        .unified_diff()
                        .header(header, header)
                        .to_string(),
                    applies: true,
                    error: None,
                },
                Err(e) => HunkPreview {
                    hunk_id: edit.hunk_id().0,
                    diff: String::new(),
                    applies: false,
                    error: Some(e.to_string()),
                },
            },
        )
        .collect();

    Ok(Json(PreviewResponse { etag, hunks }))
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_owned()
    } else {
        let mut cut = max;
        while !s.is_char_boundary(cut) {
            cut -= 1;
        }
        format!("{}…", &s[..cut])
    }
}
