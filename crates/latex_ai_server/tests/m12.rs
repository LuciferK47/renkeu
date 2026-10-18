//! M12 backend acceptance tests: the project tree, and autosave history.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use axum::body::Body;
use axum::http::{Request, StatusCode};
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

async fn app(files: &[(&str, &str)]) -> App {
    let home = tempfile::tempdir().unwrap();
    let ws = tempfile::tempdir().unwrap();
    for (name, content) in files {
        let path = ws.path().join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }
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

async fn call(app: &App, req: Request<Body>) -> (StatusCode, Value) {
    let res = app.router.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

fn save(path: &str, content: &str, etag: &str, autosave: bool) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/api/file")
        .header("content-type", "application/json")
        .header("if-match", etag)
        .body(Body::from(
            json!({"path": path, "content": content, "autosave": autosave}).to_string(),
        ))
        .unwrap()
}

#[tokio::test]
async fn the_tree_nests_files_hides_secrets_and_marks_what_is_editable() {
    let a = app(&[
        ("main.tex", "x"),
        ("chapters/one.tex", "x"),
        ("chapters/figs/plot.png", "png"),
        (".env", "SECRET"),
        (".git/config", "c"),
        ("keys/server.pem", "pem"),
    ])
    .await;

    let (status, body) = call(
        &a,
        Request::builder()
            .uri("/api/tree")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let text = body.to_string();
    for hidden in [".env", ".git", "server.pem"] {
        assert!(
            !text.contains(hidden),
            "{hidden} leaked into the tree: {text}"
        );
    }
    let tree = body["tree"].as_array().unwrap();
    assert_eq!(tree[0]["name"], "chapters");
    assert_eq!(tree[0]["kind"], "dir");
    assert_eq!(tree[0]["children"][0]["name"], "figs");
    assert_eq!(
        tree[0]["children"][0]["children"][0]["path"],
        "chapters/figs/plot.png"
    );
    assert_eq!(tree[0]["children"][0]["children"][0]["editable"], false);
    assert_eq!(tree[0]["children"][1]["path"], "chapters/one.tex");
    assert_eq!(tree[0]["children"][1]["editable"], true);
    assert_eq!(tree[1]["name"], "main.tex");
}

#[tokio::test]
async fn autosaves_fold_into_one_history_row_and_manual_saves_do_not() {
    let a = app(&[("main.tex", "v0\n")]).await;

    let mut etag = etag_of("v0\n");
    for v in ["v1\n", "v2\n", "v3\n"] {
        let (status, body) = call(&a, save("main.tex", v, &etag, true)).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        etag = body["etag"].as_str().unwrap().to_owned();
    }
    assert_eq!(
        a.state
            .store
            .recent_history(None, false, 10)
            .await
            .unwrap()
            .len(),
        1
    );

    let (status, body) = call(&a, save("main.tex", "v4\n", &etag, false)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let rows = a.state.store.recent_history(None, false, 10).await.unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].edit_type, "manual-save");
    assert_eq!(rows[1].edit_type, "autosave");
    assert!(rows[1].diff.contains("-v0") && rows[1].diff.contains("+v3"));
    assert_eq!(
        std::fs::read_to_string(a.root.join("main.tex")).unwrap(),
        "v4\n"
    );
}
