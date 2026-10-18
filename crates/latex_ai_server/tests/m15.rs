//! M15 acceptance tests: `SyncTeX` both ways over a real multi-file build.

#![allow(clippy::unwrap_used, clippy::expect_used)]
// Short names for the app, a hit and its x/y read best in these small tests.
#![allow(clippy::many_single_char_names)]

use axum::body::Body;
use axum::http::{Request, StatusCode};
use latex_ai_core::config::EnvOverrides;
use latex_ai_server::bootstrap::{BootOptions, build_state};
use latex_ai_server::build_router;
use serde_json::Value;
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;

const MAIN: &str = "\\documentclass{article}\n\
\\begin{document}\n\
First paragraph in the main file.\n\
% a comment typesets nothing\n\
\\input{chapters/one}\n\
\n\
Closing paragraph.\n\
\\end{document}\n";

const CHAPTER: &str = "\\section{One}\n\
Text that lives in chapter one.\n";

struct App {
    router: axum::Router,
    root: std::path::PathBuf,
    build: latex_ai_compiler::builds::Build,
    _dirs: (tempfile::TempDir, tempfile::TempDir),
}

async fn app() -> App {
    let home = tempfile::tempdir().unwrap();
    let ws = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(ws.path().join("chapters")).unwrap();
    std::fs::write(ws.path().join("main.tex"), MAIN).unwrap();
    std::fs::write(ws.path().join("chapters/one.tex"), CHAPTER).unwrap();

    let state = build_state(BootOptions {
        workspace_root: ws.path().to_owned(),
        home: home.path().to_owned(),
        env: EnvOverrides::default(),
        worker_exe: Some(env!("CARGO_BIN_EXE_latex-ai").into()),
    })
    .await
    .unwrap();
    let root = state.workspace_root.clone();
    let build = state
        .compiler
        .compile(
            &latex_ai_compiler::CompileJob {
                root: root.clone(),
                main: "main.tex".to_owned(),
                synctex: true,
                timeout: std::time::Duration::from_secs(120),
            },
            &CancellationToken::new(),
        )
        .await
        .expect("fixture compiles");
    assert!(build.manifest.success && build.manifest.has_synctex);
    App {
        router: build_router(state).unwrap(),
        root,
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

fn forward_uri(app: &App, file: &str, line: u32) -> String {
    format!(
        "/api/synctex/forward?file={file}&line={line}&build={}",
        app.build.id()
    )
}

#[tokio::test]
async fn forward_search_finds_the_region_for_a_line_in_the_main_file_and_in_an_included_one() {
    let a = app().await;

    let (s, main) = get(&a, &forward_uri(&a, "main.tex", 3)).await;
    assert_eq!(s, StatusCode::OK, "{main}");
    assert_eq!(main["build"], a.build.id());
    assert_eq!(main["stale"], false);
    let hit = &main["hits"][0];
    assert_eq!(hit["page"], 1);
    // The region to flash is always big enough to see, whatever the raw hit.
    let region = &main["region"];
    assert_eq!(region["page"], 1);
    assert_eq!(region["line"], 3);
    assert!(region["height"].as_f64().unwrap() >= 12.0, "{region}");
    assert!(region["width"].as_f64().unwrap() >= 12.0, "{region}");
    assert!(hit["width"].as_f64().unwrap() > 10.0, "{hit}");
    assert!(hit["y"].as_f64().unwrap() > 0.0);

    let (s, chapter) = get(&a, &forward_uri(&a, "chapters/one.tex", 2)).await;
    assert_eq!(s, StatusCode::OK, "{chapter}");
    let ch = &chapter["hits"][0];
    assert_eq!(ch["line"], 2);
    // The chapter text sits below the first paragraph of the main file.
    assert!(ch["y"].as_f64().unwrap() > hit["y"].as_f64().unwrap());
}

#[tokio::test]
async fn backward_search_returns_the_included_file_and_line_it_came_from() {
    let a = app().await;
    let (_, fwd) = get(&a, &forward_uri(&a, "chapters/one.tex", 2)).await;
    let h = &fwd["hits"][0];
    let (x, y) = (
        h["x"].as_f64().unwrap() + h["width"].as_f64().unwrap() / 2.0,
        h["y"].as_f64().unwrap() + h["height"].as_f64().unwrap() / 2.0,
    );

    let (s, back) = get(
        &a,
        &format!(
            "/api/synctex/backward?build={}&page={}&x={x}&y={y}",
            a.build.id(),
            h["page"]
        ),
    )
    .await;

    assert_eq!(s, StatusCode::OK, "{back}");
    assert_eq!(back["file"], "chapters/one.tex", "{back}");
    assert_eq!(back["line"], 2);
    assert_eq!(back["exact"], true);
    assert_eq!(back["stale"], false);
}

#[tokio::test]
async fn a_click_in_the_main_text_maps_to_the_main_file() {
    let a = app().await;
    let (_, fwd) = get(&a, &forward_uri(&a, "main.tex", 7)).await;
    let h = &fwd["hits"][0];
    let (s, back) = get(
        &a,
        &format!(
            "/api/synctex/backward?build={}&page=1&x={}&y={}",
            a.build.id(),
            h["x"].as_f64().unwrap() + 5.0,
            h["y"].as_f64().unwrap() + h["height"].as_f64().unwrap() / 2.0
        ),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{back}");
    assert_eq!(back["file"], "main.tex");
    assert_eq!(back["line"], 7);
}

#[tokio::test]
async fn a_changed_project_is_reported_as_stale_not_refused() {
    let a = app().await;
    std::fs::write(a.root.join("main.tex"), format!("% moved\n{MAIN}")).unwrap();

    let (s, v) = get(&a, &forward_uri(&a, "main.tex", 3)).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["stale"], true);
    assert_ne!(v["hits"].as_array().unwrap().len(), 0);
    assert!(v["region"]["height"].as_f64().unwrap() >= 12.0, "{v}");

    let (s, v) = get(
        &a,
        &format!(
            "/api/synctex/backward?build={}&page=1&x=100&y=100",
            a.build.id()
        ),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["stale"], true);
}

#[tokio::test]
async fn a_line_that_typeset_nothing_falls_to_the_next_one_that_did() {
    let a = app().await;
    // Line 4 of main.tex is a comment; the answer is the following content.
    let (s, v) = get(&a, &forward_uri(&a, "main.tex", 4)).await;
    assert_eq!(s, StatusCode::OK);
    assert!(v["hits"][0]["line"].as_u64().unwrap() > 4, "{v}");
}

#[tokio::test]
async fn bad_requests_are_refused_with_the_right_status() {
    let a = app().await;
    std::fs::write(a.root.join(".env"), "SECRET=1").unwrap();
    let id = a.build.id();
    let cases: Vec<(String, StatusCode)> = vec![
        (
            format!("/api/synctex/forward?file=main.tex&line=0&build={id}"),
            StatusCode::BAD_REQUEST,
        ),
        (
            format!("/api/synctex/forward?file=nope.tex&line=1&build={id}"),
            StatusCode::NOT_FOUND,
        ),
        (
            format!("/api/synctex/forward?file=../x.tex&line=1&build={id}"),
            StatusCode::BAD_REQUEST,
        ),
        (
            format!("/api/synctex/forward?file=.env&line=1&build={id}"),
            StatusCode::FORBIDDEN,
        ),
        (
            "/api/synctex/forward?file=main.tex&line=1&build=zz".to_owned(),
            StatusCode::BAD_REQUEST,
        ),
        (
            format!(
                "/api/synctex/forward?file=main.tex&line=1&build={}",
                "0".repeat(32)
            ),
            StatusCode::NOT_FOUND,
        ),
        (
            format!("/api/synctex/backward?build={id}&page=0&x=1&y=1"),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            format!("/api/synctex/backward?build={id}&page=1&x=NaN&y=1"),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            format!("/api/synctex/backward?build={id}&page=1&x=-5&y=1"),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            format!("/api/synctex/backward?build={id}&page=99&x=10&y=10"),
            StatusCode::NOT_FOUND,
        ),
        (
            format!("/api/synctex/backward?build={id}&page=1&x=1"),
            StatusCode::BAD_REQUEST,
        ),
    ];
    for (uri, want) in cases {
        let (s, v) = get(&a, &uri).await;
        assert_eq!(s, want, "{uri} -> {v}");
    }
}

#[tokio::test]
async fn a_build_without_a_map_says_to_recompile() {
    let a = app().await;
    std::fs::remove_file(a.build.synctex_path()).unwrap();
    let (s, v) = get(&a, &forward_uri(&a, "main.tex", 3)).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    assert!(v["message"].as_str().unwrap().contains("recompile"), "{v}");
}
