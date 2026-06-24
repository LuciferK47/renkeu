//! M1 acceptance: the shipped `examples/sample.tex` compiles to a non-empty PDF
//! in-process (masterplan.md §12 M1).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use latex_ai_compiler::Engine;
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn sample_tex_compiles_to_pdf() {
    let sample = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/sample.tex");
    let latex = std::fs::read_to_string(sample).expect("read examples/sample.tex");

    let engine = Engine::default();
    let result = engine
        .compile(&latex, &CancellationToken::new())
        .await
        .expect("sample.tex should compile");

    assert!(result.success, "compile reported failure");
    assert!(result.pdf_bytes.starts_with(b"%PDF"), "output is not a PDF");
}
