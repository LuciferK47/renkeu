//! The agent loop (masterplan §6.3), verify-first.
//!
//! One run works on a private mirror of the project ([`ScratchProject`]).
//! Each iteration the planner is shown the *current* mirror and what went
//! wrong last time; its edits are applied to the mirror, which is compiled.
//! Edits therefore accumulate: attempt two corrects attempt one's output
//! instead of starting again from the original (the first design re-copied
//! the original file every iteration, so a second attempt could not build on
//! the first, and the planner was shown text the compiler never saw).
//!
//! Every way an attempt can fail — the planner's edits are refused, do not
//! apply, do not compile, or compile to nothing — is the same thing to the
//! loop: a failed iteration whose reason is fed back. When an attempt
//! compiles, the run is *verified*:
//!
//! - `auto_apply`: all changed files are written in one all-or-nothing
//!   change, each with a history row;
//! - otherwise: the net change is offered as independent per-hunk edits
//!   against the user's original files, to accept in part.

use crate::AppState;
use crate::error::ApiError;
use crate::sse::{EventSink, SseEvent};
use latex_ai_compiler::diagnostics::{Diagnostic, Severity};
use latex_ai_compiler::project::project_files;
use latex_ai_compiler::scratchpad::ScratchProject;
use latex_ai_compiler::{CompileError, CompileJob};
use latex_ai_core::ai::{PlanFile, PlanRequest};
use latex_ai_core::apply::edits_from_diff;
use latex_ai_core::compile::AgentEdit;
use latex_ai_core::policy::lint_edit;
use latex_ai_core::security::has_writable_extension;
use latex_ai_core::storage::jobs::CompileErrorRecord;
use latex_ai_core::storage::{FileChange, etag_of};
use serde_json::json;
use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

/// Keep compiler noise fed back to the model (and the UI) bounded.
const MAX_ERROR_SUMMARY_LEN: usize = 2_000;
/// Files shown to the planner at once: the one being edited plus those the
/// last compile blamed.
const MAX_SCOPE_FILES: usize = 4;
const MAX_LISTED_FILES: usize = 200;

pub(crate) struct LoopRequest {
    /// The file the user is editing, project-relative.
    pub active: String,
    pub instruction: String,
    pub auto_apply: bool,
    pub max_iterations: u8,
}

/// Stamp server-computed hunk ids so a proposal can be applied per hunk
/// through `POST /api/agent/apply-selected`.
pub(crate) fn stamp_hunk_ids(edits: &mut [AgentEdit]) {
    for edit in edits.iter_mut() {
        edit.hunk_id = Some(edit.hunk_id().0);
    }
}

/// `POST /api/plan/stream`: one planning call, no compile.
/// Emits `progress` -> `proposal` -> `done` (or `error` -> `done`).
pub(crate) async fn run_plan(
    state: AppState,
    active: String,
    instruction: String,
    cancel: CancellationToken,
    mut sink: EventSink,
) {
    let content = match tokio::fs::read_to_string(state.workspace_root.join(&active)).await {
        Ok(c) => c,
        Err(e) => return sink.fail(format!("read failed: {e}")).await,
    };
    if !sink
        .emit(SseEvent::Progress(
            json!({ "message": "Planning edits...", "progress": 0.1 }),
        ))
        .await
    {
        return;
    }

    // Counted before the call so an in-flight or failed request is still
    // visible (§12 M5 audit: integration).
    state.metrics.record_llm_call();
    let files = [PlanFile {
        path: &active,
        content: &content,
    }];
    let request = PlanRequest {
        instruction: &instruction,
        files: &files,
        other_files: &[],
        feedback: "",
    };
    let mut edits = match state.planner.plan_edits(request, cancel).await {
        Ok(edits) => edits,
        Err(e) => return sink.fail(e.to_string()).await,
    };

    if let Err(violation) = latex_ai_core::policy::lint_edits(&edits, &content) {
        return sink
            .fail(format!("The AI proposed an unsafe edit: {violation}"))
            .await;
    }
    stamp_hunk_ids(&mut edits);
    sink.emit(SseEvent::Proposal(json!({
        "summary": "AI proposed edits based on your request.",
        "edit_count": edits.len(),
        "edits": edits,
        "etag": etag_of(&content),
    })))
    .await;
    sink.emit(SseEvent::Done(json!({ "success": true }))).await;
}

/// `POST /api/agent/stream`: the verify-first loop.
// One loop whose steps share a lot of state; splitting it would mean passing
// that state around for no gain in clarity.
#[allow(clippy::too_many_lines)]
pub(crate) async fn run_loop(
    state: AppState,
    req: LoopRequest,
    cancel: CancellationToken,
    mut sink: EventSink,
) {
    state.metrics.record_agent_run();
    let LoopRequest {
        active,
        instruction: original,
        auto_apply,
        max_iterations,
    } = req;

    let Ok(initial) = tokio::fs::read_to_string(state.workspace_root.join(&active)).await else {
        return sink.fail("could not read file").await;
    };
    let main = match main_file(&state, &active).await {
        Ok(m) => m,
        Err(message) => return sink.fail(message).await,
    };

    let scratch = {
        let (root, home) = (
            state.workspace_root.clone(),
            state.settings.home().join("scratch"),
        );
        let session = uuid::Uuid::new_v4().to_string();
        match tokio::task::spawn_blocking(move || ScratchProject::create(&home, &session, &root))
            .await
        {
            Ok(Ok(s)) => s,
            Ok(Err(e)) => {
                return sink
                    .fail(format!("could not prepare a working copy: {e}"))
                    .await;
            }
            Err(e) => {
                return sink
                    .fail(format!("could not prepare a working copy: {e}"))
                    .await;
            }
        }
    };
    let other_files = listed_files(&state.workspace_root).await;

    let mut feedback = String::new();
    let mut blamed: Vec<String> = Vec::new();
    // Diagnostics from every failed iteration, attached to the history row
    // once the run succeeds (masterplan §7.1, §12 M5).
    let mut failures: Vec<CompileErrorRecord> = Vec::new();

    for iteration in 1..=u32::from(max_iterations) {
        state.metrics.record_agent_iteration();
        if !sink
            .emit(SseEvent::Thinking {
                message: "Planning edits...".to_owned(),
                iteration,
            })
            .await
        {
            return; // client gone; CancelOnDisconnect already fired the token
        }

        // What the planner sees: the mirror as it is now (earlier iterations'
        // edits included), the file being worked on first.
        let mut scope = vec![active.clone()];
        for file in &blamed {
            if !scope.contains(file) && scope.len() < MAX_SCOPE_FILES {
                scope.push(file.clone());
            }
        }
        let mut contents: Vec<(String, String)> = Vec::new();
        for path in &scope {
            match scratch.read(path).await {
                Ok(c) => contents.push((path.clone(), c)),
                // A blamed file that is not editable source (a class file the
                // error mentioned, say) is simply not shown.
                Err(e) if *path == active => {
                    return sink.fail(format!("could not read {path}: {e}")).await;
                }
                Err(_) => {}
            }
        }
        let shown: Vec<String> = contents.iter().map(|(p, _)| p.clone()).collect();
        let others: Vec<String> = other_files
            .iter()
            .filter(|f| !shown.contains(f))
            .cloned()
            .collect();
        let files: Vec<PlanFile<'_>> = contents
            .iter()
            .map(|(path, content)| PlanFile { path, content })
            .collect();

        state.metrics.record_llm_call();
        let planned = state
            .planner
            .plan_edits(
                PlanRequest {
                    instruction: &original,
                    files: &files,
                    other_files: &others,
                    feedback: &feedback,
                },
                cancel.clone(),
            )
            .await;
        let mut edits = match planned {
            Ok(edits) => edits,
            Err(latex_ai_core::ai::AiError::Cancelled) => return,
            Err(e) => return sink.fail(e.to_string()).await,
        };
        if edits.is_empty() {
            return sink.fail("no edits proposed").await;
        }
        normalise_paths(&mut edits, &shown, &other_files);

        // Refused edits are a failed attempt, not the end of the run.
        let refusal = edits.iter().find_map(|edit| {
            let current = contents
                .iter()
                .find(|(p, _)| *p == edit.file_path)
                .map_or("", |(_, c)| c.as_str());
            lint_edit(edit, current)
                .err()
                .map(|v| format!("The AI proposed an unsafe edit: {v}"))
        });
        stamp_hunk_ids(&mut edits);

        let announced = if auto_apply {
            sink.emit(SseEvent::Proposal(json!({
                "summary": "AI proposed edits based on your request.",
                "edit_count": edits.len(),
                "edits": edits,
                "iteration": iteration,
            })))
            .await
        } else {
            // Intermediate attempts are not for the client to apply; the one
            // proposal comes at the end, once something compiles.
            sink.emit(SseEvent::Step(
                json!({ "iteration": iteration, "edit_count": edits.len() }),
            ))
            .await
        };
        if !announced {
            return;
        }

        if let Some(reason) = refusal {
            if !fail_iteration(&mut sink, &mut failures, iteration, &reason).await {
                return;
            }
            feedback = format!("{reason}. Propose a different change.");
            continue;
        }

        if !sink
            .emit(SseEvent::Applying {
                message: "Applying edits to a working copy...".to_owned(),
            })
            .await
        {
            return;
        }
        if let Err(e) = scratch.apply(&edits).await {
            let reason = truncate(
                &format!("The edits failed to apply: {e}"),
                MAX_ERROR_SUMMARY_LEN,
            );
            if !fail_iteration(&mut sink, &mut failures, iteration, &reason).await {
                return;
            }
            feedback = format!(
                "{reason}\nLine numbers refer to the files exactly as shown; \"text\" must match verbatim, without line numbers."
            );
            continue;
        }

        let job = CompileJob {
            root: scratch.root().to_path_buf(),
            main: main.clone(),
            synctex: false,
            timeout: Duration::from_secs(state.settings.snapshot().config.compile.timeout_secs),
        };
        match state.compiler.compile_scratch(&job, &cancel).await {
            Ok(out) if out.success => {
                return finish(
                    &state, &mut sink, &scratch, &original, &active, &initial, iteration,
                    auto_apply, failures,
                )
                .await;
            }
            Ok(out) => {
                let summary = truncate(&describe_errors(&out.diagnostics), MAX_ERROR_SUMMARY_LEN);
                for d in out
                    .diagnostics
                    .iter()
                    .filter(|d| d.severity == Severity::Error)
                {
                    failures.push(CompileErrorRecord {
                        iteration,
                        line: d.line,
                        file: d.file.clone(),
                        message: d.message.clone(),
                        error_type: kind_name(d),
                    });
                }
                blamed = out
                    .diagnostics
                    .iter()
                    .filter(|d| d.severity == Severity::Error)
                    .filter_map(|d| d.file.clone())
                    .collect();
                if !sink
                    .emit(SseEvent::Compiled(json!({
                        "success": false,
                        "error_summary": summary,
                        "diagnostics": out.diagnostics,
                    })))
                    .await
                {
                    return;
                }
                feedback = format!("The document failed to compile:\n{summary}");
            }
            Err(CompileError::Cancelled) => return, // disconnect: token already cancelled
            Err(CompileError::Timeout { secs }) => {
                let reason = format!("compilation timed out after {secs}s (an infinite loop?)");
                if !fail_iteration(&mut sink, &mut failures, iteration, &reason).await {
                    return;
                }
                feedback = format!("The previous attempt: {reason}.");
            }
            Err(e) => return sink.fail(e.to_string()).await,
        }
    }

    sink.emit(SseEvent::Done(
        json!({ "success": false, "message": "Max iterations reached" }),
    ))
    .await;
}

/// Record a failed iteration and tell the client. `false` means it is gone.
async fn fail_iteration(
    sink: &mut EventSink,
    failures: &mut Vec<CompileErrorRecord>,
    iteration: u32,
    reason: &str,
) -> bool {
    failures.push(CompileErrorRecord::engine_failure(iteration, reason));
    sink.emit(SseEvent::Compiled(json!({
        "success": false,
        "error_summary": reason,
    })))
    .await
}

/// An attempt compiled. Write it, or offer it for review.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
async fn finish(
    state: &AppState,
    sink: &mut EventSink,
    scratch: &ScratchProject,
    instruction: &str,
    active: &str,
    initial: &str,
    iteration: u32,
    auto_apply: bool,
    failures: Vec<CompileErrorRecord>,
) {
    let changes = match scratch.changes().await {
        Ok(c) => c,
        Err(e) => return sink.fail(format!("could not read the result: {e}")).await,
    };

    if auto_apply {
        let etags: Vec<String> = changes
            .iter()
            .map(|(_, original, _)| etag_of(original))
            .collect();
        let set: Vec<FileChange<'_>> = changes
            .iter()
            .zip(&etags)
            .map(|((path, _, current), etag)| FileChange {
                path: Path::new(path),
                expected_etag: etag,
                new_content: current,
            })
            .collect();

        match state
            .store
            .apply_file_set(&set, instruction, "agent-apply")
            .await
        {
            Ok(outcomes) => {
                // The errors that were healed on the way here are part of the
                // audit trail for this change.
                if let (Some(first), false) = (outcomes.first(), failures.is_empty()) {
                    if let Err(e) = latex_ai_core::storage::jobs::record_compile_errors(
                        state.store.pool(),
                        first.history_id,
                        &failures,
                    )
                    .await
                    {
                        tracing::warn!(error = %e, "could not record compile errors");
                    }
                }
                let of_active = outcomes.iter().find(|o| o.path == Path::new(active));
                let files: Vec<_> = outcomes
                    .iter()
                    .map(|o| json!({ "path": o.path, "history_id": o.history_id, "new_etag": o.new_etag }))
                    .collect();
                if !sink
                    .emit(SseEvent::Compiled(json!({ "success": true })))
                    .await
                {
                    return;
                }
                sink.emit(SseEvent::Done(json!({
                    "success": true,
                    "history_id": of_active.or(outcomes.first()).map(|o| o.history_id),
                    "new_etag": of_active.map_or_else(|| etag_of(initial), |o| o.new_etag.clone()),
                    "iterations": iteration,
                    "files": files,
                })))
                .await;
            }
            Err(latex_ai_core::storage::StorageError::EtagConflict { .. }) => {
                sink.fail("A file changed while the AI was working, so nothing was applied. Run it again.")
                    .await;
            }
            Err(e) => sink.fail(format!("compiled but saving failed: {e}")).await,
        }
        return;
    }

    // Review: the net change as independent hunks against the user's files.
    let mut edits = Vec::new();
    let mut base = BTreeMap::new();
    let mut summary = Vec::new();
    for (path, original, current) in &changes {
        let mut file_edits = edits_from_diff(path, original, current);
        stamp_hunk_ids(&mut file_edits);
        summary.push(
            json!({ "path": path, "etag": etag_of(original), "edit_count": file_edits.len() }),
        );
        base.insert(
            path.clone(),
            crate::proposals::BaseFile {
                content: original.clone(),
                etag: etag_of(original),
            },
        );
        edits.extend(file_edits);
    }
    let active_etag = base
        .get(active)
        .map_or_else(|| etag_of(initial), |b| b.etag.clone());
    let proposal_id = state.proposals.insert(instruction.to_owned(), base);

    if !sink
        .emit(SseEvent::Compiled(json!({ "success": true })))
        .await
    {
        return;
    }
    if !sink
        .emit(SseEvent::Proposal(json!({
            "summary": "AI proposed edits that compile.",
            "proposal_id": proposal_id,
            "verified": true,
            "edit_count": edits.len(),
            "edits": edits,
            "etag": active_etag,
            "files": summary,
            "iterations": iteration,
        })))
        .await
    {
        return;
    }
    sink.emit(SseEvent::AwaitingApproval(json!({
        "message": "Review the proposal, then apply selected hunks.",
        "proposal_id": proposal_id,
    })))
    .await;
    sink.emit(SseEvent::Done(
        json!({ "success": true, "awaiting_approval": true, "iterations": iteration }),
    ))
    .await;
}

/// The file to compile for `active`: the project's root document. A project
/// with several plausible roots needs the user to say which; one with none is
/// compiled as it stands and the compiler explains what is missing.
async fn main_file(state: &AppState, active: &str) -> Result<String, String> {
    match crate::routes::compile::resolve_main(state, active, None).await {
        Ok(main) => Ok(main),
        Err(ApiError::MainFileAmbiguous { candidates }) => Err(format!(
            "cannot tell which file is the main one ({}); compile once and choose",
            candidates.join(", ")
        )),
        Err(_) => Ok(active.to_owned()),
    }
}

async fn listed_files(root: &Path) -> Vec<String> {
    let root = root.to_path_buf();
    tokio::task::spawn_blocking(move || {
        project_files(&root)
            .unwrap_or_default()
            .into_iter()
            .filter(|p| has_writable_extension(p))
            .map(|p| p.to_string_lossy().into_owned())
            .take(MAX_LISTED_FILES)
            .collect()
    })
    .await
    .unwrap_or_default()
}

/// Models write `./main.tex`. Strip that; and when exactly one file was
/// shown and the edit names something that is not a project file at all
/// (usually a basename or a made-up name), point it at the one shown. A name
/// that *is* another project file is left alone: editing it is legitimate.
fn normalise_paths(edits: &mut [AgentEdit], scope: &[String], project: &[String]) {
    for edit in edits {
        let cleaned = edit.file_path.trim().trim_start_matches("./").to_owned();
        let known = scope.contains(&cleaned) || project.contains(&cleaned);
        edit.file_path = if known || scope.len() != 1 {
            cleaned
        } else {
            scope[0].clone()
        };
    }
}

/// The errors a failed compile reported, one per line, for the model.
pub(crate) fn describe_errors(diagnostics: &[Diagnostic]) -> String {
    let lines: Vec<String> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .take(8)
        .map(|d| match &d.context {
            Some(ctx) => format!("- {} (near: {ctx})", d.headline()),
            None => format!("- {}", d.headline()),
        })
        .collect();
    if lines.is_empty() {
        "(the engine gave no details)".to_owned()
    } else {
        lines.join("\n")
    }
}

/// The diagnostic's kind as its wire name, for the audit trail.
fn kind_name(d: &Diagnostic) -> String {
    serde_json::to_value(d.kind)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_else(|| "unknown".to_owned())
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

#[cfg(test)]
mod tests {
    use super::*;
    use latex_ai_core::compile::AgentOp;

    fn edit(path: &str) -> AgentEdit {
        AgentEdit {
            file_path: path.into(),
            op: AgentOp::Insert,
            payload: None,
            text: None,
            start_line: None,
            end_line: None,
            line: None,
            insert_text: Some("x".into()),
            expected_etag: None,
            hunk_id: None,
        }
    }

    #[test]
    fn paths_are_tidied_and_a_stray_name_is_pointed_at_the_only_file() {
        let scope = ["main.tex".to_owned()];
        let project = ["main.tex".to_owned(), "ch.tex".to_owned()];
        let mut edits = vec![edit("./main.tex"), edit("made-up.tex"), edit("ch.tex")];
        normalise_paths(&mut edits, &scope, &project);
        assert_eq!(edits[0].file_path, "main.tex");
        assert_eq!(
            edits[1].file_path, "main.tex",
            "not a project file: the one shown is meant"
        );
        assert_eq!(
            edits[2].file_path, "ch.tex",
            "another project file is a real target"
        );

        // With several files shown a wrong name is left alone, so applying
        // it fails visibly instead of silently editing the wrong file.
        let mut edits = vec![edit("nope.tex")];
        normalise_paths(&mut edits, &["a.tex".to_owned(), "b.tex".to_owned()], &[]);
        assert_eq!(edits[0].file_path, "nope.tex");
    }

    #[test]
    fn truncation_respects_character_boundaries() {
        assert_eq!(truncate("short", 10), "short");
        let long = "é".repeat(20);
        let cut = truncate(&long, 5);
        assert!(cut.ends_with('…') && cut.is_char_boundary(cut.len() - '…'.len_utf8()));
    }
}
