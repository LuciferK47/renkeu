//! M16 backend: reading a history entry's text, so a version can be restored.

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
    state: latex_ai_server::AppState,
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

fn get(uri: &str) -> Request<Body> {
    Request::builder().uri(uri).body(Body::empty()).unwrap()
}

#[tokio::test]
async fn an_entry_returns_the_text_it_recorded() {
    let a = app(&[("a.tex", "one\n")]).await;
    let save = Request::builder()
        .method("POST")
        .uri("/api/file")
        .header("content-type", "application/json")
        .header("if-match", etag_of("one\n"))
        .body(Body::from(
            json!({"path": "a.tex", "content": "two\n"}).to_string(),
        ))
        .unwrap();
    let (s, saved) = call(&a, save).await;
    assert_eq!(s, StatusCode::OK, "{saved}");
    let id = saved["history_id"].as_i64().unwrap();

    let (s, v) = call(&a, get(&format!("/api/history/{id}"))).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["file_path"], "a.tex");
    assert_eq!(v["text_after"], "two\n");
    assert_eq!(v["text_before"], "one\n", "a save keeps what it replaced");
    assert_eq!(v["edit_type"], "manual-save");

    let (_, list) = call(&a, get("/api/history?path=a.tex")).await;
    assert_eq!(list[0]["has_before"], true);
}

#[tokio::test]
async fn a_deleted_files_entry_keeps_what_was_removed() {
    let a = app(&[("gone.tex", "precious\n")]).await;
    let del = Request::builder()
        .method("DELETE")
        .uri("/api/file?path=gone.tex")
        .header("if-match", etag_of("precious\n"))
        .body(Body::empty())
        .unwrap();
    assert_eq!(call(&a, del).await.0, StatusCode::NO_CONTENT);
    let (_, list) = call(&a, get("/api/history?path=gone.tex")).await;
    let id = list[0]["id"].as_i64().unwrap();

    let (s, v) = call(&a, get(&format!("/api/history/{id}"))).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["edit_type"], "delete");
    assert_eq!(v["text_before"], "precious\n");
}

#[tokio::test]
async fn a_missing_entry_is_404_and_a_row_for_a_secret_is_never_served() {
    let a = app(&[]).await;
    assert_eq!(
        call(&a, get("/api/history/12345")).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(&a, get("/api/history/not-a-number")).await.0,
        StatusCode::BAD_REQUEST
    );

    // A row that somehow names a secret (written before a rule, or by hand).
    sqlx::query(
        "INSERT INTO history (file_path, command, new_text, diff, edit_type)
         VALUES ('.env', 'x', 'KEY=1', 'd', 'manual-save')",
    )
    .execute(a.state.store.pool())
    .await
    .unwrap();
    let id: i64 = sqlx::query_scalar("SELECT id FROM history WHERE file_path = '.env'")
        .fetch_one(a.state.store.pool())
        .await
        .unwrap();
    let (s, v) = call(&a, get(&format!("/api/history/{id}"))).await;
    assert_eq!(s, StatusCode::FORBIDDEN, "{v}");
    assert!(!v.to_string().contains("KEY=1"));
}
