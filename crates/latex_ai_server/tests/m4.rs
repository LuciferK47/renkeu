//! M4 acceptance tests (masterplan.md §12 M4): per-hunk accept/reject.
//!
//! - the same plan produced twice yields an identical hunk-id set;
//! - reject-all leaves the file byte-identical and writes no history;
//! - accepting 1 of 3 modifies only that hunk's range;
//! - identical hunks collapse to a single application;
//! - a stale `expected_etag` is a 409 that writes nothing;
//! - `start_line > end_line` is a 422 caught before any filesystem work;
//! - one history row per apply-selected regardless of hunk count;
//! - the per-hunk preview matches what applying actually does.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use axum::body::Body;
use axum::http::{Request, StatusCode};
use latex_ai_core::ai::{AiError, EditPlanner, PlanFuture, VisualFixRequest};
use latex_ai_core::compile::{AgentEdit, AgentOp};
use latex_ai_server::{AppState, build_router, jobs::JobRegistry};
use serde_json::{Value, json};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;

/// Three distinct, independently-applicable changes.
const DOC: &str = "\\documentclass{article}\n\
\\begin{document}\n\
alpha line\n\
beta line\n\
gamma line\n\
\\end{document}\n";

struct NoPlanner;
impl EditPlanner for NoPlanner {
    fn plan_edits<'a>(
        &'a self,
        _instruction: &'a str,
        _file_content: &'a str,
        _cancel: CancellationToken,
    ) -> PlanFuture<'a> {
        Box::pin(async { Err(AiError::EmptyResponse) })
    }
    fn visual_fix<'a>(
        &'a self,
        _req: VisualFixRequest<'a>,
        _cancel: CancellationToken,
    ) -> PlanFuture<'a> {
        Box::pin(async { Err(AiError::EmptyResponse) })
    }
}

struct TestApp {
    state: AppState,
    _workspace: tempfile::TempDir,
    root: std::path::PathBuf,
    pool: sqlx::SqlitePool,
}

async fn test_app() -> TestApp {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::query(
        "CREATE TABLE history (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            file_path TEXT NOT NULL, command TEXT NOT NULL, new_text TEXT NOT NULL,
            diff TEXT NOT NULL, edit_type TEXT NOT NULL,
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP)",
    )
    .execute(&pool)
    .await
    .unwrap();

    let workspace = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(workspace.path()).unwrap();
    std::fs::write(root.join("main.tex"), DOC).unwrap();
    let engine = Arc::new(latex_ai_compiler::Engine::new(2));

    TestApp {
        state: AppState {
            planner: Arc::new(NoPlanner),
            queue: Arc::new(latex_ai_compiler::queue::CompilationQueue::new(engine)),
            store: Arc::new(latex_ai_core::storage::Store::new(
                pool.clone(),
                root.clone(),
            )),
            jobs: Arc::new(JobRegistry::default()),
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
        pool,
    }
}

impl TestApp {
    fn content(&self) -> String {
        std::fs::read_to_string(self.root.join("main.tex")).unwrap()
    }

    async fn history_rows(&self) -> i64 {
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM history")
            .fetch_one(&self.pool)
            .await
            .unwrap()
    }
}

/// One `replace_range` hunk anchored on `anchor`.
fn hunk(anchor: &str, replacement: &str) -> AgentEdit {
    AgentEdit {
        file_path: "main.tex".into(),
        op: AgentOp::ReplaceRange,
        payload: None,
        text: Some(anchor.into()),
        start_line: None,
        end_line: None,
        line: None,
        insert_text: Some(replacement.into()),
        expected_etag: None,
        hunk_id: None,
    }
}

/// The three-hunk plan used across these tests.
fn three_hunks() -> Vec<AgentEdit> {
    vec![
        hunk("alpha line", "ALPHA LINE"),
        hunk("beta line", "BETA LINE"),
        hunk("gamma line", "GAMMA LINE"),
    ]
}

fn post(uri: &str, body: &Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

async fn body_json(res: axum::response::Response) -> Value {
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

async fn apply(
    app: &TestApp,
    edits: &[AgentEdit],
    selected: &[String],
) -> axum::response::Response {
    build_router(app.state.clone())
        .unwrap()
        .oneshot(post(
            "/api/agent/apply-selected",
            &json!({
                "file_path": "main.tex",
                "edits": edits,
                "selected_hunk_ids": selected,
                // The version the proposal was made against: the file as it
                // stands when the user clicks apply.
                "expected_etag": latex_ai_core::storage::etag_of(
                    &std::fs::read_to_string(app.root.join("main.tex")).unwrap()
                )
            }),
        ))
        .await
        .unwrap()
}

// ---------------------------------------------------------------- stability

#[tokio::test]
async fn the_same_plan_twice_yields_an_identical_hunk_id_set() {
    // This is what lets a user's selection survive a requery: ids come from
    // the anchor text, not from position or ordering.
    let first: Vec<String> = three_hunks().iter().map(|e| e.hunk_id().0).collect();
    let second: Vec<String> = three_hunks().iter().map(|e| e.hunk_id().0).collect();
    assert_eq!(first, second);

    // Reordering the plan must not change the ids either.
    let mut reordered = three_hunks();
    reordered.reverse();
    let mut reordered_ids: Vec<String> = reordered.iter().map(|e| e.hunk_id().0).collect();
    reordered_ids.reverse();
    assert_eq!(first, reordered_ids);

    // Distinct anchors must not collide.
    let unique: std::collections::HashSet<_> = first.iter().collect();
    assert_eq!(unique.len(), 3);
}

// ------------------------------------------------------------------ apply

#[tokio::test]
async fn rejecting_every_hunk_leaves_the_file_and_history_untouched() {
    let app = test_app().await;
    let before = app.content();

    let res = apply(&app, &three_hunks(), &[]).await;

    // Nothing selected is a client error, not a silent no-op write.
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    assert_eq!(app.content(), before, "file must be byte-identical");
    assert_eq!(app.history_rows().await, 0, "no history row for a no-op");
}

#[tokio::test]
async fn accepting_one_of_three_modifies_only_that_hunk() {
    let app = test_app().await;
    let edits = three_hunks();
    let chosen = edits[1].hunk_id().0.clone();

    let res = apply(&app, &edits, &[chosen]).await;
    assert_eq!(res.status(), StatusCode::OK);

    let content = app.content();
    assert!(content.contains("BETA LINE"), "the accepted hunk applied");
    assert!(
        content.contains("alpha line") && content.contains("gamma line"),
        "rejected hunks must be untouched: {content}"
    );
    assert_eq!(
        app.history_rows().await,
        1,
        "one history row per apply-selected"
    );
}

#[tokio::test]
async fn accepting_all_three_still_writes_exactly_one_history_row() {
    // masterplan §12 M4 audit (integration): one row regardless of hunk count.
    let app = test_app().await;
    let edits = three_hunks();
    let all: Vec<String> = edits.iter().map(|e| e.hunk_id().0).collect();

    let res = apply(&app, &edits, &all).await;
    assert_eq!(res.status(), StatusCode::OK);

    let content = app.content();
    for expected in ["ALPHA LINE", "BETA LINE", "GAMMA LINE"] {
        assert!(content.contains(expected), "missing {expected}: {content}");
    }
    assert_eq!(app.history_rows().await, 1);
}

#[tokio::test]
async fn identical_hunks_collapse_to_a_single_application() {
    // Two prompts proposing the same change must not double-apply. The
    // second copy would otherwise fail to find its anchor and abort the batch.
    let app = test_app().await;
    let once = hunk("alpha line", "ALPHA LINE");
    let duplicate = once.clone();
    let id = once.hunk_id().0.clone();

    let res = apply(&app, &[once, duplicate], &[id]).await;
    assert_eq!(res.status(), StatusCode::OK);

    let content = app.content();
    assert_eq!(
        content.matches("ALPHA LINE").count(),
        1,
        "collapsed hunk applied exactly once: {content}"
    );
}

#[tokio::test]
async fn a_failing_hunk_aborts_the_whole_batch() {
    // masterplan §12 M4 audit (logic): subset-apply aborts on any failure.
    let app = test_app().await;
    let before = app.content();
    let good = hunk("alpha line", "ALPHA LINE");
    let stale = hunk("text that is no longer in the file", "whatever");
    let ids = vec![good.hunk_id().0.clone(), stale.hunk_id().0.clone()];

    let res = apply(&app, &[good, stale], &ids).await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(app.content(), before, "no partial write");
    assert_eq!(app.history_rows().await, 0);
}

// ------------------------------------------------------------- concurrency

#[tokio::test]
async fn a_stale_expected_etag_is_a_409_that_writes_nothing() {
    let app = test_app().await;
    let mut edit = hunk("alpha line", "ALPHA LINE");
    edit.expected_etag = Some("etag-from-a-previous-version".into());
    let id = edit.hunk_id().0.clone();
    let before = app.content();

    let res = apply(&app, &[edit], &[id]).await;
    assert_eq!(res.status(), StatusCode::CONFLICT);

    let body = body_json(res).await;
    assert_eq!(body["error"], "etag_conflict");
    assert!(
        body["current_etag"].as_str().is_some_and(|e| !e.is_empty()),
        "the envelope must carry the winning etag so the UI can re-sync"
    );
    assert_eq!(app.content(), before);
    assert_eq!(app.history_rows().await, 0);
}

#[tokio::test]
async fn a_matching_expected_etag_applies() {
    let app = test_app().await;
    let router = build_router(app.state.clone()).unwrap();

    let res = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/file?path=main.tex")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let etag = body_json(res).await["etag"].as_str().unwrap().to_owned();

    let mut edit = hunk("alpha line", "ALPHA LINE");
    edit.expected_etag = Some(etag);
    let id = edit.hunk_id().0.clone();

    let res = apply(&app, &[edit], &[id]).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert!(app.content().contains("ALPHA LINE"));
}

// -------------------------------------------------------------- validation

#[tokio::test]
async fn an_inverted_line_range_is_422_before_any_write() {
    // masterplan §12 M4 audit (security): schema validates start_line <= end_line.
    let app = test_app().await;
    let before = app.content();

    let mut edit = hunk("alpha line", "ALPHA LINE");
    edit.start_line = std::num::NonZeroU64::new(9);
    edit.end_line = std::num::NonZeroU64::new(3);
    let id = edit.hunk_id().0.clone();

    let res = apply(&app, &[edit], &[id]).await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body = body_json(res).await;
    assert_eq!(body["error"], "unprocessable");
    assert!(
        body["message"].as_str().unwrap().contains("end_line"),
        "the message should name the problem: {body}"
    );
    assert_eq!(app.content(), before);
}

#[tokio::test]
async fn an_edit_missing_its_required_field_is_422() {
    let app = test_app().await;
    let mut edit = hunk("alpha line", "x");
    edit.op = AgentOp::RewriteFile;
    edit.payload = None; // rewrite_file without a payload
    let id = edit.hunk_id().0.clone();

    let res = apply(&app, &[edit], &[id]).await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

// ----------------------------------------------------------------- preview

#[tokio::test]
async fn preview_returns_a_diff_per_hunk_without_writing() {
    let app = test_app().await;
    let before = app.content();
    let edits = three_hunks();

    let res = build_router(app.state.clone())
        .unwrap()
        .oneshot(post(
            "/api/agent/preview",
            &json!({ "file_path": "main.tex", "edits": edits }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_json(res).await;

    assert!(body["etag"].as_str().is_some_and(|e| !e.is_empty()));
    let hunks = body["hunks"].as_array().unwrap();
    assert_eq!(hunks.len(), 3);

    for (preview, edit) in hunks.iter().zip(&edits) {
        assert_eq!(preview["hunk_id"], edit.hunk_id().0);
        assert_eq!(preview["applies"], true);
        let diff = preview["diff"].as_str().unwrap();
        assert!(
            diff.contains(&format!("-{}", edit.text.as_ref().unwrap())),
            "diff should remove the anchor: {diff}"
        );
        assert!(
            diff.contains(&format!("+{}", edit.insert_text.as_ref().unwrap())),
            "diff should add the replacement: {diff}"
        );
    }

    assert_eq!(app.content(), before, "preview must not write");
    assert_eq!(app.history_rows().await, 0);
}

#[tokio::test]
async fn preview_flags_a_hunk_that_no_longer_applies() {
    let app = test_app().await;
    let edits = vec![
        hunk("alpha line", "ALPHA LINE"),
        hunk("text that is no longer in the file", "whatever"),
    ];

    let res = build_router(app.state.clone())
        .unwrap()
        .oneshot(post(
            "/api/agent/preview",
            &json!({ "file_path": "main.tex", "edits": edits }),
        ))
        .await
        .unwrap();
    let body = body_json(res).await;
    let hunks = body["hunks"].as_array().unwrap();

    assert_eq!(hunks[0]["applies"], true);
    assert_eq!(hunks[1]["applies"], false, "stale hunk flagged, not fatal");
    assert!(hunks[1]["error"].as_str().is_some());
    assert_eq!(hunks[1]["diff"], "");
}

#[tokio::test]
async fn the_previewed_diff_is_what_applying_actually_produces() {
    // The preview and the write share core::apply; this pins that they agree,
    // which is the whole point of previewing server-side.
    let app = test_app().await;
    let edit = hunk("beta line", "BETA LINE");
    let id = edit.hunk_id().0.clone();

    let res = build_router(app.state.clone())
        .unwrap()
        .oneshot(post(
            "/api/agent/preview",
            &json!({ "file_path": "main.tex", "edits": [edit.clone()] }),
        ))
        .await
        .unwrap();
    let previewed = body_json(res).await["hunks"][0]["diff"]
        .as_str()
        .unwrap()
        .to_owned();

    let res = apply(&app, &[edit], &[id]).await;
    assert_eq!(res.status(), StatusCode::OK);
    let applied = body_json(res).await["diff"].as_str().unwrap().to_owned();

    assert_eq!(
        previewed, applied,
        "preview diverged from the real apply diff"
    );
}

#[tokio::test]
async fn preview_rejects_traversal_and_invalid_edits() {
    let app = test_app().await;
    let router = build_router(app.state.clone()).unwrap();

    let res = router
        .clone()
        .oneshot(post(
            "/api/agent/preview",
            &json!({ "file_path": "../../etc/passwd", "edits": [] }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);

    let mut bad = hunk("alpha line", "x");
    bad.start_line = std::num::NonZeroU64::new(5);
    bad.end_line = std::num::NonZeroU64::new(2);
    let res = router
        .oneshot(post(
            "/api/agent/preview",
            &json!({ "file_path": "main.tex", "edits": [bad] }),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
}
