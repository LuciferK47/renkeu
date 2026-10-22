//! M8 acceptance tests: runtime configuration, bootstrap, per-project data.
//!
//! - the API key is write-only and lands in a `0600` file;
//! - env-pinned fields answer 409; invalid or unknown fields 422;
//! - with no key, AI routes are a clean 503 before any stream opens;
//! - history lives under the app home, not in the project, and foreign keys
//!   are on for every pooled connection.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use axum::body::Body;
use axum::http::{Request, StatusCode};
use latex_ai_core::config::{ConfiguredPlanner, EnvOverrides, Settings};
use latex_ai_server::bootstrap::{BootOptions, build_state};
use latex_ai_server::build_router;
use serde_json::{Value, json};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use tower::ServiceExt;

const SECRET: &str = "gsk_m8_super_secret_value";

async fn state_in(home: &Path, ws: &Path, env: EnvOverrides) -> latex_ai_server::AppState {
    build_state(BootOptions {
        workspace_root: ws.to_owned(),
        home: home.to_owned(),
        env,
        worker_exe: Some(env!("CARGO_BIN_EXE_latex-ai").into()),
    })
    .await
    .unwrap()
}

fn req(method: &str, uri: &str, body: Option<Value>) -> Request<Body> {
    let b = Request::builder().method(method).uri(uri);
    match body {
        Some(v) => b
            .header("content-type", "application/json")
            .body(Body::from(v.to_string()))
            .unwrap(),
        None => b.body(Body::empty()).unwrap(),
    }
}

async fn call(app: &axum::Router, r: Request<Body>) -> (StatusCode, String) {
    let res = app.clone().oneshot(r).await.unwrap();
    let status = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

fn parse(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

#[tokio::test]
async fn key_is_write_only_hot_swapped_and_stored_owner_only() {
    let home = tempfile::tempdir().unwrap();
    let ws = tempfile::tempdir().unwrap();
    let state = state_in(home.path(), ws.path(), EnvOverrides::default()).await;
    let app = build_router(state).unwrap();

    let (s, body) = call(&app, req("GET", "/api/config", None)).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(parse(&body)["ai_enabled"], false);
    assert_eq!(parse(&body)["key_source"], "none");

    let (s, body) = call(
        &app,
        req("PUT", "/api/config/api-key", Some(json!({ "key": SECRET }))),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{body}");
    assert_eq!(parse(&body)["ai_enabled"], true);
    assert_eq!(parse(&body)["key_source"], "file");
    assert!(!body.contains(SECRET), "PUT response leaked the key");

    // Live without a restart, and still never echoed.
    let (_, body) = call(&app, req("GET", "/api/config", None)).await;
    assert_eq!(parse(&body)["ai_enabled"], true);
    assert!(!body.contains(SECRET), "GET leaked the key");

    #[cfg(unix)]
    {
        let mode = std::fs::metadata(home.path().join("secrets.toml"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    let (s, body) = call(&app, req("DELETE", "/api/config/api-key", None)).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(parse(&body)["ai_enabled"], false);
    assert!(!home.path().join("secrets.toml").exists());
}

#[tokio::test]
async fn patch_persists_validates_and_rejects_unknown_fields() {
    let home = tempfile::tempdir().unwrap();
    let ws = tempfile::tempdir().unwrap();
    let state = state_in(home.path(), ws.path(), EnvOverrides::default()).await;
    let app = build_router(state).unwrap();

    let (s, body) = call(
        &app,
        req(
            "PATCH",
            "/api/config",
            Some(json!({ "model": "other-model", "timeout_secs": 30 })),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{body}");
    assert_eq!(parse(&body)["model"], "other-model");
    let file = std::fs::read_to_string(home.path().join("config.toml")).unwrap();
    assert!(file.contains("other-model"), "{file}");

    let (s, _) = call(
        &app,
        req("PATCH", "/api/config", Some(json!({ "timeout_secs": 1 }))),
    )
    .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    let (s, _) = call(
        &app,
        req("PATCH", "/api/config", Some(json!({ "modle": "typo" }))),
    )
    .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    // The rejected patches changed nothing.
    let (_, body) = call(&app, req("GET", "/api/config", None)).await;
    assert_eq!(parse(&body)["timeout_secs"], 30);
}

#[tokio::test]
async fn env_pinned_settings_answer_409() {
    let home = tempfile::tempdir().unwrap();
    let ws = tempfile::tempdir().unwrap();
    let env = EnvOverrides {
        model: Some("env-model".into()),
        api_key: Some(SECRET.into()),
        ..EnvOverrides::default()
    };
    let state = state_in(home.path(), ws.path(), env).await;
    let app = build_router(state).unwrap();

    let (_, body) = call(&app, req("GET", "/api/config", None)).await;
    let v = parse(&body);
    assert_eq!(v["model"], "env-model");
    assert_eq!(v["key_source"], "env");
    assert_eq!(v["env_overridden"], json!(["model"]));
    assert!(!body.contains(SECRET));

    let (s, _) = call(
        &app,
        req("PATCH", "/api/config", Some(json!({ "model": "x" }))),
    )
    .await;
    assert_eq!(s, StatusCode::CONFLICT);
    let (s, _) = call(
        &app,
        req("PUT", "/api/config/api-key", Some(json!({ "key": "k" }))),
    )
    .await;
    assert_eq!(s, StatusCode::CONFLICT);
    let (s, _) = call(&app, req("DELETE", "/api/config/api-key", None)).await;
    assert_eq!(s, StatusCode::CONFLICT);
}

#[tokio::test]
async fn ai_routes_are_503_until_a_key_exists() {
    let home = tempfile::tempdir().unwrap();
    let ws = tempfile::tempdir().unwrap();
    std::fs::write(ws.path().join("a.tex"), "x").unwrap();
    let state = state_in(home.path(), ws.path(), EnvOverrides::default()).await;
    let app = build_router(state).unwrap();

    let body = json!({ "file_path": "a.tex", "instruction": "fix it" });
    for uri in ["/api/plan/stream", "/api/agent/stream"] {
        let (s, text) = call(&app, req("POST", uri, Some(body.clone()))).await;
        assert_eq!(s, StatusCode::SERVICE_UNAVAILABLE, "{uri}: {text}");
        assert_eq!(parse(&text)["error"], "unavailable");
    }
}

#[tokio::test]
async fn history_lives_under_home_not_in_the_project() {
    let home = tempfile::tempdir().unwrap();
    let ws = tempfile::tempdir().unwrap();
    let _state = state_in(home.path(), ws.path(), EnvOverrides::default()).await;

    assert!(
        !ws.path().join(".latex-ai.db").exists(),
        "a database must not appear inside the project"
    );
    let dbs: Vec<_> = std::fs::read_dir(home.path().join("projects"))
        .unwrap()
        .flatten()
        .map(|e| e.path().join("history.db"))
        .collect();
    assert_eq!(dbs.len(), 1);
    assert!(dbs[0].is_file());
}

#[tokio::test]
async fn foreign_keys_are_enforced_on_every_pooled_connection() {
    let home = tempfile::tempdir().unwrap();
    let ws = tempfile::tempdir().unwrap();
    let state = state_in(home.path(), ws.path(), EnvOverrides::default()).await;
    let pool = state.store.pool();

    // Hold several connections at once so the pool must open distinct ones.
    let mut held = Vec::new();
    for _ in 0..4 {
        held.push(pool.acquire().await.unwrap());
    }
    for conn in &mut held {
        let on: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
            .fetch_one(&mut **conn)
            .await
            .unwrap();
        assert_eq!(on, 1, "a pooled connection has foreign keys off");
    }
}

#[tokio::test]
async fn a_legacy_workspace_database_is_imported_with_its_history() {
    let home = tempfile::tempdir().unwrap();
    let ws = tempfile::tempdir().unwrap();
    // Build a database exactly as pre-M8 did: in the workspace, migrated.
    let legacy = ws.path().join(".latex-ai.db");
    {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .connect(&format!("sqlite://{}?mode=rwc", legacy.display()))
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO history (file_path, command, new_text, diff, edit_type) \
             VALUES ('a.tex','old run','t','d','manual')",
        )
        .execute(&pool)
        .await
        .unwrap();
        // Leave the pool open: the row may still sit in the WAL.
        std::mem::forget(pool);
    }

    let state = state_in(home.path(), ws.path(), EnvOverrides::default()).await;
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM history WHERE command = 'old run'")
        .fetch_one(state.store.pool())
        .await
        .unwrap();
    assert_eq!(n, 1, "legacy history was not carried over");
    assert!(legacy.exists(), "the original must be left in place");
}

#[tokio::test]
async fn invalid_config_file_stops_startup_naming_the_file() {
    let home = tempfile::tempdir().unwrap();
    let ws = tempfile::tempdir().unwrap();
    std::fs::write(home.path().join("config.toml"), "[ai]\nmodle = \"typo\"\n").unwrap();

    let err = build_state(BootOptions {
        workspace_root: ws.path().to_owned(),
        home: home.path().to_owned(),
        env: EnvOverrides::default(),
        worker_exe: Some(env!("CARGO_BIN_EXE_latex-ai").into()),
    })
    .await
    .err()
    .expect("startup must fail");
    let text = format!("{err:#}");
    assert!(
        text.contains("config.toml") && text.contains("modle"),
        "{text}"
    );
}

#[tokio::test]
async fn configured_planner_reports_unavailable_without_a_key() {
    let home = tempfile::tempdir().unwrap();
    let settings = std::sync::Arc::new(
        Settings::open(home.path().to_owned(), EnvOverrides::default()).unwrap(),
    );
    let planner = ConfiguredPlanner::new(settings);
    assert!(!latex_ai_core::ai::EditPlanner::is_available(&planner));
}
