//! The layout report over real builds: pages, a budget, boxes, short lines.

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
\\begin{document}\n\
First page text.\n\
\n\
\\noindent\\texttt{aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa}\n\
\n\
\\newpage\n\
Second page text.\n\
\\newpage\n\
Third page text.\n\
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
async fn the_report_counts_pages_and_names_where_a_budget_is_crossed() {
    let a = app(&[("main.tex", DOC)]).await;
    let (status, r) = get(&a, &format!("/api/layout?build={}&pages=2", a.build.id())).await;
    assert_eq!(status, StatusCode::OK, "{r}");
    assert_eq!(r["pages"], 3);
    // The last page holds one line, the first two, and the page numbers in the
    // feet are not counted: the last page is clearly emptier, not "100% full".
    let ratio = r["fill"]["ratio"].as_f64().unwrap();
    assert!(ratio > 0.0 && ratio < 0.9, "{r}");
    assert_eq!(r["budget"]["limit"], 2);
    assert_eq!(r["budget"]["over"], true);
    let first = &r["budget"]["first_over"];
    assert_eq!(first["page"], 3);
    assert_eq!(first["file"], "main.tex");
    // A paragraph is recorded under its last line: "Third page text." ends
    // where \end{document} starts, on line 11.
    assert_eq!(first["line"], 11, "{r}");

    let (_, fits) = get(&a, &format!("/api/layout?build={}&pages=3", a.build.id())).await;
    assert_eq!(fits["budget"]["over"], false);
    assert!(fits["budget"]["first_over"].is_null());

    let (_, none) = get(&a, &format!("/api/layout?build={}", a.build.id())).await;
    assert!(none["budget"].is_null());
    assert_eq!(none["stale"], false);
}

#[tokio::test]
async fn an_overfull_line_is_listed_with_its_size_and_its_place() {
    let a = app(&[("main.tex", DOC)]).await;
    let (_, r) = get(&a, &format!("/api/layout?build={}", a.build.id())).await;
    let boxes = r["boxes"].as_array().unwrap();
    let over = boxes
        .iter()
        .find(|b| b["overfull"] == true && b["horizontal"] == true);
    let over = over.unwrap_or_else(|| panic!("no overfull hbox in {r}"));
    assert!(over["amount_pt"].as_f64().unwrap() > 0.0);
    assert_eq!(over["place"]["page"], 1);
    assert_eq!(over["place"]["file"], "main.tex");
}

#[tokio::test]
async fn a_short_line_under_a_full_one_is_a_candidate() {
    let doc = "\\documentclass{article}\n\
\\begin{document}\n\
\\noindent\\hbox to \\textwidth{\\hfil A full line\\hfil}\n\
\\noindent\\hbox{short}\n\
\\end{document}\n";
    let a = app(&[("main.tex", doc)]).await;
    let (_, r) = get(&a, &format!("/api/layout?build={}", a.build.id())).await;
    let short = r["short_last_lines"].as_array().unwrap();
    assert_eq!(short.len(), 1, "{r}");
    // Both lines belong to the paragraph that ends on line 4.
    assert_eq!(short[0]["line"], 5, "{r}");
}

#[tokio::test]
async fn fonts_are_checked_and_nothing_is_reported_for_a_normal_build() {
    let a = app(&[("main.tex", DOC)]).await;
    let (_, r) = get(&a, &format!("/api/layout?build={}", a.build.id())).await;
    // `null` only when libpdfium cannot load (see the README); then the rest still works.
    assert!(
        r["fonts_not_embedded"].is_null() || r["fonts_not_embedded"].as_array().unwrap().is_empty(),
        "{r}"
    );
}

#[tokio::test]
async fn nonsense_requests_are_refused() {
    let a = app(&[("main.tex", DOC)]).await;
    for q in ["pages=0", "pages=99999"] {
        let (status, _) = get(&a, &format!("/api/layout?build={}&{q}", a.build.id())).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{q}");
    }
    let (status, _) = get(&a, "/api/layout?build=0123456789abcdef0123456789abcdef").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = get(&a, "/api/layout?build=../../etc").await;
    assert!(status.is_client_error());
}
