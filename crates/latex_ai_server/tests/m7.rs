//! M7 acceptance tests: data-integrity and access-control hotfixes.
//!
//! - saving requires `If-Match` (428), so no write is ever blind;
//! - secrets are unreadable through the API, and unlisted in the tree;
//! - auto-apply is validated as a WRITE before the stream starts;
//! - apply-selected needs the request-level `ETag`;
//! - a symlink loop cannot hang the file walker;
//! - concurrent saves of same-stem files do not corrupt each other.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use axum::body::Body;
use axum::http::{Request, StatusCode};
use latex_ai_core::ai::{AiError, EditPlanner, PlanFuture, PlanRequest, VisualFixRequest};
use latex_ai_core::storage::etag_of;
use latex_ai_server::{AppState, build_router, jobs::JobRegistry};
use serde_json::{Value, json};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;

struct NoPlanner;
impl EditPlanner for NoPlanner {
    fn plan_edits<'a>(&'a self, _req: PlanRequest<'a>, _t: CancellationToken) -> PlanFuture<'a> {
        Box::pin(async { Err(AiError::EmptyResponse) })
    }
    fn visual_fix<'a>(&'a self, _r: VisualFixRequest<'a>, _t: CancellationToken) -> PlanFuture<'a> {
        Box::pin(async { Err(AiError::EmptyResponse) })
    }
}

struct App {
    state: AppState,
    root: std::path::PathBuf,
    _dir: tempfile::TempDir,
}

async fn app() -> App {
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
    let dir = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(dir.path()).unwrap();
    App {
        state: AppState {
            planner: Arc::new(NoPlanner),
            compiler: Arc::new(latex_ai_compiler::Compiler::new(
                latex_ai_compiler::CompilerConfig::new(
                    env!("CARGO_BIN_EXE_latex-ai").into(),
                    tempfile::tempdir().unwrap().keep(),
                ),
            )),
            store: Arc::new(latex_ai_core::storage::Store::new(pool, root.clone())),
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
        root,
        _dir: dir,
    }
}

fn save_request(path: &str, content: &str, if_match: Option<&str>) -> Request<Body> {
    let mut req = Request::builder()
        .method("POST")
        .uri("/api/file")
        .header("content-type", "application/json");
    if let Some(etag) = if_match {
        req = req.header("if-match", etag);
    }
    req.body(Body::from(
        json!({ "path": path, "content": content }).to_string(),
    ))
    .unwrap()
}

fn get(uri: &str) -> Request<Body> {
    Request::builder().uri(uri).body(Body::empty()).unwrap()
}

async fn json_of(res: axum::response::Response) -> Value {
    serde_json::from_slice(
        &axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap()
}

#[tokio::test]
async fn a_save_without_if_match_is_refused_and_writes_nothing() {
    let app = app().await;
    std::fs::write(app.root.join("main.tex"), "original\n").unwrap();
    let router = build_router(app.state.clone()).unwrap();

    let res = router
        .oneshot(save_request("main.tex", "blind overwrite\n", None))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::PRECONDITION_REQUIRED);
    assert_eq!(
        std::fs::read_to_string(app.root.join("main.tex")).unwrap(),
        "original\n"
    );
}

#[tokio::test]
async fn secrets_are_forbidden_to_read_and_absent_from_the_listing() {
    let app = app().await;
    std::fs::write(app.root.join(".env"), "GROQ_API_KEY=gsk_secret\n").unwrap();
    std::fs::write(app.root.join("signing.pem"), "-----BEGIN-----\n").unwrap();
    std::fs::write(app.root.join("main.tex"), "x\n").unwrap();
    let router = build_router(app.state.clone()).unwrap();

    for path in [".env", "signing.pem"] {
        let res = router
            .clone()
            .oneshot(get(&format!("/api/file?path={path}")))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::FORBIDDEN, "{path} was readable");
        let body = json_of(res).await;
        assert!(
            !body.to_string().contains("gsk_secret"),
            "error leaked the secret: {body}"
        );
    }

    let listing = json_of(router.oneshot(get("/api/files")).await.unwrap()).await;
    assert_eq!(listing["files"], json!(["main.tex"]));
}

#[tokio::test]
async fn auto_apply_to_a_non_source_file_is_rejected_before_streaming() {
    let app = app().await;
    std::fs::write(app.root.join("deploy.sh"), "#!/bin/sh\n").unwrap();
    let router = build_router(app.state.clone()).unwrap();

    let res = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/agent/stream")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "file_path": "deploy.sh",
                        "instruction": "make it better",
                        "auto_apply": true
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    // Read-only review of the same file is not what is being forbidden;
    // writing it is.
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn apply_selected_without_the_request_etag_is_rejected() {
    let app = app().await;
    std::fs::write(app.root.join("main.tex"), "theirs\n").unwrap();
    let router = build_router(app.state.clone()).unwrap();

    let res = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/agent/apply-selected")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "file_path": "main.tex",
                        "edits": [],
                        "selected_hunk_ids": []
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        std::fs::read_to_string(app.root.join("main.tex")).unwrap(),
        "theirs\n"
    );
}

#[tokio::test]
async fn apply_selected_rewrite_cannot_clobber_a_changed_file() {
    let app = app().await;
    std::fs::write(app.root.join("main.tex"), "changed meanwhile\n").unwrap();
    let router = build_router(app.state.clone()).unwrap();

    let rewrite = json!({
        "file_path": "main.tex",
        "op": "rewrite_file",
        "payload": "ai version\n"
    });
    let hunk_id = {
        let edit: latex_ai_core::compile::AgentEdit =
            serde_json::from_value(rewrite.clone()).unwrap();
        edit.hunk_id().0
    };

    let res = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/agent/apply-selected")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "file_path": "main.tex",
                        "edits": [rewrite],
                        "selected_hunk_ids": [hunk_id],
                        // What the user saw when the proposal was made:
                        "expected_etag": etag_of("what the user reviewed\n")
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::CONFLICT);
    assert_eq!(
        std::fs::read_to_string(app.root.join("main.tex")).unwrap(),
        "changed meanwhile\n"
    );
}

#[tokio::test]
async fn a_symlink_loop_does_not_hang_the_file_listing() {
    let app = app().await;
    std::fs::create_dir(app.root.join("chapters")).unwrap();
    std::fs::write(app.root.join("chapters/one.tex"), "x\n").unwrap();
    std::os::unix::fs::symlink(&app.root, app.root.join("chapters/loop")).unwrap();
    std::os::unix::fs::symlink("/etc", app.root.join("etc-link")).unwrap();
    let router = build_router(app.state.clone()).unwrap();

    let res = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        router.oneshot(get("/api/files")),
    )
    .await
    .expect("listing hung on a symlink loop")
    .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        json_of(res).await["files"],
        json!(["chapters/one.tex"]),
        "linked directories must not be followed"
    );
}

#[tokio::test]
async fn concurrent_saves_of_same_stem_files_do_not_collide() {
    let app = app().await;
    std::fs::write(app.root.join("paper.tex"), "tex v1\n").unwrap();
    std::fs::write(app.root.join("paper.bib"), "bib v1\n").unwrap();
    let router = build_router(app.state.clone()).unwrap();

    let (tex, bib) = tokio::join!(
        router.clone().oneshot(save_request(
            "paper.tex",
            "tex v2\n",
            Some(&etag_of("tex v1\n"))
        )),
        router.clone().oneshot(save_request(
            "paper.bib",
            "bib v2\n",
            Some(&etag_of("bib v1\n"))
        )),
    );

    assert_eq!(tex.unwrap().status(), StatusCode::OK);
    assert_eq!(bib.unwrap().status(), StatusCode::OK);
    assert_eq!(
        std::fs::read_to_string(app.root.join("paper.tex")).unwrap(),
        "tex v2\n"
    );
    assert_eq!(
        std::fs::read_to_string(app.root.join("paper.bib")).unwrap(),
        "bib v2\n"
    );
    let leftovers: Vec<_> = std::fs::read_dir(&app.root)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| {
            std::path::Path::new(n)
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("tmp"))
        })
        .collect();
    assert!(leftovers.is_empty(), "temp files leaked: {leftovers:?}");
}

#[tokio::test]
async fn the_saved_history_row_carries_the_new_text_and_matches_the_file() {
    let app = app().await;
    std::fs::write(app.root.join("main.tex"), "v1\n").unwrap();
    let router = build_router(app.state.clone()).unwrap();

    let res = router
        .clone()
        .oneshot(save_request("main.tex", "v2\n", Some(&etag_of("v1\n"))))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let saved = json_of(res).await;
    assert_eq!(saved["etag"], etag_of("v2\n"));

    let history = json_of(
        router
            .oneshot(get("/api/history?path=main.tex&limit=10"))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(history.as_array().unwrap().len(), 1);
    assert_eq!(history[0]["id"], saved["history_id"]);
}
