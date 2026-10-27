//! M13 acceptance tests: creating, renaming, deleting and uploading project
//! files, serving them back, and the persisted main-file choice.

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

async fn send(app: &App, req: Request<Body>) -> (StatusCode, Vec<u8>, axum::http::HeaderMap) {
    let res = app.router.clone().oneshot(req).await.unwrap();
    let (parts, body) = res.into_parts();
    let bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap();
    (parts.status, bytes.to_vec(), parts.headers)
}

async fn call(app: &App, req: Request<Body>) -> (StatusCode, Value) {
    let (status, bytes, _) = send(app, req).await;
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

fn post_json(uri: &str, body: &Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

fn create(path: &str) -> Request<Body> {
    post_json(
        "/api/files/create",
        &json!({"path": path, "content": "x\n"}),
    )
}

fn rename(from: &str, to: &str) -> Request<Body> {
    post_json("/api/files/rename", &json!({"from": from, "to": to}))
}

fn delete(path: &str, etag: Option<&str>) -> Request<Body> {
    let mut b = Request::builder()
        .method("DELETE")
        .uri(format!("/api/file?path={path}"));
    if let Some(e) = etag {
        b = b.header("if-match", e);
    }
    b.body(Body::empty()).unwrap()
}

fn upload(path: &str, bytes: Vec<u8>) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(format!("/api/upload?path={path}"))
        .body(Body::from(bytes))
        .unwrap()
}

#[tokio::test]
async fn a_nested_create_makes_the_folders_and_records_history() {
    let a = app(&[]).await;
    let (s, v) = call(&a, create("chapters/intro/one.tex")).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["path"], "chapters/intro/one.tex");
    assert_eq!(v["etag"], etag_of("x\n"));
    assert_eq!(
        std::fs::read_to_string(a.root.join("chapters/intro/one.tex")).unwrap(),
        "x\n"
    );
    let rows = a.state.store.recent_history(None, false, 10).await.unwrap();
    assert_eq!(rows[0].file_path, "chapters/intro/one.tex");

    let (s, _) = call(&a, create("chapters/intro/one.tex")).await;
    assert_eq!(
        s,
        StatusCode::CONFLICT,
        "creating over a file must not replace it"
    );
}

#[tokio::test]
async fn create_refuses_traversal_absolute_hidden_reserved_and_non_text_names() {
    let a = app(&[]).await;
    for bad in [
        "../escape.tex",
        "a/../../escape.tex",
        "/etc/cron.d/x.tex",
        ".env.tex",
        "dir/.hidden.tex",
        "CON.tex",
        "a:b.tex",
        "picture.png",
        "noext",
        "keys/server.pem",
    ] {
        let (s, v) = call(&a, create(bad)).await;
        assert!(s.is_client_error(), "{bad} -> {s} {v}");
    }
    assert!(!a.root.parent().unwrap().join("escape.tex").exists());
}

#[tokio::test]
async fn rename_moves_the_file_keeps_its_history_and_never_replaces() {
    let a = app(&[
        ("a.tex", "one\n"),
        ("b.tex", "two\n"),
        ("ch/c.tex", "three\n"),
    ])
    .await;
    // Give a.tex a history row to follow.
    let (_, bytes, _) = send(
        &a,
        Request::builder()
            .method("GET")
            .uri("/api/file?path=a.tex")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    let etag = serde_json::from_slice::<Value>(&bytes).unwrap()["etag"]
        .as_str()
        .unwrap()
        .to_owned();
    let save = Request::builder()
        .method("POST")
        .uri("/api/file")
        .header("content-type", "application/json")
        .header("if-match", etag)
        .body(Body::from(
            json!({"path": "a.tex", "content": "one!\n"}).to_string(),
        ))
        .unwrap();
    assert_eq!(call(&a, save).await.0, StatusCode::OK);

    let (s, v) = call(&a, rename("a.tex", "b.tex")).await;
    assert_eq!(s, StatusCode::CONFLICT, "{v}");
    assert_eq!(
        std::fs::read_to_string(a.root.join("b.tex")).unwrap(),
        "two\n"
    );

    let (s, v) = call(&a, rename("a.tex", "moved/z.tex")).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert!(!a.root.join("a.tex").exists());
    assert!(a.root.join("moved/z.tex").exists());
    let rows = a.state.store.recent_history(None, false, 10).await.unwrap();
    assert!(
        rows.iter().any(|r| r.file_path == "moved/z.tex"),
        "{rows:?}"
    );
    assert!(rows.iter().all(|r| r.file_path != "a.tex"));

    // A whole directory.
    let (s, _) = call(&a, rename("ch", "chapters")).await;
    assert_eq!(s, StatusCode::OK);
    assert!(a.root.join("chapters/c.tex").exists());

    for (from, to) in [
        ("b.tex", "../out.tex"),
        ("b.tex", "b.png"),
        ("b.tex", ".git/x.tex"),
        ("chapters", "chapters/inner"),
        ("missing.tex", "m.tex"),
    ] {
        let (s, v) = call(&a, rename(from, to)).await;
        assert!(s.is_client_error(), "{from} -> {to}: {s} {v}");
    }
}

#[tokio::test]
async fn delete_needs_the_etag_and_a_stale_one_removes_nothing() {
    let a = app(&[("gone.tex", "bye\n")]).await;

    let (s, _) = call(&a, delete("gone.tex", None)).await;
    assert_eq!(s, StatusCode::PRECONDITION_REQUIRED);

    let (s, v) = call(&a, delete("gone.tex", Some("stale"))).await;
    assert_eq!(s, StatusCode::CONFLICT);
    assert!(v["current_etag"].is_string());
    assert!(a.root.join("gone.tex").exists());

    let (s, _) = call(&a, delete("gone.tex", Some(&etag_of("bye\n")))).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    assert!(!a.root.join("gone.tex").exists());
    let rows = a.state.store.recent_history(None, false, 10).await.unwrap();
    assert_eq!(rows[0].edit_type, "delete");

    let (s, _) = call(&a, delete(".env", Some("x"))).await;
    assert!(s.is_client_error());
}

#[tokio::test]
async fn upload_accepts_images_refuses_executables_and_overwrites_nothing() {
    let a = app(&[("fig.png", "old")]).await;
    let png = vec![0x89, b'P', b'N', b'G', 1, 2, 3];

    let (s, v) = call(&a, upload("figs/plot.png", png.clone())).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(std::fs::read(a.root.join("figs/plot.png")).unwrap(), png);

    let (s, _) = call(&a, upload("fig.png", png.clone())).await;
    assert_eq!(s, StatusCode::CONFLICT);
    assert_eq!(std::fs::read(a.root.join("fig.png")).unwrap(), b"old");

    for bad in ["run.sh", "x.exe", "../up.png", ".env", "page.html"] {
        let (s, v) = call(&a, upload(bad, png.clone())).await;
        assert!(s.is_client_error(), "{bad}: {s} {v}");
    }
    let (s, _) = call(&a, upload("empty.png", Vec::new())).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn an_upload_over_the_cap_is_refused() {
    let a = app(&[]).await;
    let too_big = vec![0u8; latex_ai_server::routes::manage::MAX_UPLOAD_BYTES + 1];
    let (s, _) = call(&a, upload("big.png", too_big)).await;
    assert_eq!(s, StatusCode::PAYLOAD_TOO_LARGE);
    assert!(!a.root.join("big.png").exists());
}

#[tokio::test]
async fn raw_serves_images_inertly_and_refuses_secrets() {
    let a = app(&[
        ("fig.png", "PNGDATA"),
        ("logo.svg", "<svg/>"),
        (".env", "KEY=1"),
    ])
    .await;
    let get = |p: &str| {
        Request::builder()
            .uri(format!("/api/raw?path={p}"))
            .body(Body::empty())
            .unwrap()
    };

    let (s, body, h) = send(&a, get("fig.png")).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(body, b"PNGDATA");
    assert_eq!(h["content-type"], "image/png");
    assert_eq!(h["x-content-type-options"], "nosniff");

    let (_, _, h) = send(&a, get("logo.svg")).await;
    assert!(
        h["content-security-policy"]
            .to_str()
            .unwrap()
            .contains("sandbox")
    );

    assert!(send(&a, get(".env")).await.0.is_client_error());
    assert!(send(&a, get("..%2Fsecret.png")).await.0.is_client_error());
}

#[tokio::test]
async fn the_main_file_choice_persists_and_drives_compile_resolution() {
    let a = app(&[
        (
            "a.tex",
            "\\documentclass{article}\\begin{document}A\\end{document}",
        ),
        (
            "b.tex",
            "\\documentclass{article}\\begin{document}B\\end{document}",
        ),
    ])
    .await;
    let get = || {
        Request::builder()
            .uri("/api/project")
            .body(Body::empty())
            .unwrap()
    };
    let put = |main: Value| {
        Request::builder()
            .method("PUT")
            .uri("/api/project")
            .header("content-type", "application/json")
            .body(Body::from(json!({"main": main}).to_string()))
            .unwrap()
    };

    assert_eq!(call(&a, get()).await.1["main"], Value::Null);

    for bad in [
        json!("../x.tex"),
        json!("nope.tex"),
        json!(".env"),
        json!("a.bib"),
    ] {
        let (s, v) = call(&a, put(bad.clone())).await;
        assert!(s.is_client_error(), "{bad}: {s} {v}");
    }

    let (s, _) = call(&a, put(json!("b.tex"))).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(call(&a, get()).await.1["main"], "b.tex");

    // Compiling `a.tex` as the active file still builds the chosen main.
    let (s, v) = call(
        &a,
        post_json("/api/compile", &json!({"file_path": "a.tex"})),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["main_file"], "b.tex");

    // Deleting the chosen file falls back to detection instead of failing.
    std::fs::remove_file(a.root.join("b.tex")).unwrap();
    assert_eq!(call(&a, get()).await.1["main"], Value::Null);

    let (s, _) = call(&a, put(Value::Null)).await;
    assert_eq!(s, StatusCode::OK);
}
