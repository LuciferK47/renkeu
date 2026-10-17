//! M1 acceptance: the shipped `examples/sample.tex` compiles to a non-empty PDF
//! (masterplan.md §12 M1), now through the worker process.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use latex_ai_compiler::{CompileJob, Compiler, CompilerConfig};
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn sample_tex_compiles_to_pdf() {
    let examples = std::fs::canonicalize(concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples")).unwrap();
    let work = tempfile::tempdir().unwrap();
    let compiler = Compiler::new(CompilerConfig::new(
        env!("CARGO_BIN_EXE_latex-ai").into(),
        work.path().to_owned(),
    ));

    let out = compiler
        .compile_scratch(
            &CompileJob {
                root: examples,
                main: "sample.tex".to_owned(),
                synctex: false,
                timeout: std::time::Duration::from_secs(120),
            },
            &CancellationToken::new(),
        )
        .await
        .expect("sample.tex should compile");

    assert!(out.success, "{:?}", out.diagnostics);
    assert!(out.pdf.starts_with(b"%PDF"), "output is not a PDF");
}
