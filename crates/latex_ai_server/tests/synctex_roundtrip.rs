//! M3 end-to-end: compile a real document, parse the `SyncTeX` map it emits,
//! and prove a PDF coordinate maps back to the source line that produced it
//! (masterplan.md §12 M3).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use latex_ai_compiler::{CompileJob, Compiler, CompilerConfig};
use tokio_util::sync::CancellationToken;

/// Line numbers are load-bearing here, so keep the document literal and count
/// carefully:
///  1 \documentclass{article}
///  2 \usepackage[margin=1in]{geometry}
///  3 \begin{document}
///  4 First paragraph on line four.
///  5 (blank)
///  6 \section{A Heading}
///  7 (blank)
///  8 Body text far down the page.
///  9 \end{document}
const DOC: &str = "\\documentclass{article}\n\
\\usepackage[margin=1in]{geometry}\n\
\\begin{document}\n\
First paragraph on line four.\n\
\n\
\\section{A Heading}\n\
\n\
Body text far down the page.\n\
\\end{document}\n";


/// Compile `DOC` as `main.tex` in a fresh project and return the parsed map
/// plus the stored build (kept alive with its temp dirs).
async fn compile_doc() -> (synctex::SyncTexData, (tempfile::TempDir, tempfile::TempDir)) {
    let project = tempfile::tempdir().unwrap();
    let store = tempfile::tempdir().unwrap();
    std::fs::write(project.path().join("main.tex"), DOC).unwrap();
    let compiler = Compiler::new(CompilerConfig::new(
        env!("CARGO_BIN_EXE_latex-ai").into(),
        store.path().to_owned(),
    ));
    let build = compiler
        .compile(
            &CompileJob {
                root: std::fs::canonicalize(project.path()).unwrap(),
                main: "main.tex".to_owned(),
                synctex: true,
                timeout: std::time::Duration::from_secs(120),
            },
            &CancellationToken::new(),
        )
        .await
        .expect("compile should succeed");
    assert!(build.manifest.success, "{:?}", build.manifest.diagnostics);
    let data = synctex::SyncTexData::parse(&build.synctex_path()).expect("parse synctex");
    (data, (project, store))
}

#[tokio::test]
async fn pdf_coordinates_map_back_to_the_source_lines_that_made_them() {
    let (data, _keep) = compile_doc().await;

    // Sanity: the map describes a real page with real content.
    assert_eq!(data.magnification, 1000);
    assert!(!data.pages.is_empty(), "no pages recorded");
    let page1 = data.pages.get(&1).expect("page 1");
    assert!(page1.len() > 5, "page 1 looks empty: {} nodes", page1.len());

    let tag = data.primary_tag().expect("a primary input tag");

    // Forward search tells us where line 4 landed; reverse search from the
    // middle of that rect must come back to line 4. Doing it this way keeps
    // the test independent of exact font metrics.
    let hits = data.forward_search(tag, 4);
    assert!(!hits.is_empty(), "line 4 produced no output");
    let rect = &hits[0];
    assert_eq!(rect.page, 1);

    // A 1in margin means content starts at x=72bp; the first line sits near
    // the top of the text block.
    assert!(
        rect.x >= 71.0 && rect.x <= 73.0,
        "expected the 1in margin at x=72bp, got {}",
        rect.x
    );

    let back = data
        .backward_search(
            rect.page,
            rect.x + rect.width / 2.0,
            rect.y + rect.height / 2.0,
        )
        .expect("reverse search should find something");
    assert_eq!(
        back.line, 4,
        "round trip landed on line {} instead of 4",
        back.line
    );
    assert!(
        back.exact,
        "the centre of a line's own box must be an exact hit"
    );
    assert_eq!(back.tag, tag);
}

#[tokio::test]
async fn later_source_lines_typeset_further_down_the_page() {
    let (data, _keep) = compile_doc().await;
    let tag = data.primary_tag().unwrap();

    let first = data.forward_search(tag, 4);
    let last = data.forward_search(tag, 8);
    assert!(!first.is_empty() && !last.is_empty());

    // y grows downward from the top of the page, so line 8 must sit below
    // line 4. This is the check that would fail if the sp->bp conversion or
    // the y-axis direction were wrong.
    assert!(
        last[0].y > first[0].y,
        "line 8 (y={}) should be below line 4 (y={})",
        last[0].y,
        first[0].y
    );
    assert!(
        first[0].y > 50.0 && last[0].y < 750.0,
        "content should sit inside a Letter page: {} .. {}",
        first[0].y,
        last[0].y
    );
}
