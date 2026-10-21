//! Submission checks over a real build: the profile list, a profile applied
//! to a project and its PDF, and the failure modes.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use axum::body::Body;
use axum::http::{Request, StatusCode};
use latex_ai_core::config::EnvOverrides;
use latex_ai_server::bootstrap::{BootOptions, build_state};
use latex_ai_server::build_router;
use serde_json::Value;
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;

const DOC: &str = "\\documentclass{article}\n\
\\author{Ada Lovelace}\n\
\\begin{document}\n\
Hello, as shown in our previous work.\n\
\\end{document}\n";

struct App {
    router: axum::Router,
    build: latex_ai_compiler::builds::Build,
    _dirs: (tempfile::TempDir, tempfile::TempDir),
}

async fn app(files: &[(&str, &str)]) -> App {
    let home = tempfile::tempdir().unwrap();
    let ws = tempfile::tempdir().unwrap();
    for (name, text) in files {
        std::fs::write(ws.path().join(name), text).unwrap();
    }
    let state = build_state(BootOptions {
        workspace_root: ws.path().to_owned(),
        home: home.path().to_owned(),
        env: EnvOverrides::default(),
        worker_exe: Some(env!("CARGO_BIN_EXE_latex-ai").into()),
    })
    .await
    .unwrap();
    let build = state
        .compiler
        .compile(
            &latex_ai_compiler::CompileJob {
                root: state.workspace_root.clone(),
                main: "main.tex".to_owned(),
                synctex: true,
                bundle: None,
                timeout: std::time::Duration::from_secs(120),
            },
            &CancellationToken::new(),
        )
        .await
        .expect("fixture compiles");
    assert!(build.manifest.success && build.manifest.has_synctex);
    App {
        router: build_router(state).unwrap(),
        build,
        _dirs: (home, ws),
    }
}

async fn get(app: &App, uri: &str) -> (StatusCode, Value) {
    let res = app
        .router
        .clone()
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
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

#[tokio::test]
async fn the_profiles_are_listed_with_their_rules_and_where_they_come_from() {
    let a = app(&[("main.tex", DOC)]).await;
    let (status, r) = get(&a, "/api/preflight/profiles").await;
    assert_eq!(status, StatusCode::OK, "{r}");
    let ids: Vec<_> = r["profiles"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["arxiv", "double-blind", "accessibility"]);
    let arxiv = &r["profiles"][0];
    assert!(
        arxiv["source"]
            .as_str()
            .unwrap()
            .starts_with("https://info.arxiv.org/")
    );
    assert!(
        arxiv["rules"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x["check"] == "bbl_present")
    );
}

#[tokio::test]
async fn a_profile_is_applied_to_the_project_and_the_build() {
    let a = app(&[
        ("main.tex", DOC),
        ("my figure.png", "x"),
        (".env", "SECRET=1"),
    ])
    .await;
    let (status, r) = get(
        &a,
        &format!("/api/preflight?build={}&profile=double-blind", a.build.id()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{r}");
    assert_eq!(r["passed"], false);
    let found = r["findings"].as_array().unwrap();
    let author = found
        .iter()
        .find(|f| f["check"] == "anonymous_authors")
        .expect("the author is named");
    assert_eq!(
        (author["file"].as_str(), author["line"].as_u64()),
        (Some("main.tex"), Some(2))
    );
    assert!(
        found
            .iter()
            .any(|f| f["check"] == "no_self_reference" && f["severity"] == "warning")
    );
    assert_eq!(r["stale"], false);

    let (_, arxiv) = get(
        &a,
        &format!("/api/preflight?build={}&profile=arxiv", a.build.id()),
    )
    .await;
    let messages: Vec<_> = arxiv["findings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["message"].as_str().unwrap())
        .collect();
    assert!(
        messages.iter().any(|m| m.contains("\"my figure.png\"")),
        "{messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains(".env is a hidden")),
        "{messages:?}"
    );
}

#[tokio::test]
async fn an_unknown_profile_or_build_is_refused_and_a_path_is_not_a_profile() {
    let a = app(&[("main.tex", DOC)]).await;
    let (status, _) = get(
        &a,
        &format!("/api/preflight?build={}&profile=nope", a.build.id()),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    // A browser can name a profile, not a file on the machine.
    let (status, _) = get(
        &a,
        &format!("/api/preflight?build={}&profile=/etc/passwd", a.build.id()),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = get(&a, "/api/preflight?build=0000&profile=arxiv").await;
    assert_ne!(status, StatusCode::OK);
}
