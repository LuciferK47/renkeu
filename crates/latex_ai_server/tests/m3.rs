//! M3 acceptance tests (masterplan.md §12 M3):
//! - a bbox over the figure yields edits touching only the `\includegraphics`
//!   line range, with the source snippet and hit line reported;
//! - a missing `SyncTeX` map -> 404 in the error envelope;
//! - an out-of-bounds / degenerate bbox -> 422;
//! - the crop handed to the model is a real PNG capped at 512px;
//! - a VLM shape error -> 422 rather than a 500.
#![allow(clippy::unwrap_used, clippy::expect_used)]
// Test-only pixel maths on known-positive, page-sized values.
#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]

use axum::body::Body;
use axum::http::{Request, StatusCode};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use latex_ai_core::ai::{AiError, EditPlanner, PlanFuture, PlanRequest, VisualFixRequest};
use latex_ai_core::compile::{AgentEdit, AgentOp};
use latex_ai_server::{AppState, build_router, jobs::JobRegistry};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;

/// A figure whose width overflows the text column — the §12 M3 scenario.
/// Line numbers matter:
///  1 \documentclass{article}
///  2 \usepackage{graphicx}
///  3 \begin{document}
///  4 Text before the figure.
///  5 \begin{figure}[h]
///  6 \centering
///  7 \includegraphics[width=1.5\textwidth]{example-image-a}
///  8 \caption{Too wide}
///  9 \end{figure}
/// 10 Text after the figure.
/// 11 \end{document}
const FIGURE_DOC: &str = "\\documentclass{article}\n\
\\usepackage{graphicx}\n\
\\begin{document}\n\
Text before the figure.\n\
\\begin{figure}[h]\n\
\\centering\n\
\\includegraphics[width=1.5\\textwidth]{example-image-a}\n\
\\caption{Too wide}\n\
\\end{figure}\n\
Text after the figure.\n\
\\end{document}\n";

/// Records what the planner was handed, and replies with a scripted result.
struct RecordingPlanner {
    reply: Mutex<Option<Result<Vec<AgentEdit>, AiError>>>,
    seen: Arc<Mutex<Option<SeenRequest>>>,
}

#[derive(Clone, Debug)]
struct SeenRequest {
    instruction: String,
    png_base64: String,
    file_path: Option<String>,
    line: Option<u32>,
    snippet: String,
}

impl RecordingPlanner {
    fn new(reply: Result<Vec<AgentEdit>, AiError>) -> (Self, Arc<Mutex<Option<SeenRequest>>>) {
        let seen = Arc::new(Mutex::new(None));
        (
            Self {
                reply: Mutex::new(Some(reply)),
                seen: seen.clone(),
            },
            seen,
        )
    }
}

impl EditPlanner for RecordingPlanner {
    fn plan_edits<'a>(
        &'a self,
        _req: PlanRequest<'a>,
        _cancel: CancellationToken,
    ) -> PlanFuture<'a> {
        Box::pin(async { Err(AiError::EmptyResponse) })
    }

    fn visual_fix<'a>(
        &'a self,
        req: VisualFixRequest<'a>,
        _cancel: CancellationToken,
    ) -> PlanFuture<'a> {
        *self.seen.lock().unwrap() = Some(SeenRequest {
            instruction: req.instruction.to_owned(),
            png_base64: req.png_base64.to_owned(),
            file_path: req.file_path.map(str::to_owned),
            line: req.line,
            snippet: req.snippet.to_owned(),
        });
        let reply = self
            .reply
            .lock()
            .unwrap()
            .take()
            .unwrap_or(Err(AiError::EmptyResponse));
        Box::pin(async move { reply })
    }
}

struct TestApp {
    state: AppState,
    _workspace: tempfile::TempDir,
    root: std::path::PathBuf,
}

async fn test_app(planner: Arc<dyn EditPlanner>) -> TestApp {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::query(
        "CREATE TABLE history (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            file_path TEXT NOT NULL, command TEXT NOT NULL, new_text TEXT NOT NULL,
            diff TEXT NOT NULL, edit_type TEXT NOT NULL,
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP)",
    )
    .execute(&pool)
    .await
    .unwrap();

    let workspace = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(workspace.path()).unwrap();

    TestApp {
        state: AppState {
            planner,
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
        _workspace: workspace,
        root,
    }
}

/// Write `main.tex` and compile it through the real engine so the stored PDF
/// and `SyncTeX` map are genuine.
async fn compile_fixture(app: &TestApp, source: &str) -> latex_ai_compiler::builds::Build {
    std::fs::write(app.root.join("main.tex"), source).unwrap();

    let build = app
        .state
        .compiler
        .compile(
            &latex_ai_compiler::CompileJob {
                root: app.root.clone(),
                main: "main.tex".to_owned(),
                synctex: true,
                timeout: std::time::Duration::from_secs(120),
            },
            &CancellationToken::new(),
        )
        .await
        .expect("fixture must compile");
    assert!(build.manifest.success, "{:?}", build.manifest.diagnostics);
    assert!(build.manifest.has_synctex, "fixture needs a synctex map");
    build
}

fn post(uri: &str, body: &Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

async fn body_json(res: axum::response::Response) -> Value {
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

fn includegraphics_edit() -> AgentEdit {
    AgentEdit {
        file_path: "main.tex".into(),
        op: AgentOp::ReplaceRange,
        payload: None,
        text: Some("\\includegraphics[width=1.5\\textwidth]{example-image-a}".into()),
        start_line: std::num::NonZeroU64::new(7),
        end_line: std::num::NonZeroU64::new(7),
        line: None,
        insert_text: Some("\\includegraphics[width=\\textwidth]{example-image-a}".into()),
        expected_etag: None,
        hunk_id: None,
    }
}

/// The headline M3 scenario: a box drawn over the oversized figure resolves to
/// the source line that produced it, and the model's edit touches only that
/// line's range.
#[tokio::test]
async fn bbox_over_the_figure_yields_an_edit_scoped_to_the_includegraphics_line() {
    let (planner, seen) = RecordingPlanner::new(Ok(vec![includegraphics_edit()]));
    let app = test_app(Arc::new(planner)).await;
    let build = compile_fixture(&app, FIGURE_DOC).await;

    // Find where the figure landed, so the bbox is derived from the document
    // rather than from guessed pixel coordinates.
    let data = synctex::SyncTexData::parse(&build.synctex_path()).unwrap();
    let tag = data.primary_tag().unwrap();
    let figure = data
        .forward_search(tag, 7)
        .into_iter()
        .next()
        .expect("line 7 (the includegraphics) must typeset something");

    // PDF points -> page-image pixels at 150dpi.
    let to_px = |bp: f64| (bp * 150.0 / 72.0).round() as u32;
    let res = build_router(app.state.clone())
        .unwrap()
        .oneshot(post(
            "/api/visual-fix",
            &json!({
                "file_path": "main.tex",
                "page": figure.page,
                "x": to_px(figure.x),
                "y": to_px(figure.y),
                "w": to_px(figure.width).max(1),
                "h": to_px(figure.height).max(1),
                "dpi": 150,
                "instruction": "shrink this image to fit the column"
            }),
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = body_json(res).await;

    assert_eq!(body["file"], "main.tex");
    assert_eq!(
        body["line"], 7,
        "the bbox should map to the \\includegraphics line, got line {}",
        body["line"]
    );
    assert_eq!(body["exact"], true);
    // What the client needs to apply the proposal safely.
    assert_eq!(body["build"], build.id());
    assert_eq!(body["etag"], latex_ai_core::storage::etag_of(FIGURE_DOC));

    // Exactly one edit, confined to line 7.
    let edits = body["edits"].as_array().unwrap();
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0]["op"], "replace_range");
    assert_eq!(edits[0]["start_line"], 7);
    assert_eq!(edits[0]["end_line"], 7);
    assert!(
        edits[0]["text"]
            .as_str()
            .unwrap()
            .contains("includegraphics"),
        "the edit must target the includegraphics call"
    );
    assert!(
        edits[0]["hunk_id"].as_str().is_some(),
        "hunk ids are stamped so the proposal can be applied per hunk"
    );

    // What the planner actually received.
    let seen = seen.lock().unwrap().clone().expect("planner was called");
    assert_eq!(seen.instruction, "shrink this image to fit the column");
    assert_eq!(seen.file_path.as_deref(), Some("main.tex"));
    assert_eq!(seen.line, Some(7));
    assert!(
        seen.snippet.contains("\\includegraphics"),
        "the ±30-line snippet must include the hit line"
    );
    assert!(
        seen.snippet.contains("\\begin{figure}"),
        "the snippet must carry surrounding context"
    );

    // The crop is a real PNG, capped at 512px on its long edge.
    let png = STANDARD
        .decode(&seen.png_base64)
        .expect("crop is valid base64");
    assert_eq!(&png[..4], b"\x89PNG", "crop must be a PNG");
    let img = image::load_from_memory(&png).expect("crop decodes");
    assert!(
        img.width() <= 512 && img.height() <= 512,
        "crop should be capped at 512px, got {}x{}",
        img.width(),
        img.height()
    );

    // The response echoes the same crop the model saw.
    assert_eq!(body["base64_crop"], seen.png_base64);
    assert!(
        body["snippet"]
            .as_str()
            .unwrap()
            .contains("includegraphics")
    );
    assert!(body["snippet_start_line"].as_u64().unwrap() >= 1);
}

#[tokio::test]
async fn missing_synctex_map_is_a_404_envelope() {
    let (planner, _) = RecordingPlanner::new(Ok(vec![]));
    let app = test_app(Arc::new(planner)).await;
    let build = compile_fixture(&app, FIGURE_DOC).await;

    // Simulate a stored build whose SyncTeX map is gone.
    std::fs::remove_file(build.synctex_path()).unwrap();

    let res = build_router(app.state.clone())
        .unwrap()
        .oneshot(post(
            "/api/visual-fix",
            &json!({"file_path": "main.tex", "page": 1, "x": 100, "y": 100, "w": 50, "h": 50}),
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    let body = body_json(res).await;
    assert_eq!(body["error"], "not_found");
    assert!(
        body["message"].as_str().unwrap().contains("SyncTeX"),
        "the envelope should say what is missing: {body}"
    );
}

#[tokio::test]
async fn missing_pdf_is_a_404_envelope() {
    let (planner, _) = RecordingPlanner::new(Ok(vec![]));
    let app = test_app(Arc::new(planner)).await;
    std::fs::write(app.root.join("main.tex"), FIGURE_DOC).unwrap();

    let res = build_router(app.state.clone())
        .unwrap()
        .oneshot(post(
            "/api/visual-fix",
            &json!({"file_path": "main.tex", "page": 1, "x": 10, "y": 10, "w": 10, "h": 10}),
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    assert_eq!(body_json(res).await["error"], "not_found");
}

#[tokio::test]
async fn degenerate_and_oversized_bboxes_are_422() {
    let (planner, _) = RecordingPlanner::new(Ok(vec![]));
    let app = test_app(Arc::new(planner)).await;
    compile_fixture(&app, FIGURE_DOC).await;
    let router = build_router(app.state.clone()).unwrap();

    let cases = [
        ("zero width", json!({"w": 0, "h": 50})),
        ("zero height", json!({"w": 50, "h": 0})),
        ("absurd width", json!({"w": 999_999, "h": 50})),
    ];

    for (label, dims) in cases {
        let res = router
            .clone()
            .oneshot(post(
                "/api/visual-fix",
                &json!({
                    "file_path": "main.tex", "page": 1, "x": 10, "y": 10,
                    "w": dims["w"], "h": dims["h"]
                }),
            ))
            .await
            .unwrap();
        assert_eq!(
            res.status(),
            StatusCode::UNPROCESSABLE_ENTITY,
            "{label} should be rejected"
        );
        assert_eq!(body_json(res).await["error"], "unprocessable");
    }

    // Page 0 is not a thing: pages are 1-based.
    let res = router
        .oneshot(post(
            "/api/visual-fix",
            &json!({"file_path": "main.tex", "page": 0, "x": 1, "y": 1, "w": 5, "h": 5}),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn a_bbox_outside_the_page_is_422_not_500() {
    let (planner, _) = RecordingPlanner::new(Ok(vec![]));
    let app = test_app(Arc::new(planner)).await;
    compile_fixture(&app, FIGURE_DOC).await;

    // Well beyond a Letter page at 150dpi (1275x1650).
    let res = build_router(app.state.clone())
        .unwrap()
        .oneshot(post(
            "/api/visual-fix",
            &json!({"file_path": "main.tex", "page": 1, "x": 9000, "y": 9000, "w": 50, "h": 50}),
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn a_page_beyond_the_document_is_404() {
    let (planner, _) = RecordingPlanner::new(Ok(vec![]));
    let app = test_app(Arc::new(planner)).await;
    compile_fixture(&app, FIGURE_DOC).await;

    let res = build_router(app.state.clone())
        .unwrap()
        .oneshot(post(
            "/api/visual-fix",
            &json!({"file_path": "main.tex", "page": 99, "x": 10, "y": 10, "w": 10, "h": 10}),
        ))
        .await
        .unwrap();

    // No page 99 in the SyncTeX map -> reverse search miss.
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    assert_eq!(body_json(res).await["error"], "sync_tex_miss");
}

#[tokio::test]
async fn a_vlm_shape_error_is_422_with_a_warning_not_a_500() {
    // masterplan §12 M3 audit gate: "VLM JSON parse strict; 422 on bad shape".
    let bad_shape = serde_json::from_str::<Vec<AgentEdit>>("{\"nope\":1}").unwrap_err();
    let (planner, _) = RecordingPlanner::new(Err(AiError::Parse(bad_shape)));
    let app = test_app(Arc::new(planner)).await;
    compile_fixture(&app, FIGURE_DOC).await;

    let res = build_router(app.state.clone())
        .unwrap()
        .oneshot(post(
            "/api/visual-fix",
            &json!({"file_path": "main.tex", "page": 1, "x": 200, "y": 200, "w": 100, "h": 100}),
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body_json(res).await["error"], "vlm_parse_error");
}

#[tokio::test]
async fn traversal_paths_are_rejected_before_any_work() {
    let (planner, seen) = RecordingPlanner::new(Ok(vec![]));
    let app = test_app(Arc::new(planner)).await;

    let res = build_router(app.state.clone())
        .unwrap()
        .oneshot(post(
            "/api/visual-fix",
            &json!({"file_path": "../../etc/passwd", "page": 1, "x": 1, "y": 1, "w": 5, "h": 5}),
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    assert!(
        seen.lock().unwrap().is_none(),
        "planner must never be called"
    );
}

#[tokio::test]
async fn compiling_through_the_api_stores_a_build_with_a_synctex_map_and_leaves_the_project_alone()
{
    // The visual-fix flow depends on POST /api/compile leaving a map in the
    // build store, and nothing may be written beside the user's sources.
    let (planner, _) = RecordingPlanner::new(Ok(vec![]));
    let app = test_app(Arc::new(planner)).await;
    std::fs::write(app.root.join("main.tex"), FIGURE_DOC).unwrap();

    let router = build_router(app.state.clone()).unwrap();
    let res = router
        .clone()
        .oneshot(post("/api/compile", &json!({"file_path": "main.tex"})))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let job_id = body_json(res).await["job_id"].as_str().unwrap().to_owned();

    // Drain the job stream so the compile has finished.
    let res = router
        .oneshot(
            Request::builder()
                .uri(format!("/api/compile/stream/{job_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let stream = String::from_utf8(
        axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();

    let build_id = stream
        .lines()
        .find_map(|l| l.strip_prefix("data:").filter(|d| d.contains("build_id")))
        .map(|d| {
            serde_json::from_str::<Value>(d.trim()).unwrap()["build_id"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .expect("the compiled event names the build");
    let build = app
        .state
        .compiler
        .builds()
        .get(&build_id)
        .await
        .expect("build stored");

    assert!(build.pdf_path().exists(), "PDF stored");
    assert!(
        synctex::SyncTexData::parse(&build.synctex_path())
            .unwrap()
            .pages
            .contains_key(&1),
        "the stored map parses and has page 1"
    );
    assert!(
        !app.root.join("main.pdf").exists(),
        "no PDF beside the source"
    );
    assert!(
        !app.root.join("main.synctex.gz").exists(),
        "no map beside the source"
    );
}

#[tokio::test]
async fn pdf_pages_are_addressed_by_build_and_cached_only_when_they_cannot_change() {
    let (planner, _) = RecordingPlanner::new(Ok(vec![]));
    let app = test_app(Arc::new(planner)).await;
    let build = compile_fixture(&app, FIGURE_DOC).await;
    let router = build_router(app.state.clone()).unwrap();
    let get = |uri: String| Request::builder().uri(uri).body(Body::empty()).unwrap();

    // By build: an image that names one compile, so it may be cached for good.
    let res = router
        .clone()
        .oneshot(get(format!("/api/pdf/page?build={}&n=1", build.id())))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers()["content-type"], "image/png");
    assert!(
        res.headers()["cache-control"]
            .to_str()
            .unwrap()
            .contains("immutable")
    );

    // By file: "the newest build", which a recompile can replace.
    let res = router
        .clone()
        .oneshot(get("/api/pdf/page?file=main.tex&n=1".to_owned()))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers()["cache-control"], "no-cache");

    // Unknown, malformed, and neither-given are clean client errors.
    let missing = format!("/api/pdf/page?build={}&n=1", "0".repeat(32));
    assert_eq!(
        router.clone().oneshot(get(missing)).await.unwrap().status(),
        StatusCode::NOT_FOUND
    );
    let bad = "/api/pdf/page?build=../../etc&n=1".to_owned();
    assert_eq!(
        router.clone().oneshot(get(bad)).await.unwrap().status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        router
            .oneshot(get("/api/pdf/page?n=1".to_owned()))
            .await
            .unwrap()
            .status(),
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn a_box_drawn_on_an_out_of_date_pdf_is_refused_without_calling_the_model() {
    let (planner, seen) = RecordingPlanner::new(Ok(vec![]));
    let app = test_app(Arc::new(planner)).await;
    let build = compile_fixture(&app, FIGURE_DOC).await;

    // The user edits the source after the PDF was made: the box no longer
    // corresponds to these lines.
    std::fs::write(
        app.root.join("main.tex"),
        format!("% a new first line\n{FIGURE_DOC}"),
    )
    .unwrap();

    let res = build_router(app.state.clone())
        .unwrap()
        .oneshot(post(
            "/api/visual-fix",
            &json!({"file_path": "main.tex", "build": build.id(), "page": 1, "x": 100, "y": 100, "w": 50, "h": 50}),
        ))
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::CONFLICT);
    let body = body_json(res).await;
    assert!(
        body["message"].as_str().unwrap().contains("compile again"),
        "{body}"
    );
    assert!(
        seen.lock().unwrap().is_none(),
        "the model must not be asked about stale geometry"
    );
}
