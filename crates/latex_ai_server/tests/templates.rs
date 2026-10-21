//! Every embedded template must compile with the real engine, from a fresh
//! folder, with ordinary and hostile user text.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use latex_ai_compiler::{CompileJob, Compiler, CompilerConfig};
use latex_ai_core::templates::{Author, Fields, list, render};
use tokio_util::sync::CancellationToken;

fn fields() -> Fields {
    Fields {
        title: "Cats & Dogs: 100% of _everything_ #1".to_owned(),
        authors: vec![
            Author {
                name: "Ada Lovelace".to_owned(),
                affiliation: "Analytical Engine Co.".to_owned(),
                email: "ada_l@example.org".to_owned(),
            },
            Author {
                name: "Alan Turing".to_owned(),
                affiliation: "King's College".to_owned(),
                email: "alan@example.org".to_owned(),
            },
        ],
        abstract_text: "First paragraph with $dollars$ and 50% of ~tildes~.\n\nSecond paragraph."
            .to_owned(),
        keywords: "tex, rust, a_b".to_owned(),
        date: String::new(),
    }
}

async fn compile_template(id: &str) -> Result<(), String> {
    let dir = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(dir.path()).unwrap();
    for file in render(id, &fields()).unwrap() {
        let path = root.join(file.path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, file.content).unwrap();
    }
    let work = tempfile::tempdir().unwrap();
    let compiler = Compiler::new(CompilerConfig::new(
        env!("CARGO_BIN_EXE_latex-ai").into(),
        work.path().to_owned(),
    ));
    let out = compiler
        .compile_scratch(
            &CompileJob {
                root,
                main: "main.tex".to_owned(),
                synctex: false,
                bundle: None,
                timeout: std::time::Duration::from_secs(240),
            },
            &CancellationToken::new(),
        )
        .await
        .map_err(|e| e.to_string())?;
    if !out.success {
        return Err(format!("{:?}", out.diagnostics));
    }
    if !out.pdf.starts_with(b"%PDF") {
        return Err("not a PDF".to_owned());
    }
    Ok(())
}

#[tokio::test]
async fn every_template_compiles() {
    let mut failures = Vec::new();
    for t in list() {
        if let Err(e) = compile_template(t.id).await {
            failures.push(format!("{}: {e}", t.id));
        }
    }
    assert!(
        failures.is_empty(),
        "templates that failed:\n{}",
        failures.join("\n")
    );
}
