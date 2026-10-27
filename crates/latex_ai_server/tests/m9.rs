//! M9 acceptance tests: the edit engine as seen through the API.
//!
//! - every hunk has its own id, so selecting one applies only that one;
//! - a batch's line numbers all refer to the original file;
//! - unsafe TeX in a proposal is refused (apply and preview);
//! - CRLF files stay CRLF.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use axum::body::Body;
use axum::http::{Request, StatusCode};
use latex_ai_core::compile::AgentEdit;
use latex_ai_core::config::EnvOverrides;
use latex_ai_core::storage::etag_of;
use latex_ai_server::bootstrap::{BootOptions, build_state};
use latex_ai_server::build_router;
use serde_json::{Value, json};
use tower::ServiceExt;

struct App {
    router: axum::Router,
    root: std::path::PathBuf,
    state: latex_ai_server::AppState,
    _dirs: (tempfile::TempDir, tempfile::TempDir),
}

async fn app() -> App {
    let home = tempfile::tempdir().unwrap();
    let ws = tempfile::tempdir().unwrap();
    let state = build_state(BootOptions {
        workspace_root: ws.path().to_owned(),
        home: home.path().to_owned(),
        env: EnvOverrides::default(),
        worker_exe: Some(env!("CARGO_BIN_EXE_latex-ai").into()),
    })
    .await
    .unwrap();
    App {
        router: build_router(state.clone()).unwrap(),
        root: state.workspace_root.clone(),
        state,
        _dirs: (home, ws),
    }
}

fn edit(v: Value) -> AgentEdit {
    serde_json::from_value(v).unwrap()
}

async fn post(app: &App, uri: &str, body: Value) -> (StatusCode, Value) {
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
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn apply(
    app: &App,
    file: &str,
    content: &str,
    edits: &[AgentEdit],
    ids: &[String],
) -> (StatusCode, Value) {
    post(
        app,
        "/api/agent/apply-selected",
        json!({
            "file_path": file,
            "edits": edits,
            "selected_hunk_ids": ids,
            "expected_etag": etag_of(content),
        }),
    )
    .await
}

fn read(app: &App, file: &str) -> String {
    std::fs::read_to_string(app.root.join(file)).unwrap()
}

#[tokio::test]
async fn two_replacements_of_one_anchor_are_selected_independently() {
    let app = app().await;
    let content = "x\nx\n";
    std::fs::write(app.root.join("a.tex"), content).unwrap();
    // Before M9 both shared one id (a hash of the anchor "x"), so choosing
    // one applied the other as well.
    let first =
        edit(json!({"file_path":"a.tex","op":"replace_range","text":"x","insert_text":"A"}));
    let second =
        edit(json!({"file_path":"a.tex","op":"replace_range","text":"x","insert_text":"B"}));
    assert_ne!(first.hunk_id(), second.hunk_id());

    let (status, body) = apply(
        &app,
        "a.tex",
        content,
        &[first.clone(), second],
        &[first.hunk_id().0],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(read(&app, "a.tex"), "A\nx\n");
}

#[tokio::test]
async fn line_based_hunks_apply_one_at_a_time() {
    let app = app().await;
    let content = "one\ntwo\nthree\nfour\n";
    std::fs::write(app.root.join("a.tex"), content).unwrap();
    let top = edit(
        json!({"file_path":"a.tex","op":"replace_range","start_line":1,"end_line":1,"insert_text":"ONE"}),
    );
    let bottom = edit(
        json!({"file_path":"a.tex","op":"replace_range","start_line":4,"end_line":4,"insert_text":"FOUR"}),
    );
    assert_ne!(top.hunk_id(), bottom.hunk_id());

    let (status, body) = apply(
        &app,
        "a.tex",
        content,
        &[top, bottom.clone()],
        &[bottom.hunk_id().0],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(read(&app, "a.tex"), "one\ntwo\nthree\nFOUR\n");
}

#[tokio::test]
async fn a_batch_is_resolved_against_the_original_line_numbers() {
    let app = app().await;
    let content = "a\nb\nc\nd\ne\n";
    std::fs::write(app.root.join("a.tex"), content).unwrap();
    // The first edit adds two lines; "line 4" must still mean the original d.
    let grow = edit(
        json!({"file_path":"a.tex","op":"replace_range","start_line":2,"end_line":2,"insert_text":"B1\nB2\nB3"}),
    );
    let later = edit(
        json!({"file_path":"a.tex","op":"replace_range","start_line":4,"end_line":4,"insert_text":"D"}),
    );
    let ids = [grow.hunk_id().0, later.hunk_id().0];

    let (status, body) = apply(&app, "a.tex", content, &[grow, later], &ids).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(read(&app, "a.tex"), "a\nB1\nB2\nB3\nc\nD\ne\n");
}

#[tokio::test]
async fn overlapping_selected_hunks_are_refused_and_nothing_is_written() {
    let app = app().await;
    let content = "1\n2\n3\n4\n";
    std::fs::write(app.root.join("a.tex"), content).unwrap();
    let a = edit(
        json!({"file_path":"a.tex","op":"replace_range","start_line":2,"end_line":3,"insert_text":"X"}),
    );
    let b = edit(
        json!({"file_path":"a.tex","op":"replace_range","start_line":3,"end_line":4,"insert_text":"Y"}),
    );
    let ids = [a.hunk_id().0, b.hunk_id().0];

    let (status, body) = apply(&app, "a.tex", content, &[a, b], &ids).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(
        body["message"].as_str().unwrap().contains("overlapping"),
        "{body}"
    );
    assert_eq!(read(&app, "a.tex"), content);
}

#[tokio::test]
async fn unsafe_tex_in_a_proposal_is_refused_on_apply() {
    let app = app().await;
    let content = "hello\n";
    std::fs::write(app.root.join("a.tex"), content).unwrap();
    let bad = edit(
        json!({"file_path":"a.tex","op":"insert","line":1,"insert_text":"\\input{/etc/passwd}"}),
    );

    let (status, body) = apply(
        &app,
        "a.tex",
        content,
        std::slice::from_ref(&bad),
        &[bad.hunk_id().0],
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(
        body["message"]
            .as_str()
            .unwrap()
            .contains("inside the project"),
        "{body}"
    );
    assert_eq!(read(&app, "a.tex"), content);
    let history = app
        .state
        .store
        .recent_history(None, false, 10)
        .await
        .unwrap();
    assert!(
        history.is_empty(),
        "a refused edit must leave no history row"
    );
}

#[tokio::test]
async fn preview_flags_the_unsafe_hunk_and_still_shows_the_others() {
    let app = app().await;
    std::fs::write(app.root.join("a.tex"), "hello\n").unwrap();
    let good =
        edit(json!({"file_path":"a.tex","op":"replace_range","text":"hello","insert_text":"hi"}));
    let bad = edit(
        json!({"file_path":"a.tex","op":"insert","line":1,"insert_text":"\\immediate\\write18{id}"}),
    );

    let (status, body) = post(
        &app,
        "/api/agent/preview",
        json!({"file_path":"a.tex","edits":[good,bad]}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let hunks = body["hunks"].as_array().unwrap();
    assert_eq!(hunks.len(), 2);
    assert_eq!(hunks[0]["applies"], true);
    assert_eq!(hunks[1]["applies"], false);
    assert!(
        hunks[1]["error"].as_str().unwrap().contains("write18"),
        "{body}"
    );
}

#[tokio::test]
async fn crlf_files_stay_crlf_through_apply_selected() {
    let app = app().await;
    let content = "a\r\nb\r\nc\r\n";
    std::fs::write(app.root.join("a.tex"), content).unwrap();
    let e = edit(
        json!({"file_path":"a.tex","op":"replace_range","start_line":2,"end_line":2,"insert_text":"B1\nB2"}),
    );

    let (status, body) = apply(
        &app,
        "a.tex",
        content,
        std::slice::from_ref(&e),
        &[e.hunk_id().0],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(read(&app, "a.tex"), "a\r\nB1\r\nB2\r\nc\r\n");
}
