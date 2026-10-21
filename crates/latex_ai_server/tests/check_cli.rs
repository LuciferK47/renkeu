//! `latex-ai check` on real projects: the report, and the exit status a CI job
//! relies on.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use serde_json::Value;
use std::process::{Command, Output};

const GOOD: &str = "\\documentclass{article}\n\
\\begin{document}\n\
First page text.\n\
\\newpage\n\
Second page text.\n\
\\newpage\n\
Third page text.\n\
\\end{document}\n";

const BROKEN: &str = "\\documentclass{article}\n\
\\begin{document}\n\
\\nosuchcommand\n\
\\end{document}\n";

fn project(text: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("main.tex"), text).unwrap();
    dir
}

fn check(dir: &tempfile::TempDir, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_latex-ai"))
        .arg("check")
        .arg(dir.path())
        .args(args)
        .output()
        .unwrap()
}

fn text(out: &[u8]) -> String {
    String::from_utf8_lossy(out).into_owned()
}

#[test]
fn a_good_document_passes_and_says_how_long_it_is() {
    let dir = project(GOOD);
    let out = check(&dir, &["--pages", "5"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let md = text(&out.stdout);
    assert!(md.contains("main.tex passed"), "{md}");
    assert!(md.contains("Pages: 3 of 5 allowed"), "{md}");
}

#[test]
fn going_over_the_page_limit_fails_and_names_the_line() {
    let dir = project(GOOD);
    let out = check(&dir, &["--pages", "2", "--json"]);
    assert_eq!(out.status.code(), Some(1), "{}", text(&out.stderr));
    let report: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["passed"], false);
    assert_eq!(report["layout"]["pages"], 3);
    assert_eq!(report["layout"]["budget"]["over"], true);
    assert_eq!(
        report["layout"]["budget"]["first_over"]["file"], "main.tex",
        "{report}"
    );
    let problem = report["problems"][0].as_str().unwrap();
    assert!(problem.contains("over the limit of 2"), "{problem}");
}

#[test]
fn a_document_that_does_not_compile_fails_with_its_error() {
    let dir = project(BROKEN);
    let out = check(&dir, &[]);
    assert_eq!(out.status.code(), Some(1));
    let md = text(&out.stdout);
    assert!(md.contains("failed"), "{md}");
    assert!(md.contains("main.tex:3"), "{md}");
}

#[test]
fn checking_never_touches_the_project() {
    let dir = project(GOOD);
    let before: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    let out = check(&dir, &[]);
    assert!(out.status.success());
    let after: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(before, after);
}

#[test]
fn a_page_limit_of_zero_is_refused() {
    let dir = project(GOOD);
    let out = check(&dir, &["--pages", "0"]);
    assert!(!out.status.success());
    assert!(
        text(&out.stderr).contains("--pages"),
        "{}",
        text(&out.stderr)
    );
}

#[test]
fn github_mode_annotates_the_line_and_fills_the_job_summary() {
    let dir = project(GOOD);
    let summary = dir.path().join("summary.md");
    let out = Command::new(env!("CARGO_BIN_EXE_latex-ai"))
        .arg("check")
        .arg(dir.path().join("main.tex"))
        .args(["--pages", "2", "--github"])
        .env("GITHUB_STEP_SUMMARY", &summary)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1), "{}", text(&out.stderr));
    let stdout = text(&out.stdout);
    assert!(
        stdout.contains("::error file=main.tex,line=") && stdout.contains("over the limit of 2"),
        "{stdout}"
    );
    let md = std::fs::read_to_string(&summary).unwrap();
    assert!(md.contains("Renkeu check: main.tex failed"), "{md}");
}
