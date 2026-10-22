//! AI agent routes (masterplan.md §6.2, §6.3, §8.2).
//!
//! The loop itself lives in [`crate::agent`]; this file is the HTTP edge:
//! validate, start a run, and apply reviewed hunks. Every stream emits ONLY
//! events from the locked §8.2 taxonomy with JSON object payloads, and is
//! wrapped in [`CancelOnDisconnect`], so a client disconnect cancels the
//! in-flight planner call and compile through one token.

use crate::AppState;
use crate::agent::{LoopRequest, run_loop, run_plan};
use crate::error::ApiError;
use crate::sse::{CancelOnDisconnect, EventSink};
use axum::Json;
use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use latex_ai_core::apply::{apply_to_files, rebase_edits};
use latex_ai_core::compile::AgentEdit;
use latex_ai_core::security::{AccessKind, validate_path};
use latex_ai_core::storage::{FileChange, StorageError};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::convert::Infallible;
use std::path::{Path, PathBuf};
use tokio_stream::Stream;
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::sync::CancellationToken;
use tracing::instrument;

const MAX_INSTRUCTION_LEN: usize = 10_000;

/// A limit beyond this is a typo, not a venue rule.
const MAX_PAGE_LIMIT: u32 = 10_000;

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
    /// What to change. Optional when `pages` is set.
    #[serde(default)]
    pub instruction: String,
    /// `false` (default): run the loop, verify the result compiles, then
    /// stop at `awaiting_approval` with a reviewable proposal. `true`:
    /// write the verified result (masterplan §6.3).
    #[serde(default)]
    pub auto_apply: bool,
    #[serde(default = "default_max_iterations")]
    pub max_iterations: u8,
    /// Fit the document to this many pages. Then `instruction` is optional
    /// extra guidance, and the loop only succeeds when the page count is met.
    #[serde(default)]
    pub pages: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApplySelectedRequest {
    pub file_path: String,
    pub edits: Vec<AgentEdit>,
    pub selected_hunk_ids: Vec<String>,
    /// `ETag` of `file_path` as the proposal saw it. Required: without it an
    /// edit such as `rewrite_file` would overwrite concurrent changes.
    pub expected_etag: String,
    /// The instruction that produced the proposal, kept in the history row.
    #[serde(default)]
    pub instruction: Option<String>,
    /// The proposal these edits came from. Needed for edits that span several
    /// files, and for `rebase`.
    #[serde(default)]
    pub proposal_id: Option<String>,
    /// If a file changed since the proposal, move the hunks onto the current
    /// text instead of answering 409 — when they still fit.
    #[serde(default)]
    pub rebase: bool,
}

#[derive(Debug, Serialize)]
pub struct AppliedFile {
    pub path: String,
    pub history_id: i64,
    pub new_etag: String,
}

#[derive(Debug, Serialize)]
pub struct ApplySelectedResponse {
    pub success: bool,
    /// History row of `file_path`.
    pub history_id: i64,
    /// New `ETag` of `file_path`.
    pub new_etag: String,
    pub diff: String,
    /// Every file written (more than one for a multi-file proposal).
    pub files: Vec<AppliedFile>,
    /// The hunks were moved onto a file that had changed.
    pub rebased: bool,
}

/// A missing API key is a 503 up front, not a 200 stream that ends in an
/// error event the client has to know to look for.
pub(crate) fn ensure_ai_available(state: &AppState) -> Result<(), ApiError> {
    if state.planner.is_available() {
        Ok(())
    } else {
        Err(ApiError::Unavailable {
            message: "AI is not configured: add an API key or choose a local model in Settings".to_owned(),
        })
    }
}

/// Validate + sanitize the shared preamble of both streaming endpoints.
fn prepare(
    state: &AppState,
    file_path: &str,
    instruction: &str,
    access: AccessKind,
) -> Result<(PathBuf, String), ApiError> {
    ensure_ai_available(state)?;
    let abs = validate_path(&state.workspace_root, file_path, access)?;
    let safe_instruction = latex_ai_core::prompt_safety::sanitize(instruction, MAX_INSTRUCTION_LEN)
        .map_err(|e| ApiError::Unprocessable {
            message: e.to_string(),
        })?;
    Ok((abs, safe_instruction))
}

/// The project-relative form of a path the client sent.
fn relative(state: &AppState, abs: &Path, requested: &str) -> String {
    abs.strip_prefix(&state.workspace_root).map_or_else(
        |_| requested.trim_start_matches("./").to_owned(),
        latex_ai_core::fsutil::to_slash,
    )
}

fn stream_of(
    cancel: CancellationToken,
    rx: tokio::sync::mpsc::Receiver<Result<Event, Infallible>>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    Sse::new(CancelOnDisconnect::new(ReceiverStream::new(rx), cancel))
        .keep_alive(KeepAlive::default())
}

/// `POST /api/plan/stream` — one-shot plan, no compile (masterplan §6.2).
/// Emits: `progress` -> `proposal` -> `done` (or `error` -> `done`).
#[instrument(skip(state, req))]
pub async fn plan_stream(
    State(state): State<AppState>,
    Json(req): Json<PlanRequest>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let (abs, instruction) = prepare(&state, &req.file_path, &req.instruction, AccessKind::Read)?;
    let active = relative(&state, &abs, &req.file_path);

    let cancel = CancellationToken::new();
    let (tx, rx) = tokio::sync::mpsc::channel(32);
    tokio::spawn(run_plan(
        state,
        active,
        instruction,
        cancel.clone(),
        EventSink::new(tx),
    ));
    Ok(stream_of(cancel, rx))
}

/// `POST /api/agent/stream` — the verify-first loop (masterplan §6.3).
///
/// Per iteration: `thinking` -> `proposal`/`step` -> `applying` -> `compiled`.
/// It ends with `done` on success or after the last iteration; review mode
/// first offers the verified result as a `proposal` and `awaiting_approval`.
#[instrument(skip(state, req))]
pub async fn agent_loop_stream(
    State(state): State<AppState>,
    Json(req): Json<AgentLoopRequest>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    // Auto-apply writes the file, so it is validated as a write up front
    // (it used to be checked as a read, letting a compiling `rewrite_file`
    // overwrite any readable file).
    let access = if req.auto_apply {
        AccessKind::Write
    } else {
        AccessKind::Read
    };
    if req.pages.is_some_and(|p| p == 0 || p > MAX_PAGE_LIMIT) {
        return Err(ApiError::Unprocessable {
            message: format!("a page limit is between 1 and {MAX_PAGE_LIMIT}"),
        });
    }
    // Fitting a page limit needs no words from the user; everything else does.
    let wordless = req.pages.is_some() && req.instruction.trim().is_empty();
    let said = if wordless {
        "fit the page limit"
    } else {
        &req.instruction
    };
    let (abs, mut instruction) = prepare(&state, &req.file_path, said, access)?;
    if wordless {
        instruction.clear();
    }

    let run = LoopRequest {
        active: relative(&state, &abs, &req.file_path),
        instruction,
        auto_apply: req.auto_apply,
        // Shortening takes more tries than fixing an error does.
        max_iterations: req
            .max_iterations
            .clamp(1, if req.pages.is_some() { 8 } else { 5 }),
        page_limit: req.pages,
    };
    let cancel = CancellationToken::new();
    let (tx, rx) = tokio::sync::mpsc::channel(64);
    tokio::spawn(run_loop(state, run, cancel.clone(), EventSink::new(tx)));
    Ok(stream_of(cancel, rx))
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

fn unprocessable(message: impl ToString) -> ApiError {
    ApiError::Unprocessable {
        message: message.to_string(),
    }
}

/// `POST /api/agent/apply-selected` — write the reviewed subset atomically
/// (masterplan §6.5). Hunk ids are recomputed server-side; the client's copy
/// is never trusted.
///
/// A proposal that spans several files is written as one all-or-nothing
/// change. If a file moved since the proposal and `rebase` is set, the hunks
/// are re-pointed onto the current text when every one still fits, else 409.
#[instrument(skip(state, req))]
#[allow(clippy::too_many_lines)]
pub async fn apply_selected(
    State(state): State<AppState>,
    Json(req): Json<ApplySelectedRequest>,
) -> Result<Json<ApplySelectedResponse>, ApiError> {
    let primary_abs = validate_path(&state.workspace_root, &req.file_path, AccessKind::Write)?;
    let primary = relative(&state, &primary_abs, &req.file_path);
    validate_edits(&req.edits)?;

    // Ids come from the edits as sent; paths are tidied afterwards.
    let mut selected: Vec<AgentEdit> = req
        .edits
        .iter()
        .filter(|edit| req.selected_hunk_ids.contains(&edit.hunk_id().0))
        .cloned()
        .collect();
    if selected.is_empty() {
        return Err(ApiError::BadRequest {
            message: "no edits match the selected hunk ids".to_owned(),
        });
    }
    let mut by_file: BTreeMap<String, Vec<AgentEdit>> = BTreeMap::new();
    for edit in &mut selected {
        let abs = validate_path(&state.workspace_root, &edit.file_path, AccessKind::Write)?;
        edit.file_path = relative(&state, &abs, &edit.file_path);
        by_file
            .entry(edit.file_path.clone())
            .or_default()
            .push(edit.clone());
    }

    let proposal = match req.proposal_id.as_deref() {
        Some(id) => Some(state.proposals.get(id).ok_or_else(|| ApiError::NotFound {
            message: "that proposal has expired — run the AI again".to_owned(),
        })?),
        None => None,
    };
    let instruction = req
        .instruction
        .clone()
        .or_else(|| proposal.as_ref().map(|p| p.instruction.clone()))
        .unwrap_or_else(|| "apply-selected".to_owned());

    // ---- one file: the common case, with its own conflict handling ----
    if by_file.len() == 1 && by_file.contains_key(&primary) {
        let edits = &by_file[&primary];
        let refs: Vec<&AgentEdit> = edits.iter().collect();
        let first = state
            .store
            .atomic_apply_selected(
                Path::new(&primary),
                &refs,
                &req.expected_etag,
                &instruction,
                "apply-selected",
            )
            .await;

        let (outcome, rebased) = match first {
            Ok(outcome) => (outcome, false),
            Err(StorageError::EtagConflict { actual, expected }) if req.rebase => {
                let conflict = || ApiError::EtagConflict {
                    current_etag: actual.clone(),
                };
                let base = proposal
                    .as_ref()
                    .and_then(|p| p.base.get(&primary))
                    .ok_or_else(conflict)?;
                let (current, current_etag) =
                    state.store.read_with_etag(Path::new(&primary)).await?;
                let moved = rebase_edits(&base.content, &current, edits).map_err(|e| {
                    ApiError::Conflict {
                        message: format!(
                            "{e}; the file changed after the proposal (it was {expected})"
                        ),
                    }
                })?;
                let refs: Vec<&AgentEdit> = moved.iter().collect();
                let outcome = state
                    .store
                    .atomic_apply_selected(
                        Path::new(&primary),
                        &refs,
                        &current_etag,
                        &instruction,
                        "apply-selected",
                    )
                    .await?;
                (outcome, true)
            }
            Err(e) => return Err(e.into()),
        };

        state.metrics.record_edits_applied(selected.len() as u64);
        return Ok(Json(ApplySelectedResponse {
            success: true,
            history_id: outcome.history_id,
            new_etag: outcome.new_etag.clone(),
            diff: outcome.diff,
            files: vec![AppliedFile {
                path: primary,
                history_id: outcome.history_id,
                new_etag: outcome.new_etag,
            }],
            rebased,
        }));
    }

    // ---- several files: all-or-nothing, against the proposal's base ----
    let proposal = proposal.ok_or_else(|| {
        unprocessable("edits that span several files need the proposal_id they came from")
    })?;

    let mut current: BTreeMap<String, (String, String)> = BTreeMap::new();
    let mut moved_any = false;
    let mut final_edits: Vec<AgentEdit> = Vec::new();
    for (file, edits) in &by_file {
        let (content, etag) = state.store.read_with_etag(Path::new(file)).await?;
        let expected = if *file == primary {
            req.expected_etag.clone()
        } else {
            proposal
                .base
                .get(file)
                .map(|b| b.etag.clone())
                .ok_or_else(|| unprocessable(format!("`{file}` is not part of that proposal")))?
        };

        if etag == expected {
            final_edits.extend(edits.iter().cloned());
        } else if req.rebase {
            let base = proposal.base.get(file).ok_or(ApiError::EtagConflict {
                current_etag: etag.clone(),
            })?;
            let moved =
                rebase_edits(&base.content, &content, edits).map_err(|e| ApiError::Conflict {
                    message: format!("{file}: {e}"),
                })?;
            final_edits.extend(moved);
            moved_any = true;
        } else {
            return Err(ApiError::EtagConflict { current_etag: etag });
        }
        current.insert(file.clone(), (content, etag));
    }

    for edit in &final_edits {
        let (content, _) = &current[&edit.file_path];
        latex_ai_core::policy::lint_edit(edit, content).map_err(unprocessable)?;
    }
    let contents: BTreeMap<String, String> = current
        .iter()
        .map(|(f, (c, _))| (f.clone(), c.clone()))
        .collect();
    let updated = apply_to_files(&contents, &final_edits).map_err(unprocessable)?;

    let changes: Vec<FileChange<'_>> = updated
        .iter()
        .map(|(file, new_content)| FileChange {
            path: Path::new(file),
            expected_etag: &current[file].1,
            new_content,
        })
        .collect();
    let outcomes = state
        .store
        .apply_file_set(&changes, &instruction, "apply-selected")
        .await?;
    state.metrics.record_edits_applied(selected.len() as u64);

    let files: Vec<AppliedFile> = outcomes
        .iter()
        .map(|o| AppliedFile {
            path: latex_ai_core::fsutil::to_slash(&o.path),
            history_id: o.history_id,
            new_etag: o.new_etag.clone(),
        })
        .collect();
    let of_primary = files.iter().find(|f| f.path == primary).or(files.first());
    Ok(Json(ApplySelectedResponse {
        success: true,
        history_id: of_primary.map_or(0, |f| f.history_id),
        new_etag: of_primary.map_or_else(|| current[&primary].1.clone(), |f| f.new_etag.clone()),
        diff: String::new(),
        files,
        rebased: moved_any,
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

    let (_, etag) = state
        .store
        .read_with_etag(std::path::Path::new(&req.file_path))
        .await?;

    // A proposal may span several files; each hunk is shown against the file
    // it is for. (Reading only `file_path` made every other file's hunks
    // report "does not apply".)
    let primary = relative(
        &state,
        &validate_path(&state.workspace_root, &req.file_path, AccessKind::Read)?,
        &req.file_path,
    );
    let mut contents: BTreeMap<String, Option<String>> = BTreeMap::new();
    let mut targets: Vec<(&AgentEdit, String)> = Vec::new();
    for edit in &req.edits {
        let file = if edit.file_path.trim().is_empty() {
            primary.clone()
        } else {
            match validate_path(&state.workspace_root, &edit.file_path, AccessKind::Read) {
                Ok(abs) => relative(&state, &abs, &edit.file_path),
                // A file that does not exist is that hunk's problem to report,
                // not a reason to refuse the whole preview. Anything else
                // (traversal, a secret) still is.
                Err(latex_ai_core::security::SecurityError::NotFound) => {
                    edit.file_path.trim_start_matches("./").to_owned()
                }
                Err(e) => return Err(e.into()),
            }
        };
        if !contents.contains_key(&file) {
            let read = state
                .store
                .read_with_etag(Path::new(&file))
                .await
                .ok()
                .map(|(c, _)| c);
            contents.insert(file.clone(), read);
        }
        targets.push((edit, file));
    }

    let unique = latex_ai_core::compile::dedupe_hunks(&req.edits.iter().collect::<Vec<_>>());
    let hunks = unique
        .into_iter()
        .map(|edit| {
            let file = targets
                .iter()
                .find(|(e, _)| std::ptr::eq(*e, edit))
                .map_or(primary.as_str(), |(_, f)| f.as_str());
            let hunk_id = edit.hunk_id().0;
            let Some(Some(content)) = contents.get(file) else {
                return HunkPreview {
                    hunk_id,
                    diff: String::new(),
                    applies: false,
                    error: Some(format!("`{file}` could not be read")),
                };
            };
            let applied = latex_ai_core::policy::lint_edit(edit, content)
                .map_err(|v| v.to_string())
                .and_then(|()| {
                    latex_ai_core::apply::apply_edit(content, edit).map_err(|e| e.to_string())
                });
            match applied {
                Ok(updated) => HunkPreview {
                    hunk_id,
                    diff: similar::TextDiff::from_lines(content, &updated)
                        .unified_diff()
                        .header(file, file)
                        .to_string(),
                    applies: true,
                    error: None,
                },
                Err(error) => HunkPreview {
                    hunk_id,
                    diff: String::new(),
                    applies: false,
                    error: Some(error),
                },
            }
        })
        .collect();

    Ok(Json(PreviewResponse { etag, hunks }))
}
