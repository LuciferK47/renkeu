//! First-run support: the welcome marker, the Ollama probe and the TeX prepare step.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::routing::get;
use latex_ai_core::config::EnvOverrides;
use latex_ai_server::bootstrap::{BootOptions, build_state};
use latex_ai_server::build_router;
use latex_ai_server::routes::setup::local_models_at;
use serde_json::Value;
use tower::ServiceExt;

async fn app() -> (axum::Router, tempfile::TempDir, tempfile::TempDir) {
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
    (build_router(state).unwrap(), home, ws)
}

async fn call(app: &axum::Router, method: &str, uri: &str) -> (StatusCode, Value) {
    let res = app
        .clone()
        .oneshot(Request::builder().method(method).uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

#[tokio::test]
async fn the_welcome_screen_is_remembered_in_the_home_folder() {
    let (app, home, _ws) = app().await;
    let (_, body) = call(&app, "GET", "/api/setup").await;
    assert_eq!(body["done"], false);

    let (s, body) = call(&app, "PUT", "/api/setup").await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(body["done"], true);
    assert!(home.path().join("first-run.done").exists());

    let (_, body) = call(&app, "GET", "/api/setup").await;
    assert_eq!(body["done"], true);
}

#[tokio::test]
async fn ollama_models_are_listed_and_a_missing_ollama_is_an_answer_not_an_error() {
    let stub = axum::Router::new().route(
        "/api/tags",
        get(|| async {
            axum::Json(serde_json::json!({
                "models": [
                    { "name": "qwen2.5:7b", "size": 4_700_000_000_u64 },
                    { "name": "llama3.2:3b", "size": 2_000_000_000_u64 }
                ]
            }))
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, stub).await.unwrap() });

    let up = local_models_at(&format!("http://{addr}")).await;
    assert!(up.running);
    let names: Vec<_> = up.models.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(names, ["llama3.2:3b", "qwen2.5:7b"]);

    // Nothing listens on port 1.
    let down = local_models_at("http://127.0.0.1:1").await;
    assert!(!down.running && down.models.is_empty());
}

#[tokio::test]
async fn preparing_tex_compiles_a_test_document() {
    let (app, _home, _ws) = app().await;
    let (s, body) = call(&app, "POST", "/api/setup/prepare").await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(body["ok"], true, "{body}");
}
