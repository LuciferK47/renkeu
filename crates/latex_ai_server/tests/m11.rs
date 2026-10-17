//! M11 acceptance tests: the verify-first agent loop and its proposals.
//!
//! - a second attempt builds on the first, and sees what went wrong;
//! - every kind of failed attempt counts, and exactly one `done` ends a run;
//! - auto-apply writes all files or none; review offers original-numbered
//!   hunks that can be applied in part, rebased, or refused when stale;
//! - the working copy is removed however the run ends.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use axum::body::Body;
use axum::http::{Request, StatusCode};
use latex_ai_core::ai::{AiError, EditPlanner, PlanFuture, PlanRequest, VisualFixRequest};
use latex_ai_core::compile::AgentEdit;
use latex_ai_core::config::EnvOverrides;
use latex_ai_core::storage::etag_of;
use latex_ai_server::bootstrap::{BootOptions, build_state};
use latex_ai_server::{AppState, build_router};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;

static ONE_AT_A_TIME: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

// ---------------------------------------------------------------- planners

/// What the planner was shown on one call.
#[derive(Debug, Clone)]
struct Seen {
    files: Vec<(String, String)>,
    feedback: String,
}

type Step = Box<dyn FnOnce() -> Result<Vec<AgentEdit>, AiError> + Send>;

/// Replays one scripted reply per call and records what each call was shown.
struct Scripted {
    steps: Mutex<Vec<Step>>,
    seen: Arc<Mutex<Vec<Seen>>>,
}

impl Scripted {
    fn new(steps: Vec<Step>) -> (Self, Arc<Mutex<Vec<Seen>>>) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        (Self { steps: Mutex::new(steps), seen: seen.clone() }, seen)
    }
}

impl EditPlanner for Scripted {
    fn plan_edits<'a>(&'a self, req: PlanRequest<'a>, _cancel: CancellationToken) -> PlanFuture<'a> {
        self.seen.lock().unwrap().push(Seen {
            files: req.files.iter().map(|f| (f.path.to_owned(), f.content.to_owned())).collect(),
            feedback: req.feedback.to_owned(),
        });
        let next = {
            let mut steps = self.steps.lock().unwrap();
            assert!(!steps.is_empty(), "planner called more often than scripted");
            steps.remove(0)
        };
        Box::pin(async move { next() })
    }

    fn visual_fix<'a>(&'a self, _r: VisualFixRequest<'a>, _c: CancellationToken) -> PlanFuture<'a> {
        Box::pin(async { Err(AiError::EmptyResponse) })
    }
}

fn reply(edits: Vec<AgentEdit>) -> Step {
    Box::new(move || Ok(edits))
}

fn fail_with(e: AiError) -> Step {
    Box::new(move || Err(e))
}

fn edit(v: Value) -> AgentEdit {
    serde_json::from_value(v).unwrap()
}

fn replace(file: &str, text: &str, with: &str) -> AgentEdit {
    edit(json!({"file_path": file, "op": "replace_range", "text": text, "insert_text": with}))
}

// ---------------------------------------------------------------- fixtures

struct App {
    state: AppState,
    router: axum::Router,
    root: PathBuf,
    home: PathBuf,
    _dirs: (tempfile::TempDir, tempfile::TempDir),
}

async fn app(planner: impl EditPlanner + 'static, files: &[(&str, &str)], config: Option<&str>) -> App {
    let home = tempfile::tempdir().unwrap();
    let ws = tempfile::tempdir().unwrap();
    if let Some(config) = config {
        std::fs::write(home.path().join("config.toml"), config).unwrap();
    }
    for (name, content) in files {
        let path = ws.path().join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }
    let mut state = build_state(BootOptions {
        workspace_root: ws.path().to_owned(),
        home: home.path().to_owned(),
        env: EnvOverrides::default(),
        worker_exe: Some(env!("CARGO_BIN_EXE_latex-ai").into()),
    })
    .await
    .unwrap();
    state.planner = Arc::new(planner);
    App {
        router: build_router(state.clone()).unwrap(),
        root: state.workspace_root.clone(),
        home: home.path().to_owned(),
        state,
        _dirs: (home, ws),
    }
}

async fn post(app: &App, uri: &str, body: &Value) -> (StatusCode, String) {
    let res = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = res.status();
    let bytes = tokio::time::timeout(Duration::from_secs(180), axum::body::to_bytes(res.into_body(), usize::MAX))
        .await
        .expect("the stream did not end")
        .unwrap();
    (status, String::from_utf8(bytes.to_vec()).unwrap())
}

type Events = Vec<(String, Value)>;

fn parse_sse(text: &str) -> Events {
    let mut events = Vec::new();
    let mut name = "message".to_owned();
    for line in text.lines() {
        if let Some(n) = line.strip_prefix("event: ") {
            n.trim().clone_into(&mut name);
        } else if let Some(d) = line.strip_prefix("data: ") {
            events.push((name.clone(), serde_json::from_str(d).unwrap()));
        }
    }
    events
}

async fn run(app: &App, body: Value) -> Events {
    let (status, text) = post(app, "/api/agent/stream", &body).await;
    assert_eq!(status, StatusCode::OK, "{text}");
    let events = parse_sse(&text);
    if std::env::var_os("M11_DEBUG").is_some() {
        for (n, v) in &events {
            eprintln!("EVENT {n} {v}");
        }
    }
    // The contract for every run: exactly one `done`, and it is last.
    assert_eq!(events.iter().filter(|(n, _)| n == "done").count(), 1, "{:?}", names(&events));
    assert_eq!(events.last().unwrap().0, "done", "{:?}", names(&events));
    events
}

fn names(events: &Events) -> Vec<&str> {
    events.iter().map(|(n, _)| n.as_str()).collect()
}

fn auto(file: &str, instruction: &str) -> Value {
    json!({"file_path": file, "instruction": instruction, "auto_apply": true, "max_iterations": 3})
}

fn read(app: &App, file: &str) -> String {
    std::fs::read_to_string(app.root.join(file)).unwrap()
}

fn scratch_dirs(app: &App) -> usize {
    std::fs::read_dir(app.home.join("scratch")).map_or(0, std::iter::Iterator::count)
}

const BROKEN: &str = "\\documentclass{article}\n\\begin{document}\n\\textbf{bold\n\\end{document}\n";
const GOOD: &str = "\\documentclass{article}\n\\begin{document}\nhello\n\\end{document}\n";

// ------------------------------------------------------------------- tests

#[tokio::test]
async fn the_second_iteration_edits_apply_on_top_of_the_first() {
    let _g = ONE_AT_A_TIME.lock().await;
    let (planner, seen) = Scripted::new(vec![
        // Attempt 1 changes a word but leaves the group open: still broken.
        reply(vec![replace("main.tex", "bold", "BOLD")]),
        // Attempt 2 anchors on text that exists only because attempt 1 ran.
        reply(vec![replace("main.tex", "\\textbf{BOLD", "\\textbf{BOLD}")]),
    ]);
    let a = app(planner, &[("main.tex", BROKEN)], None).await;

    let events = run(&a, auto("main.tex", "close the bold group")).await;

    assert_eq!(events.last().unwrap().1["success"], true, "{:?}", names(&events));
    assert_eq!(events.last().unwrap().1["iterations"], 2);
    assert!(read(&a, "main.tex").contains("\\textbf{BOLD}"), "{}", read(&a, "main.tex"));

    let seen = seen.lock().unwrap();
    assert!(!seen[0].files[0].1.contains("BOLD"), "attempt 1 sees the original");
    assert!(seen[1].files[0].1.contains("\\textbf{BOLD"), "attempt 2 sees attempt 1's result");
    assert!(seen[0].feedback.is_empty());
    assert!(
        seen[1].feedback.contains("failed to compile") && seen[1].feedback.contains("main.tex"),
        "attempt 2 is told what the compiler said: {:?}",
        seen[1].feedback
    );
    assert_eq!(scratch_dirs(&a), 0, "the working copy is removed on success");
}

#[tokio::test]
async fn an_edit_that_does_not_apply_is_a_failed_iteration_with_feedback() {
    let _g = ONE_AT_A_TIME.lock().await;
    let (planner, seen) = Scripted::new(vec![
        reply(vec![replace("main.tex", "text that is not in the file", "x")]),
        reply(vec![replace("main.tex", "\\textbf{bold", "\\textbf{bold}")]),
    ]);
    let a = app(planner, &[("main.tex", BROKEN)], None).await;

    let events = run(&a, auto("main.tex", "fix")).await;

    let compiled: Vec<&Value> = events.iter().filter(|(n, _)| n == "compiled").map(|(_, v)| v).collect();
    assert_eq!(compiled.len(), 2);
    assert_eq!(compiled[0]["success"], false);
    assert!(compiled[0]["error_summary"].as_str().unwrap().contains("failed to apply"), "{}", compiled[0]);
    assert_eq!(compiled[1]["success"], true);
    assert!(seen.lock().unwrap()[1].feedback.contains("failed to apply"));
    assert_eq!(events.last().unwrap().1["iterations"], 2);
}

#[tokio::test]
async fn an_unsafe_edit_is_refused_and_the_planner_is_told_so() {
    let _g = ONE_AT_A_TIME.lock().await;
    let (planner, seen) = Scripted::new(vec![
        reply(vec![edit(json!({"file_path": "main.tex", "op": "insert", "line": 1, "insert_text": "\\input{/etc/passwd}"}))]),
        reply(vec![replace("main.tex", "\\textbf{bold", "\\textbf{bold}")]),
    ]);
    let a = app(planner, &[("main.tex", BROKEN)], None).await;

    let events = run(&a, auto("main.tex", "fix")).await;

    assert_eq!(events.last().unwrap().1["success"], true);
    assert!(seen.lock().unwrap()[1].feedback.contains("unsafe edit"), "{:?}", seen.lock().unwrap()[1].feedback);
    assert!(!read(&a, "main.tex").contains("/etc/passwd"));
}

#[tokio::test]
async fn a_compile_that_times_out_is_a_failed_iteration_too() {
    let _g = ONE_AT_A_TIME.lock().await;
    let (planner, seen) = Scripted::new(vec![
        reply(vec![replace("main.tex", "hello", "\\def\\a{\\a}\\a")]),
        reply(vec![replace("main.tex", "\\def\\a{\\a}\\a", "hello again")]),
    ]);
    let a = app(planner, &[("main.tex", GOOD)], Some("[compile]\ntimeout_secs = 10\n")).await;

    let events = run(&a, auto("main.tex", "change it")).await;

    let compiled: Vec<&Value> = events.iter().filter(|(n, _)| n == "compiled").map(|(_, v)| v).collect();
    assert_eq!(compiled[0]["success"], false);
    assert!(compiled[0]["error_summary"].as_str().unwrap().contains("timed out after 10s"), "{}", compiled[0]);
    assert!(seen.lock().unwrap()[1].feedback.contains("timed out"));
    assert_eq!(events.last().unwrap().1["success"], true);
    assert!(read(&a, "main.tex").contains("hello again"));
}

#[tokio::test]
async fn running_out_of_iterations_ends_with_one_done_and_leaves_the_file() {
    let _g = ONE_AT_A_TIME.lock().await;
    let steps = (0..3).map(|i| reply(vec![replace("main.tex", "\\end{document}", &format!("% try {i}\n\\end{{document}}"))])).collect();
    let (planner, _) = Scripted::new(steps);
    let a = app(planner, &[("main.tex", BROKEN)], None).await;

    let events = run(&a, auto("main.tex", "fix")).await;

    assert_eq!(events.last().unwrap().1["message"], "Max iterations reached");
    assert_eq!(events.last().unwrap().1["success"], false);
    assert_eq!(read(&a, "main.tex"), BROKEN);
    assert_eq!(scratch_dirs(&a), 0);
}

#[tokio::test]
async fn a_planner_failure_is_an_error_then_the_single_done() {
    let _g = ONE_AT_A_TIME.lock().await;
    for failure in [AiError::Request("provider unavailable".into()), AiError::Timeout { secs: 120 }, AiError::EmptyResponse] {
        let (planner, _) = Scripted::new(vec![fail_with(failure)]);
        let a = app(planner, &[("main.tex", GOOD)], None).await;
        let events = run(&a, auto("main.tex", "x")).await;

        let n = names(&events);
        assert_eq!(&n[n.len() - 2..], ["error", "done"], "{n:?}");
        assert_eq!(events.last().unwrap().1["success"], false);
        assert_eq!(scratch_dirs(&a), 0);
    }
}

#[tokio::test]
async fn auto_apply_writes_every_changed_file_with_its_own_history_row() {
    let _g = ONE_AT_A_TIME.lock().await;
    let main = "\\documentclass{article}\n\\begin{document}\n\\input{ch}\n\\end{document}\n";
    let (planner, _) = Scripted::new(vec![reply(vec![
        replace("ch.tex", "broken \\oops", "fixed"),
        replace("main.tex", "\\begin{document}", "\\begin{document}\n% touched"),
    ])]);
    let a = app(planner, &[("main.tex", main), ("ch.tex", "broken \\oops\n")], None).await;

    let events = run(&a, auto("main.tex", "repair the chapter")).await;

    assert_eq!(events.last().unwrap().1["success"], true, "{:?}", events.last());
    assert_eq!(read(&a, "ch.tex"), "fixed\n");
    assert!(read(&a, "main.tex").contains("% touched"));
    let files = events.last().unwrap().1["files"].as_array().unwrap();
    assert_eq!(files.len(), 2);
    let history = a.state.store.recent_history(None, true, 10).await.unwrap();
    assert_eq!(history.len(), 2);
    assert!(history.iter().all(|h| h.edit_type == "agent-apply"));
    assert!(history.iter().all(|h| h.command == "repair the chapter"), "the instruction is recorded");
}

#[tokio::test]
async fn auto_apply_is_all_or_nothing_when_a_file_moves_during_the_run() {
    let _g = ONE_AT_A_TIME.lock().await;
    let main = "\\documentclass{article}\n\\begin{document}\n\\input{ch}\n\\end{document}\n";
    let root_for_step = Arc::new(Mutex::new(None::<PathBuf>));
    let step_root = root_for_step.clone();
    let (planner, _) = Scripted::new(vec![Box::new(move || {
        // The user saves ch.tex while the AI is still thinking.
        let root = step_root.lock().unwrap().clone().unwrap();
        std::fs::write(root.join("ch.tex"), "the user's newer text\n").unwrap();
        Ok(vec![
            replace("ch.tex", "ch text", "ai text"),
            replace("main.tex", "\\begin{document}", "\\begin{document}\n% ai"),
        ])
    })]);
    let a = app(planner, &[("main.tex", main), ("ch.tex", "ch text\n")], None).await;
    *root_for_step.lock().unwrap() = Some(a.root.clone());

    let events = run(&a, auto("main.tex", "x")).await;

    let n = names(&events);
    assert_eq!(&n[n.len() - 2..], ["error", "done"], "{n:?}");
    assert!(events.last().unwrap().1["message"].as_str().unwrap().contains("nothing was applied"));
    assert_eq!(read(&a, "main.tex"), main, "main.tex must not be written when ch.tex conflicted");
    assert_eq!(read(&a, "ch.tex"), "the user's newer text\n");
    assert!(a.state.store.recent_history(None, false, 10).await.unwrap().is_empty());
}

#[tokio::test]
async fn review_mode_offers_the_net_change_as_original_numbered_hunks() {
    let _g = ONE_AT_A_TIME.lock().await;
    let original = "\\documentclass{article}\n\\begin{document}\nintro line\nmiddle\n\\textbf{bold\nlast line\n\\end{document}\n";
    let (planner, _) = Scripted::new(vec![
        // Attempt 1: a harmless rewording; the bold group is still open.
        reply(vec![replace("main.tex", "intro line", "INTRO line")]),
        // Attempt 2: the fix, written against attempt 1's result.
        reply(vec![replace("main.tex", "\\textbf{bold", "\\textbf{bold}")]),
    ]);
    let a = app(planner, &[("main.tex", original)], None).await;

    let events = run(&a, json!({"file_path": "main.tex", "instruction": "fix the bold"})).await;
    assert_eq!(
        names(&events),
        ["thinking", "step", "applying", "compiled", "thinking", "step", "applying", "compiled", "proposal", "awaiting_approval", "done"]
    );
    let proposal = &events.iter().find(|(n, _)| n == "proposal").unwrap().1;
    assert_eq!(proposal["verified"], true);
    assert_eq!(proposal["iterations"], 2);

    // Two independent hunks against the ORIGINAL file, whatever the path
    // the iterations took.
    let edits: Vec<AgentEdit> = serde_json::from_value(proposal["edits"].clone()).unwrap();
    assert_eq!(edits.len(), 2, "{edits:#?}");
    assert_eq!(read(&a, "main.tex"), original, "review never writes");

    // Accept only the fix.
    let fix = edits.iter().find(|e| e.insert_text.as_deref().is_some_and(|t| t.contains("bold}"))).unwrap();
    let (status, body) = post(
        &a,
        "/api/agent/apply-selected",
        &json!({
            "file_path": "main.tex",
            "edits": edits,
            "selected_hunk_ids": [fix.hunk_id().0],
            "expected_etag": proposal["etag"],
            "proposal_id": proposal["proposal_id"],
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let applied = read(&a, "main.tex");
    assert!(applied.contains("\\textbf{bold}") && applied.contains("intro line") && !applied.contains("INTRO"), "{applied}");

    // The instruction travelled with the proposal into the history row.
    let history = a.state.store.recent_history(None, true, 10).await.unwrap();
    assert_eq!(history[0].command, "fix the bold");
}

async fn review_proposal(a: &App, file: &str) -> Value {
    let events = run(a, json!({"file_path": file, "instruction": "make the change"})).await;
    events.iter().find(|(n, _)| n == "proposal").unwrap().1.clone()
}

fn apply_body(proposal: &Value, ids: Vec<String>, rebase: bool) -> Value {
    json!({
        "file_path": "main.tex",
        "edits": proposal["edits"],
        "selected_hunk_ids": ids,
        "expected_etag": proposal["etag"],
        "proposal_id": proposal["proposal_id"],
        "rebase": rebase,
    })
}

fn ids_of(proposal: &Value) -> Vec<String> {
    let edits: Vec<AgentEdit> = serde_json::from_value(proposal["edits"].clone()).unwrap();
    edits.iter().map(|e| e.hunk_id().0).collect()
}

const FIVE: &str = "\\documentclass{article}\n\\begin{document}\nalpha\nbeta\ngamma\n\\end{document}\n";

#[tokio::test]
async fn a_stale_proposal_is_409_unless_it_can_be_rebased() {
    let _g = ONE_AT_A_TIME.lock().await;
    let (planner, _) = Scripted::new(vec![reply(vec![replace("main.tex", "gamma", "GAMMA")])]);
    let a = app(planner, &[("main.tex", FIVE)], None).await;
    let proposal = review_proposal(&a, "main.tex").await;

    // The user adds a line at the top while reviewing.
    let edited = FIVE.replace("\\begin{document}\n", "\\begin{document}\n% added by the user\n");
    std::fs::write(a.root.join("main.tex"), &edited).unwrap();

    // Without rebase: refused, and the 409 says what the file is now.
    let (status, body) = post(&a, "/api/agent/apply-selected", &apply_body(&proposal, ids_of(&proposal), false)).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(serde_json::from_str::<Value>(&body).unwrap()["current_etag"], etag_of(&edited));
    assert_eq!(read(&a, "main.tex"), edited, "a refused apply writes nothing");

    // With rebase: the hunk moved down one line and applied.
    let (status, body) = post(&a, "/api/agent/apply-selected", &apply_body(&proposal, ids_of(&proposal), true)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(serde_json::from_str::<Value>(&body).unwrap()["rebased"], true);
    let now = read(&a, "main.tex");
    assert!(now.contains("% added by the user") && now.contains("GAMMA"), "{now}");
}

#[tokio::test]
async fn rebase_refuses_when_the_user_changed_the_same_line() {
    let _g = ONE_AT_A_TIME.lock().await;
    let (planner, _) = Scripted::new(vec![reply(vec![replace("main.tex", "gamma", "GAMMA")])]);
    let a = app(planner, &[("main.tex", FIVE)], None).await;
    let proposal = review_proposal(&a, "main.tex").await;

    let edited = FIVE.replace("gamma", "gamma, edited by the user");
    std::fs::write(a.root.join("main.tex"), &edited).unwrap();

    let (status, body) = post(&a, "/api/agent/apply-selected", &apply_body(&proposal, ids_of(&proposal), true)).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(read(&a, "main.tex"), edited);
}

#[tokio::test]
async fn a_multi_file_proposal_applies_whole_or_not_at_all() {
    let _g = ONE_AT_A_TIME.lock().await;
    let main = "\\documentclass{article}\n\\begin{document}\n\\input{ch}\n\\end{document}\n";
    let (planner, _) = Scripted::new(vec![reply(vec![
        replace("main.tex", "\\input{ch}", "\\input{ch}\n% main touched"),
        replace("ch.tex", "chapter", "CHAPTER"),
    ])]);
    let a = app(planner, &[("main.tex", main), ("ch.tex", "chapter\n")], None).await;
    let proposal = review_proposal(&a, "main.tex").await;
    assert_eq!(proposal["files"].as_array().unwrap().len(), 2);

    // Without the proposal id there is no base to check the other file against.
    let mut no_id = apply_body(&proposal, ids_of(&proposal), false);
    no_id.as_object_mut().unwrap().remove("proposal_id");
    let (status, body) = post(&a, "/api/agent/apply-selected", &no_id).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");

    // ch.tex moves while main.tex is untouched: nothing at all is written.
    std::fs::write(a.root.join("ch.tex"), "chapter, edited\n").unwrap();
    let (status, _) = post(&a, "/api/agent/apply-selected", &apply_body(&proposal, ids_of(&proposal), false)).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(read(&a, "main.tex"), main, "main.tex must not change when ch.tex was stale");
    assert!(a.state.store.recent_history(None, false, 10).await.unwrap().is_empty());

    // Put ch.tex back and apply for real.
    std::fs::write(a.root.join("ch.tex"), "chapter\n").unwrap();
    let (status, body) = post(&a, "/api/agent/apply-selected", &apply_body(&proposal, ids_of(&proposal), false)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(read(&a, "ch.tex"), "CHAPTER\n");
    assert!(read(&a, "main.tex").contains("% main touched"));
    assert_eq!(serde_json::from_str::<Value>(&body).unwrap()["files"].as_array().unwrap().len(), 2);
    assert_eq!(a.state.store.recent_history(None, false, 10).await.unwrap().len(), 2);
}

#[tokio::test]
async fn an_unknown_or_expired_proposal_id_is_a_404() {
    let _g = ONE_AT_A_TIME.lock().await;
    let (planner, _) = Scripted::new(vec![]);
    let a = app(planner, &[("main.tex", FIVE)], None).await;
    let e = replace("main.tex", "alpha", "ALPHA");
    let (status, _) = post(
        &a,
        "/api/agent/apply-selected",
        &json!({"file_path": "main.tex", "edits": [e], "selected_hunk_ids": [e.hunk_id().0],
                "expected_etag": etag_of(FIVE), "proposal_id": "0".repeat(32)}),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// Hangs until cancelled, then reports whether it saw the cancellation.
struct Hangs {
    cancelled: tokio::sync::mpsc::UnboundedSender<()>,
}

impl EditPlanner for Hangs {
    fn plan_edits<'a>(&'a self, _r: PlanRequest<'a>, cancel: CancellationToken) -> PlanFuture<'a> {
        Box::pin(async move {
            cancel.cancelled().await;
            let _ = self.cancelled.send(());
            Err(AiError::Cancelled)
        })
    }

    fn visual_fix<'a>(&'a self, _r: VisualFixRequest<'a>, _c: CancellationToken) -> PlanFuture<'a> {
        Box::pin(async { Err(AiError::EmptyResponse) })
    }
}

#[tokio::test]
async fn a_disconnect_stops_the_run_and_removes_the_working_copy() {
    let _g = ONE_AT_A_TIME.lock().await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let a = app(Hangs { cancelled: tx }, &[("main.tex", GOOD)], None).await;

    let res = a
        .router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/agent/stream")
                .header("content-type", "application/json")
                .body(Body::from(auto("main.tex", "x").to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let mut body = res.into_body().into_data_stream();
    use tokio_stream::StreamExt;
    let first = tokio::time::timeout(Duration::from_secs(10), body.next()).await.unwrap().unwrap().unwrap();
    assert!(String::from_utf8_lossy(&first).contains("thinking"));
    assert_eq!(scratch_dirs(&a), 1, "the working copy exists mid-run");

    let dropped = tokio::time::Instant::now();
    drop(body);
    tokio::time::timeout(Duration::from_millis(200), rx.recv()).await.expect("planner not cancelled in 200ms").unwrap();
    assert!(dropped.elapsed() < Duration::from_millis(200));

    // Give the run task a moment to unwind, then the mirror must be gone.
    for _ in 0..50 {
        if scratch_dirs(&a) == 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(scratch_dirs(&a), 0, "the working copy must not outlive a cancelled run");
    assert_eq!(read(&a, "main.tex"), GOOD);
}

#[tokio::test]
async fn an_ambiguous_project_asks_instead_of_guessing_a_root() {
    let _g = ONE_AT_A_TIME.lock().await;
    let (planner, _) = Scripted::new(vec![]);
    let a = app(
        planner,
        &[("a.tex", GOOD), ("b.tex", GOOD), ("notes/n.tex", "just text\n")],
        None,
    )
    .await;
    let events = run(&a, auto("notes/n.tex", "x")).await;
    let n = names(&events);
    assert_eq!(&n[n.len() - 2..], ["error", "done"]);
    assert!(events.last().unwrap().1["message"].as_str().unwrap().contains("a.tex, b.tex"));
}
