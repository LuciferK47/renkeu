//! The template routes: list, start a project, re-render the front matter.

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
        std::fs::write(ws.path().join(name), content).unwrap();
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

fn fields() -> Value {
    json!({
        "title": "Rust & TeX",
        "authors": [{ "name": "Ada Lovelace", "affiliation": "Engine Co.", "email": "ada@example.org" }],
        "abstract": "Short.",
        "keywords": "rust"
    })
}

#[tokio::test]
async fn the_gallery_lists_every_template_with_its_fields() {
    let app = app(&[]).await;
    let (status, body) = request(&app, "GET", "/api/templates", None).await;
    assert_eq!(status, StatusCode::OK);
    let list = body["templates"].as_array().unwrap();
    assert_eq!(list.len(), 13);
    let article = list.iter().find(|t| t["id"] == "article").unwrap();
    assert_eq!(article["category"], "Papers");
    assert!(
        article["fields"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f == "abstract")
    );
}

#[tokio::test]
async fn an_empty_project_starts_from_a_template_exactly_once() {
    let app = app(&[]).await;
    let (status, body) = request(
        &app,
        "POST",
        "/api/project/template",
        Some(json!({ "template": "thesis", "fields": fields() })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["main"], "main.tex");
    let main = std::fs::read_to_string(app.root.join("main.tex")).unwrap();
    assert!(main.contains(r"\title{Rust \& TeX}"));
    assert!(app.root.join("chapters/introduction.tex").is_file());
    assert!(app.root.join("refs.bib").is_file());

    // A second template must not mix into what is there now.
    let (status, _) = request(
        &app,
        "POST",
        "/api/project/template",
        Some(json!({ "template": "article", "fields": fields() })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn a_project_with_a_tex_file_refuses_a_template_and_keeps_its_file() {
    let app = app(&[("mine.tex", "\\documentclass{article}\n")]).await;
    let (status, _) = request(
        &app,
        "POST",
        "/api/project/template",
        Some(json!({ "template": "article", "fields": fields() })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(!app.root.join("main.tex").exists());
    assert_eq!(
        std::fs::read_to_string(app.root.join("mine.tex")).unwrap(),
        "\\documentclass{article}\n"
    );
}

#[tokio::test]
async fn an_unknown_template_is_a_404_and_writes_nothing() {
    let app = app(&[]).await;
    let (status, _) = request(
        &app,
        "POST",
        "/api/project/template",
        Some(json!({ "template": "nope", "fields": {} })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(std::fs::read_dir(&app.root).unwrap().count(), 0);
}

#[tokio::test]
async fn the_front_matter_route_returns_a_marked_block() {
    let app = app(&[]).await;
    let (status, body) = request(
        &app,
        "POST",
        "/api/templates/frontmatter",
        Some(json!({ "template": "ieee-conf", "fields": fields() })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let block = body["block"].as_str().unwrap();
    assert!(block.starts_with("% renkeu:frontmatter begin template=ieee-conf fields="));
    assert!(block.ends_with("% renkeu:frontmatter end"));
    assert!(block.contains(r"\title{Rust \& TeX}"));
}

#[tokio::test]
async fn a_thumbnail_is_a_cacheable_png_and_unknown_ids_are_404() {
    let app = app(&[]).await;
    let res = app
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/templates/article/thumbnail")
                .header("host", "localhost")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers()["content-type"], "image/png");
    assert!(
        res.headers()["cache-control"]
            .to_str()
            .unwrap()
            .contains("max-age")
    );
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    assert!(bytes.starts_with(b"\x89PNG"));

    let (status, _) = request(&app, "GET", "/api/templates/nope/thumbnail", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
