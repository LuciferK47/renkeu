//! Fitting a document to a page limit: the loop measures the PDF, refuses
//! tricks that buy space dishonestly, refuses edits that delete content, and
//! succeeds only when the page count is met.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use axum::body::Body;
use axum::http::{Request, StatusCode};
use latex_ai_core::ai::{AiError, EditPlanner, PlanFuture, PlanRequest};
use latex_ai_core::compile::{AgentEdit, AgentOp};
use latex_ai_server::{AppState, build_router, jobs::JobRegistry};
use serde_json::{Value, json};
use std::fmt::Write as _;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;

// ---------------------------------------------------------------- planners

/// Returns a scripted response per call; panics if called more often than
/// scripted (that would mean the loop over-iterated).
struct ScriptedPlanner {
    script: Mutex<Vec<Result<Vec<AgentEdit>, AiError>>>,
    calls: Arc<Mutex<u32>>,
}

impl ScriptedPlanner {
    fn new(script: Vec<Result<Vec<AgentEdit>, AiError>>) -> (Self, Arc<Mutex<u32>>) {
        let calls = Arc::new(Mutex::new(0));
        (
            Self {
                script: Mutex::new(script),
                calls: calls.clone(),
            },
            calls,
        )
    }
}

impl EditPlanner for ScriptedPlanner {
    fn plan_edits<'a>(
        &'a self,
        _req: PlanRequest<'a>,
        _cancel: CancellationToken,
    ) -> PlanFuture<'a> {
        *self.calls.lock().unwrap() += 1;
        let next = {
            let mut script = self.script.lock().unwrap();
            assert!(
                !script.is_empty(),
                "planner called more often than scripted"
            );
            script.remove(0)
        };
        Box::pin(async move { next })
    }

    fn visual_fix<'a>(
        &'a self,
        _req: latex_ai_core::ai::VisualFixRequest<'a>,
        _cancel: CancellationToken,
    ) -> PlanFuture<'a> {
        Box::pin(async { Err(AiError::EmptyResponse) })
    }
}

// ---------------------------------------------------------------- fixtures

struct TestApp {
    state: AppState,
    _workspace: tempfile::TempDir,
    root: std::path::PathBuf,
}

async fn test_app(planner: Arc<dyn EditPlanner>) -> TestApp {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::query(
        "CREATE TABLE history (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            file_path TEXT NOT NULL,
            command TEXT NOT NULL,
            new_text TEXT NOT NULL,
            diff TEXT NOT NULL,
            edit_type TEXT NOT NULL, base_text TEXT,
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP
        )",
    )
    .execute(&pool)
    .await
    .unwrap();

    let workspace = tempfile::tempdir().unwrap();
    let root = latex_ai_core::fsutil::canonical(workspace.path()).unwrap();

    TestApp {
        state: AppState {
            planner,
            compiler: Arc::new(latex_ai_compiler::Compiler::new(
                latex_ai_compiler::CompilerConfig::new(
                    env!("CARGO_BIN_EXE_latex-ai").into(),
                    tempfile::tempdir().unwrap().keep(),
                ),
            )),
            store: Arc::new(latex_ai_core::storage::Store::new(
                pool.clone(),
                root.clone(),
            )),
            jobs: Arc::new(JobRegistry::default()),
            proposals: Arc::new(latex_ai_server::proposals::ProposalRegistry::default()),
            workspace_root: root.clone(),
            settings: Arc::new(
                latex_ai_core::config::Settings::open(
                    tempfile::tempdir().unwrap().keep(),
                    latex_ai_core::config::EnvOverrides::default(),
                )
                .unwrap(),
            ),
            metrics: Arc::new(latex_ai_core::metrics::Metrics::new()),
        },
        _workspace: workspace,
        root,
    }
}

fn agent_edit(op: AgentOp) -> AgentEdit {
    AgentEdit {
        file_path: "main.tex".into(),
        op,
        payload: None,
        text: None,
        start_line: None,
        end_line: None,
        line: None,
        insert_text: None,
        expected_etag: None,
        hunk_id: None,
    }
}

fn post_json(uri: &str, body: &Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

/// Read a whole SSE body (the stream must terminate) and parse into
/// `(event, json)` pairs, asserting every payload parses as JSON.
async fn read_sse(res: axum::response::Response) -> Vec<(String, Value)> {
    let bytes = tokio::time::timeout(
        Duration::from_secs(120),
        axum::body::to_bytes(res.into_body(), usize::MAX),
    )
    .await
    .expect("SSE stream did not terminate in time")
    .unwrap();
    let text = String::from_utf8(bytes.to_vec()).unwrap();
    parse_sse(&text)
}

fn parse_sse(text: &str) -> Vec<(String, Value)> {
    let mut events = Vec::new();
    let mut name = "message".to_owned();
    for line in text.lines() {
        if let Some(n) = line.strip_prefix("event: ") {
            n.trim().clone_into(&mut name);
        } else if let Some(d) = line.strip_prefix("data: ") {
            let payload: Value = serde_json::from_str(d)
                .unwrap_or_else(|e| panic!("non-JSON SSE payload for `{name}`: {d:?} ({e})"));
            events.push((name.clone(), payload));
            "message".clone_into(&mut name);
        }
    }
    events
}

fn names(events: &[(String, Value)]) -> Vec<&str> {
    events.iter().map(|(n, _)| n.as_str()).collect()
}

/// A filler line per paragraph: article's letter page holds about 49 lines, so
/// 56 of them (and a heading) run onto a second page and 36 fit on one.
fn paper(fillers: usize, vspace: bool) -> String {
    let mut text = String::from("\\documentclass{article}\n\\begin{document}\n\\section{Intro}\n");
    if vspace {
        text.push_str("\\vspace{-3cm}\n");
    }
    for n in 1..=fillers {
        let _ = write!(text, "Filler sentence number {n}.\n\n");
    }
    text.push_str("\\end{document}\n");
    text
}

fn rewrite(text: String) -> Vec<AgentEdit> {
    let mut edit = agent_edit(AgentOp::RewriteFile);
    edit.payload = Some(text);
    vec![edit]
}

fn fit_request(pages: u32, max_iterations: u32) -> Value {
    json!({
        "file_path": "main.tex",
        "pages": pages,
        "max_iterations": max_iterations
    })
}

async fn run(app: &TestApp, body: &Value) -> Vec<(String, Value)> {
    let res = build_router(app.state.clone())
        .unwrap()
        .oneshot(post_json("/api/agent/stream", body))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    read_sse(res).await
}

fn summaries(events: &[(String, Value)]) -> Vec<String> {
    events
        .iter()
        .filter(|(n, _)| n == "compiled")
        .map(|(_, v)| v["error_summary"].as_str().unwrap_or("").to_owned())
        .collect()
}

#[tokio::test]
async fn the_loop_refuses_tricks_and_deleted_content_and_stops_when_the_pages_fit() {
    let original = paper(56, false);
    let script = vec![
        // Buys space with a negative \vspace: refused before any compile.
        Ok(rewrite(paper(56, true))),
        // Gets under the limit by deleting the heading: refused.
        Ok(rewrite(paper(36, false).replace("\\section{Intro}\n", ""))),
        // Honest but not enough: still two pages.
        Ok(rewrite(paper(54, false))),
        // Honest and enough.
        Ok(rewrite(paper(36, false))),
    ];
    let (planner, calls) = ScriptedPlanner::new(script);
    let app = test_app(Arc::new(planner)).await;
    std::fs::write(app.root.join("main.tex"), &original).unwrap();

    let events = run(&app, &fit_request(1, 6)).await;
    assert_eq!(*calls.lock().unwrap(), 4, "{:?}", names(&events));

    let said = summaries(&events);
    assert!(said[0].contains("negative \\vspace"), "{said:?}");
    assert!(
        said[1].contains("removed content") && said[1].contains("section heading"),
        "{said:?}"
    );
    assert!(
        said[2].contains("compiles to 2 pages; the limit is 1"),
        "{said:?}"
    );
    assert_eq!(said.len(), 4, "{said:?}");

    let proposal = &events.iter().find(|(n, _)| n == "proposal").unwrap().1;
    assert_eq!(proposal["verified"], true, "{proposal}");
    assert_eq!(
        proposal["fit"],
        json!({"limit": 1, "pages_before": 2, "pages_after": 1})
    );
    assert_eq!(events.last().unwrap().1["success"], true);
    assert_eq!(
        std::fs::read_to_string(app.root.join("main.tex")).unwrap(),
        original,
        "review mode leaves the file alone"
    );
}

#[tokio::test]
async fn a_document_that_already_fits_is_left_alone() {
    let (planner, calls) = ScriptedPlanner::new(Vec::new());
    let app = test_app(Arc::new(planner)).await;
    std::fs::write(app.root.join("main.tex"), paper(10, false)).unwrap();

    let events = run(&app, &fit_request(3, 3)).await;
    let (name, done) = events.last().unwrap();
    assert_eq!(name, "done");
    assert_eq!(done["success"], true);
    assert_eq!(done["iterations"], 0);
    assert_eq!(done["fit"]["pages_after"], 1);
    assert_eq!(*calls.lock().unwrap(), 0, "no model call is needed");
}

#[tokio::test]
async fn running_out_of_attempts_says_how_close_it_got_and_changes_nothing() {
    let script = vec![Ok(rewrite(paper(54, false))), Ok(rewrite(paper(52, false)))];
    let (planner, _) = ScriptedPlanner::new(script);
    let app = test_app(Arc::new(planner)).await;
    let original = paper(56, false);
    std::fs::write(app.root.join("main.tex"), &original).unwrap();

    let events = run(&app, &fit_request(1, 2)).await;
    let (_, done) = events.last().unwrap();
    assert_eq!(done["success"], false);
    assert!(
        done["message"]
            .as_str()
            .unwrap()
            .contains("Closest: 2 pages, limit 1"),
        "{done}"
    );
    assert_eq!(
        std::fs::read_to_string(app.root.join("main.tex")).unwrap(),
        original
    );
}

#[tokio::test]
async fn a_document_that_does_not_compile_cannot_be_fitted_yet() {
    let (planner, calls) = ScriptedPlanner::new(Vec::new());
    let app = test_app(Arc::new(planner)).await;
    std::fs::write(
        app.root.join("main.tex"),
        "\\documentclass{article}\n\\begin{document}\n\\nosuchcommand\n\\end{document}\n",
    )
    .unwrap();

    let events = run(&app, &fit_request(1, 3)).await;
    let error = events
        .iter()
        .find(|(n, _)| n == "error")
        .expect("an error event");
    assert!(
        error.1.to_string().contains("cannot be fitted"),
        "{error:?}"
    );
    assert_eq!(*calls.lock().unwrap(), 0);
}

#[tokio::test]
async fn a_page_limit_of_zero_is_refused() {
    let (planner, _) = ScriptedPlanner::new(Vec::new());
    let app = test_app(Arc::new(planner)).await;
    std::fs::write(app.root.join("main.tex"), paper(5, false)).unwrap();
    let res = build_router(app.state.clone())
        .unwrap()
        .oneshot(post_json("/api/agent/stream", &fit_request(0, 3)))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
}
