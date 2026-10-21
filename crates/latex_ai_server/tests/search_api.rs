//! Project-wide find and replace.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use axum::body::Body;
use axum::http::{Request, StatusCode};
use latex_ai_core::config::EnvOverrides;
use latex_ai_server::bootstrap::{BootOptions, build_state};
use latex_ai_server::build_router;
use serde_json::{Value, json};
use tower::ServiceExt;

struct App {
    router: axum::Router,
    root: std::path::PathBuf,
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
        _dirs: (home, ws),
    }
}

async fn request(app: &App, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
    let mut req = Request::builder().method(method).uri(uri);
    let body = if let Some(b) = body {
        req = req.header("content-type", "application/json");
        Body::from(b.to_string())
    } else {
        Body::empty()
    };
    let res = app
        .router
        .clone()
        .oneshot(req.header("host", "localhost").body(body).unwrap())
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

const FILES: &[(&str, &str)] = &[
    ("main.tex", "\\section{Intro}\nThe Quick fox. quick quick\n"),
    ("chapters/one.tex", "Another quick line\\cite{knuth84}\n"),
    ("refs.bib", "@article{knuth84, title={Quick}}\n"),
];

#[tokio::test]
async fn search_lists_matches_by_file_with_positions() {
    let app = app(FILES).await;
    let (status, body) = request(
        &app,
        "POST",
        "/api/project/search",
        Some(json!({ "query": "quick" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["total"], 5);
    let files = body["files"].as_array().unwrap();
    let paths: Vec<_> = files.iter().map(|f| f["path"].as_str().unwrap()).collect();
    assert_eq!(paths, ["chapters/one.tex", "main.tex", "refs.bib"]);
    let first = &files[1]["hits"][0];
    assert_eq!(
        (first["line"].as_u64(), first["column"].as_u64()),
        (Some(2), Some(5))
    );

    let (_, cased) = request(
        &app,
        "POST",
        "/api/project/search",
        Some(json!({ "query": "Quick", "case_sensitive": true })),
    )
    .await;
    assert_eq!(cased["total"], 2);
}

#[tokio::test]
async fn regex_search_and_bad_input() {
    let app = app(FILES).await;
    let (_, body) = request(
        &app,
        "POST",
        "/api/project/search",
        Some(json!({ "query": "\\\\cite\\{(\\w+)\\}", "regex": true })),
    )
    .await;
    assert_eq!(body["total"], 1);
    for bad in [
        json!({ "query": "" }),
        json!({ "query": "(", "regex": true }),
    ] {
        let (status, _) = request(&app, "POST", "/api/project/search", Some(bad)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }
}

#[tokio::test]
async fn search_caps_the_listing_but_still_counts() {
    let big = "x\n".repeat(700);
    let app = app(&[("big.tex", big.as_str())]).await;
    let (_, body) = request(
        &app,
        "POST",
        "/api/project/search",
        Some(json!({ "query": "x" })),
    )
    .await;
    assert_eq!(body["truncated"], true);
    assert_eq!(body["files"][0]["hits"].as_array().unwrap().len(), 500);
    assert!(body["total"].as_u64().unwrap() > 500);
}

#[tokio::test]
async fn replace_rewrites_files_and_keeps_history() {
    let app = app(FILES).await;
    let (status, body) = request(
        &app,
        "POST",
        "/api/project/replace",
        Some(json!({ "query": "quick", "replacement": "slow", "paths": ["main.tex", "chapters/one.tex"] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["total"], 4);
    assert_eq!(
        std::fs::read_to_string(app.root.join("main.tex")).unwrap(),
        "\\section{Intro}\nThe slow fox. slow slow\n"
    );
    // A file outside `paths` is untouched.
    assert!(
        std::fs::read_to_string(app.root.join("refs.bib"))
            .unwrap()
            .contains("Quick")
    );
    let (_, history) = request(&app, "GET", "/api/history?file=main.tex", None).await;
    assert!(history.to_string().contains("replace"), "{history}");
}

#[tokio::test]
async fn a_literal_replacement_is_not_a_template() {
    let app = app(&[("a.tex", "cost\n")]).await;
    let (_, body) = request(
        &app,
        "POST",
        "/api/project/replace",
        Some(json!({ "query": "cost", "replacement": "$1 \\$" })),
    )
    .await;
    assert_eq!(body["total"], 1);
    assert_eq!(
        std::fs::read_to_string(app.root.join("a.tex")).unwrap(),
        "$1 \\$\n"
    );
}
