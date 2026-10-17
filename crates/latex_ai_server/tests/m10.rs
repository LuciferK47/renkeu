//! M10 acceptance tests: the compile worker, multi-file projects,
//! diagnostics, the build cache, and the compile API.
//!
//! These run the real engine in real worker processes. They share one lock
//! so the "no worker is left behind" checks are not confused by a neighbour
//! test's workers.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use axum::body::Body;
use axum::http::{Request, StatusCode};
use latex_ai_compiler::diagnostics::{DiagnosticKind, Severity};
use latex_ai_compiler::{CompileError, CompileJob, Compiler, CompilerConfig};
use latex_ai_core::config::EnvOverrides;
use latex_ai_server::bootstrap::{BootOptions, build_state};
use latex_ai_server::build_router;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;

static ONE_AT_A_TIME: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

const WORKER: &str = env!("CARGO_BIN_EXE_latex-ai");

struct Project {
    root: PathBuf,
    _dir: tempfile::TempDir,
}

fn project(files: &[(&str, &str)]) -> Project {
    let dir = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(dir.path()).unwrap();
    for (name, content) in files {
        let path = root.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }
    Project { root, _dir: dir }
}

fn compiler(builds: &Path) -> Compiler {
    Compiler::new(CompilerConfig::new(WORKER.into(), builds.to_owned()))
}

fn job(p: &Project, main: &str, timeout_secs: u64) -> CompileJob {
    CompileJob {
        root: p.root.clone(),
        main: main.to_owned(),
        synctex: true,
        timeout: Duration::from_secs(timeout_secs),
    }
}

const DOC: &str = "\\documentclass{article}\n\\begin{document}\nhello\n\\end{document}\n";
const LOOP: &str =
    "\\documentclass{article}\n\\begin{document}\n\\def\\a{\\a}\\a\n\\end{document}\n";

/// Worker processes that are still children of this test process (running or
/// unreaped zombies).
fn child_processes() -> usize {
    let me = std::process::id().to_string();
    std::fs::read_dir("/proc")
        .unwrap()
        .flatten()
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .bytes()
                .all(|b| b.is_ascii_digit())
        })
        .filter_map(|e| std::fs::read_to_string(e.path().join("stat")).ok())
        .filter(|stat| {
            // `pid (comm) state ppid …`; comm may contain spaces.
            stat.rsplit_once(')')
                .and_then(|(_, rest)| rest.split_whitespace().nth(1).map(str::to_owned))
                .is_some_and(|ppid| ppid == me)
        })
        .count()
}

#[tokio::test]
async fn multi_file_project_compiles_independent_of_the_working_directory() {
    let _g = ONE_AT_A_TIME.lock().await;
    let p = project(&[
        (
            "main.tex",
            "\\documentclass{article}\n\\begin{document}\n\\input{chapters/a}\n\\include{b}\n\\bibliographystyle{plain}\n\\nocite{*}\n\\bibliography{refs}\n\\end{document}\n",
        ),
        ("chapters/a.tex", "Chapter A text.\n"),
        ("b.tex", "B text.\n"),
        (
            "refs.bib",
            "@book{k,\n  author={A. Author},\n  title={T},\n  publisher={P},\n  year={2020}\n}\n",
        ),
    ]);
    let store = tempfile::tempdir().unwrap();
    assert_ne!(
        std::env::current_dir().unwrap(),
        p.root,
        "cwd must not be the project"
    );

    let build = compiler(store.path())
        .compile(&job(&p, "main.tex", 120), &CancellationToken::new())
        .await
        .unwrap();

    assert!(build.manifest.success, "{:#?}", build.manifest.diagnostics);
    assert!(
        std::fs::read(build.pdf_path())
            .unwrap()
            .starts_with(b"%PDF")
    );
    assert!(build.manifest.has_synctex);
}

#[tokio::test]
async fn synctex_names_files_so_a_chapter_hit_resolves_to_the_chapter() {
    let _g = ONE_AT_A_TIME.lock().await;
    let p = project(&[
        (
            "main.tex",
            "\\documentclass{article}\n\\begin{document}\nMain line.\n\\input{chapters/a}\n\\end{document}\n",
        ),
        ("chapters/a.tex", "Chapter line one.\nChapter line two.\n"),
    ]);
    let store = tempfile::tempdir().unwrap();
    let build = compiler(store.path())
        .compile(&job(&p, "main.tex", 120), &CancellationToken::new())
        .await
        .unwrap();
    assert!(build.manifest.success);

    let data = synctex::SyncTexData::parse(&build.synctex_path()).unwrap();
    // Real files are recorded by absolute path; resolve each to the project.
    let resolved: Vec<(u32, PathBuf)> = data
        .inputs
        .iter()
        .filter_map(|(tag, name)| synctex::resolve_input_name(name, &p.root).map(|r| (*tag, r)))
        .collect();
    assert!(
        resolved.iter().any(|(_, r)| r == Path::new("main.tex")),
        "{resolved:?}"
    );
    let (chapter_tag, _) = resolved
        .iter()
        .find(|(_, r)| r == Path::new("chapters/a.tex"))
        .expect("the chapter must be named in the map");

    // Forward search into the chapter lands on the page, and backward search
    // from there names the chapter again.
    let hits = data.forward_search(*chapter_tag, 2);
    assert!(!hits.is_empty(), "chapter line 2 produced no output");
    let r = &hits[0];
    let back = data
        .backward_search(r.page, r.x + r.width / 2.0, r.y + r.height / 2.0)
        .unwrap();
    assert_eq!(
        synctex::resolve_input_name(&back.file, &p.root),
        Some(PathBuf::from("chapters/a.tex"))
    );
    assert_eq!(back.line, 2);
}

#[tokio::test]
async fn tex_errors_come_back_as_a_failed_build_with_file_and_line() {
    let _g = ONE_AT_A_TIME.lock().await;
    let p = project(&[
        (
            "main.tex",
            "\\documentclass{article}\n\\begin{document}\n\\input{chapters/a}\n\\end{document}\n",
        ),
        ("chapters/a.tex", "fine\n\\undefinedmacro\nmore\n"),
    ]);
    let store = tempfile::tempdir().unwrap();
    let build = compiler(store.path())
        .compile(&job(&p, "main.tex", 120), &CancellationToken::new())
        .await
        .expect("a TeX error is a result, not an Err");

    assert!(!build.manifest.success);
    assert!(!build.manifest.has_pdf);
    let error = build
        .manifest
        .diagnostics
        .iter()
        .find(|d| d.severity == Severity::Error)
        .expect("an error diagnostic");
    assert_eq!(error.kind, DiagnosticKind::UndefinedControlSequence);
    assert_eq!(error.file.as_deref(), Some("chapters/a.tex"));
    assert_eq!(error.line, Some(2));
    // The log is kept for people who want all of it.
    assert!(
        std::fs::read_to_string(build.log_path())
            .unwrap()
            .contains("Undefined control sequence")
    );
}

#[tokio::test]
async fn a_missing_package_is_reported_against_the_main_file() {
    let _g = ONE_AT_A_TIME.lock().await;
    let p = project(&[(
        "main.tex",
        "\\documentclass{article}\n\\usepackage{nonexistentpkg}\n\\begin{document}\nx\n\\end{document}\n",
    )]);
    let store = tempfile::tempdir().unwrap();
    let build = compiler(store.path())
        .compile(&job(&p, "main.tex", 120), &CancellationToken::new())
        .await
        .unwrap();
    let e = build
        .manifest
        .diagnostics
        .iter()
        .find(|d| d.severity == Severity::Error)
        .unwrap();
    assert_eq!(e.kind, DiagnosticKind::MissingFile);
    assert_eq!(e.file.as_deref(), Some("main.tex"));
}

#[tokio::test]
async fn an_infinite_loop_times_out_and_the_next_compile_still_works() {
    let _g = ONE_AT_A_TIME.lock().await;
    let p = project(&[("main.tex", DOC), ("loop.tex", LOOP)]);
    let store = tempfile::tempdir().unwrap();
    let c = compiler(store.path());
    // Warm the format cache so the timeout measures the loop, not first use.
    c.compile(&job(&p, "main.tex", 120), &CancellationToken::new())
        .await
        .unwrap();

    let started = Instant::now();
    let err = c
        .compile(&job(&p, "loop.tex", 3), &CancellationToken::new())
        .await
        .expect_err("an infinite loop must not finish");
    let took = started.elapsed();

    assert!(matches!(err, CompileError::Timeout { secs: 3 }), "{err:?}");
    assert!(
        took < Duration::from_secs(5),
        "the kill took too long: {took:?}"
    );
    assert_eq!(child_processes(), 0, "a worker was left behind");

    // The wedge that motivated the worker: the next compile must succeed.
    p_write(&p, "after.tex", DOC);
    let after = c
        .compile(&job(&p, "after.tex", 120), &CancellationToken::new())
        .await
        .unwrap();
    assert!(after.manifest.success);
    // And the timeout was not remembered as the document's verdict.
    assert!(c.builds().latest_with_pdf("loop.tex").await.is_none());
}

fn p_write(p: &Project, name: &str, content: &str) {
    std::fs::write(p.root.join(name), content).unwrap();
}

#[tokio::test]
async fn cancel_kills_the_worker_promptly_and_reaps_it() {
    let _g = ONE_AT_A_TIME.lock().await;
    let p = project(&[("main.tex", DOC), ("loop.tex", LOOP)]);
    let store = tempfile::tempdir().unwrap();
    let c = std::sync::Arc::new(compiler(store.path()));
    c.compile(&job(&p, "main.tex", 120), &CancellationToken::new())
        .await
        .unwrap();

    let cancel = CancellationToken::new();
    let running = {
        let (c, cancel, j) = (c.clone(), cancel.clone(), job(&p, "loop.tex", 60));
        tokio::spawn(async move { c.compile(&j, &cancel).await })
    };
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(
        child_processes(),
        1,
        "the loop should be running in a worker"
    );

    let cancelled_at = Instant::now();
    cancel.cancel();
    let outcome = running.await.unwrap();
    let took = cancelled_at.elapsed();

    assert!(
        matches!(outcome, Err(CompileError::Cancelled)),
        "{outcome:?}"
    );
    assert!(took < Duration::from_millis(500), "cancel took {took:?}");
    assert_eq!(child_processes(), 0, "the worker was not reaped");
}

#[tokio::test]
async fn a_worker_that_dies_is_a_crash_not_a_tex_error() {
    let _g = ONE_AT_A_TIME.lock().await;
    let p = project(&[("main.tex", DOC)]);
    let store = tempfile::tempdir().unwrap();
    let c = Compiler::new(CompilerConfig::new(
        "/bin/false".into(),
        store.path().to_owned(),
    ));
    let err = c
        .compile(&job(&p, "main.tex", 30), &CancellationToken::new())
        .await
        .unwrap_err();
    assert!(matches!(err, CompileError::WorkerCrashed { .. }), "{err:?}");
    // A crash is not cached as the project's verdict.
    assert!(c.builds().latest_with_pdf("main.tex").await.is_none());

    let missing = Compiler::new(CompilerConfig::new(
        "/nonexistent/worker".into(),
        store.path().to_owned(),
    ));
    assert!(matches!(
        missing
            .compile(&job(&p, "main.tex", 30), &CancellationToken::new())
            .await,
        Err(CompileError::Spawn { .. })
    ));
}

#[tokio::test]
async fn identical_inputs_reuse_the_build_and_any_input_change_busts_it() {
    let _g = ONE_AT_A_TIME.lock().await;
    let p = project(&[
        (
            "main.tex",
            "\\documentclass{article}\n\\usepackage{graphicx}\n\\begin{document}\n\\input{chapters/a}\\nocite{*}\n\\bibliographystyle{plain}\\bibliography{refs}\n\\end{document}\n",
        ),
        ("chapters/a.tex", "one\n"),
        (
            "refs.bib",
            "@book{k,author={A},title={T},publisher={P},year={2020}}\n",
        ),
    ]);
    let store = tempfile::tempdir().unwrap();
    let real = compiler(store.path());
    let first = real
        .compile(&job(&p, "main.tex", 120), &CancellationToken::new())
        .await
        .unwrap();
    assert!(first.manifest.success, "{:#?}", first.manifest.diagnostics);

    // A compiler whose worker cannot run still answers from the store.
    let offline = Compiler::new(CompilerConfig::new(
        "/bin/false".into(),
        store.path().to_owned(),
    ));
    let again = offline
        .compile(&job(&p, "main.tex", 5), &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(again.id(), first.id(), "same inputs, same build");

    // Each kind of dependency changes the id, so none can serve a stale PDF.
    let mut seen = vec![first.id().to_owned()];
    for (file, content) in [
        ("chapters/a.tex", "two\n"),
        (
            "refs.bib",
            "@book{k,author={B},title={T},publisher={P},year={2021}}\n",
        ),
        ("figure.png", "not really a png"),
    ] {
        p_write_nested(&p, file, content);
        let id = latex_ai_compiler::project::fingerprint(&p.root, "main.tex", true).unwrap();
        assert!(!seen.contains(&id), "{file} did not change the fingerprint");
        seen.push(id);
    }
}

fn p_write_nested(p: &Project, name: &str, content: &str) {
    let path = p.root.join(name);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

#[tokio::test]
async fn concurrent_compiles_of_one_project_publish_exactly_one_whole_build() {
    let _g = ONE_AT_A_TIME.lock().await;
    let p = project(&[("main.tex", DOC)]);
    let store = tempfile::tempdir().unwrap();
    let c = std::sync::Arc::new(compiler(store.path()));

    let spawn = |c: std::sync::Arc<Compiler>, j: CompileJob| {
        tokio::spawn(async move { c.compile(&j, &CancellationToken::new()).await })
    };
    let (a, b) = (
        spawn(c.clone(), job(&p, "main.tex", 120)),
        spawn(c.clone(), job(&p, "main.tex", 120)),
    );
    let (a, b) = (a.await.unwrap().unwrap(), b.await.unwrap().unwrap());

    assert_eq!(a.id(), b.id());
    assert!(a.pdf_path().exists() && a.dir.join("manifest.json").exists());
    let entries: Vec<String> = std::fs::read_dir(store.path())
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        entries,
        [a.id().to_owned()],
        "scratch directories leaked: {entries:?}"
    );
}

#[tokio::test]
async fn the_engine_cannot_write_outside_the_project_or_read_secrets() {
    let _g = ONE_AT_A_TIME.lock().await;
    let outside = tempfile::tempdir().unwrap();
    let target = outside.path().join("pwned.txt");
    let p = project(&[
        (
            "write.tex",
            &format!(
                "\\documentclass{{article}}\n\\begin{{document}}\n\\immediate\\openout3={}\n\\immediate\\write3{{pwned}}\n\\immediate\\closeout3\nx\n\\end{{document}}\n",
                target.display()
            ),
        ),
        (
            "read.tex",
            "\\documentclass{article}\n\\begin{document}\n\\input{.env}\n\\end{document}\n",
        ),
        (".env", "SECRET_MARKER_12345\n"),
    ]);
    let store = tempfile::tempdir().unwrap();
    let c = compiler(store.path());

    let write = c
        .compile(&job(&p, "write.tex", 120), &CancellationToken::new())
        .await
        .unwrap();
    assert!(write.manifest.success, "{:#?}", write.manifest.diagnostics);
    assert!(!target.exists(), "\\openout reached the real filesystem");
    let stray: Vec<_> = std::fs::read_dir(&p.root)
        .unwrap()
        .flatten()
        .map(|e| e.file_name())
        .collect();
    assert!(
        !stray.iter().any(|n| n.to_string_lossy().ends_with(".aux")),
        "files appeared in the project: {stray:?}"
    );

    let read = c
        .compile(&job(&p, "read.tex", 120), &CancellationToken::new())
        .await
        .unwrap();
    assert!(!read.manifest.success, "the engine read .env");
    assert!(
        read.manifest
            .diagnostics
            .iter()
            .any(|d| d.kind == DiagnosticKind::MissingFile),
        "{:#?}",
        read.manifest.diagnostics
    );
    assert!(!read.pdf_path().exists());
}

// ---- the HTTP API --------------------------------------------------------

struct Api {
    router: axum::Router,
    root: PathBuf,
    _dirs: (tempfile::TempDir, tempfile::TempDir),
}

async fn api(files: &[(&str, &str)]) -> Api {
    api_with_config(files, None).await
}

async fn api_with_config(files: &[(&str, &str)], config: Option<&str>) -> Api {
    let home = tempfile::tempdir().unwrap();
    if let Some(config) = config {
        std::fs::write(home.path().join("config.toml"), config).unwrap();
    }
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
        worker_exe: Some(WORKER.into()),
    })
    .await
    .unwrap();
    Api {
        router: build_router(state.clone()).unwrap(),
        root: state.workspace_root.clone(),
        _dirs: (home, ws),
    }
}

async fn send(
    api: &Api,
    method: &str,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
    let mut req = Request::builder().method(method).uri(uri);
    let body = match body {
        Some(v) => {
            req = req.header("content-type", "application/json");
            Body::from(v.to_string())
        }
        None => Body::empty(),
    };
    let res = api
        .router
        .clone()
        .oneshot(req.body(body).unwrap())
        .await
        .unwrap();
    let (status, headers) = (res.status(), res.headers().clone());
    (
        status,
        headers,
        axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec(),
    )
}

/// Follow a job to its end; returns the `compiled` event's data.
async fn finish(api: &Api, job_id: &str) -> Value {
    let (status, _, body) = send(api, "GET", &format!("/api/compile/stream/{job_id}"), None).await;
    assert_eq!(status, StatusCode::OK);
    let text = String::from_utf8(body).unwrap();
    let mut event = String::new();
    for line in text.lines() {
        if let Some(name) = line.strip_prefix("event:") {
            name.trim().clone_into(&mut event);
        } else if let Some(data) = line.strip_prefix("data:") {
            if event == "compiled" {
                return serde_json::from_str(data.trim()).unwrap();
            }
        }
    }
    panic!("no compiled event in:\n{text}");
}

#[tokio::test]
async fn compiling_a_chapter_compiles_the_project_and_reports_diagnostics() {
    let _g = ONE_AT_A_TIME.lock().await;
    let a = api(&[
        (
            "main.tex",
            "\\documentclass{article}\n\\begin{document}\n\\input{chapters/a}\n\\end{document}\n",
        ),
        ("chapters/a.tex", "ok\n\\oops\n"),
    ])
    .await;

    let (status, _, body) = send(
        &a,
        "POST",
        "/api/compile",
        Some(json!({"file_path": "chapters/a.tex"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let started: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        started["main_file"], "main.tex",
        "a chapter must compile its root"
    );

    let done = finish(&a, started["job_id"].as_str().unwrap()).await;
    assert_eq!(done["success"], false);
    assert_eq!(done["main_file"], "main.tex");
    assert_eq!(
        done["error_summary"],
        "chapters/a.tex:2: Undefined control sequence."
    );
    let d = &done["diagnostics"][0];
    assert_eq!(
        (d["file"].as_str(), d["line"].as_u64()),
        (Some("chapters/a.tex"), Some(2))
    );
    assert_eq!(d["kind"], "undefined_control_sequence");

    // The failed build is addressable, so the UI can show it later.
    let id = done["build_id"].as_str().unwrap();
    let (status, _, body) = send(&a, "GET", &format!("/api/builds/{id}"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        serde_json::from_slice::<Value>(&body).unwrap()["success"],
        false
    );
    // No PDF to export from it.
    assert_eq!(
        send(&a, "GET", &format!("/api/builds/{id}/pdf"), None)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    // And nothing was written into the project.
    assert!(!a.root.join("main.pdf").exists());
}

#[tokio::test]
async fn an_ambiguous_project_asks_which_file_instead_of_guessing() {
    let a = api(&[
        ("a.tex", DOC),
        ("b.tex", DOC),
        ("notes/n.tex", "text only\n"),
    ])
    .await;
    let (status, _, body) = send(
        &a,
        "POST",
        "/api/compile",
        Some(json!({"file_path": "notes/n.tex"})),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    let v: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["error"], "main_file_ambiguous");
    assert_eq!(v["candidates"], json!(["a.tex", "b.tex"]));

    // Naming one resolves it.
    let (status, _, _) = send(
        &a,
        "POST",
        "/api/compile",
        Some(json!({"file_path": "notes/n.tex", "main": "b.tex"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn a_running_job_can_be_cancelled_through_the_api() {
    let _g = ONE_AT_A_TIME.lock().await;
    let a = api(&[("main.tex", DOC), ("loop.tex", LOOP)]).await;

    // Warm up, then start the loop.
    let (_, _, body) = send(
        &a,
        "POST",
        "/api/compile",
        Some(json!({"file_path": "main.tex"})),
    )
    .await;
    finish(
        &a,
        serde_json::from_slice::<Value>(&body).unwrap()["job_id"]
            .as_str()
            .unwrap(),
    )
    .await;

    let (_, _, body) = send(
        &a,
        "POST",
        "/api/compile",
        Some(json!({"file_path": "loop.tex"})),
    )
    .await;
    let job_id = serde_json::from_slice::<Value>(&body).unwrap()["job_id"]
        .as_str()
        .unwrap()
        .to_owned();
    tokio::time::sleep(Duration::from_millis(1500)).await;

    let started = Instant::now();
    let (status, _, _) = send(&a, "POST", &format!("/api/compile/{job_id}/cancel"), None).await;
    assert_eq!(status, StatusCode::OK);
    let done = finish(&a, &job_id).await;

    assert_eq!(done["success"], false);
    assert_eq!(done["error_summary"], "compile cancelled");
    assert!(started.elapsed() < Duration::from_secs(2));
    assert_eq!(child_processes(), 0);

    assert_eq!(
        send(&a, "POST", "/api/compile/nope/cancel", None).await.0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn the_configured_timeout_applies_to_api_compiles() {
    let _g = ONE_AT_A_TIME.lock().await;
    // 5 s is the smallest budget the config accepts.
    let a = api_with_config(
        &[("main.tex", DOC), ("loop.tex", LOOP)],
        Some("[compile]\ntimeout_secs = 5\n"),
    )
    .await;
    let (_, _, body) = send(
        &a,
        "POST",
        "/api/compile",
        Some(json!({"file_path": "main.tex"})),
    )
    .await;
    finish(
        &a,
        serde_json::from_slice::<Value>(&body).unwrap()["job_id"]
            .as_str()
            .unwrap(),
    )
    .await;

    let started = Instant::now();
    let (_, _, body) = send(
        &a,
        "POST",
        "/api/compile",
        Some(json!({"file_path": "loop.tex"})),
    )
    .await;
    let done = finish(
        &a,
        serde_json::from_slice::<Value>(&body).unwrap()["job_id"]
            .as_str()
            .unwrap(),
    )
    .await;

    assert_eq!(done["success"], false);
    assert!(
        done["error_summary"]
            .as_str()
            .unwrap()
            .contains("timed out after 5s"),
        "{done}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(8),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(child_processes(), 0);
}

#[tokio::test]
async fn export_returns_the_pdf_as_an_immutable_download() {
    let _g = ONE_AT_A_TIME.lock().await;
    let a = api(&[("my paper!.tex", DOC)]).await;
    let (_, _, body) = send(
        &a,
        "POST",
        "/api/compile",
        Some(json!({"file_path": "my paper!.tex"})),
    )
    .await;
    let done = finish(
        &a,
        serde_json::from_slice::<Value>(&body).unwrap()["job_id"]
            .as_str()
            .unwrap(),
    )
    .await;
    assert_eq!(done["success"], true, "{done}");
    let id = done["build_id"].as_str().unwrap();

    let (status, headers, bytes) = send(&a, "GET", &format!("/api/builds/{id}/pdf"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(bytes.starts_with(b"%PDF"));
    assert_eq!(headers["content-type"], "application/pdf");
    // The file name is the main file's stem, made header-safe.
    assert_eq!(
        headers["content-disposition"],
        "attachment; filename=\"my_paper_.pdf\""
    );
    assert!(
        headers["cache-control"]
            .to_str()
            .unwrap()
            .contains("immutable")
    );

    for bad in ["not-hex", "../../etc/passwd", &"0".repeat(32)] {
        let (status, _, _) = send(&a, "GET", &format!("/api/builds/{bad}/pdf"), None).await;
        assert!(
            status == StatusCode::BAD_REQUEST || status == StatusCode::NOT_FOUND,
            "{bad}: {status}"
        );
    }
}
