//! Structured diagnostics from the TeX and BibTeX logs.
//!
//! The model (and the user) need `main.tex:12: Undefined control sequence`,
//! not a 2 KB log. Tectonic keeps the `.log` and `.blg` in memory even when a
//! run fails, so they are parsed here rather than scraped from status output.
//!
//! TeX logs are not a grammar, they are a convention, and this parser reads
//! the conventions that matter:
//!
//! - lines are hard-wrapped at 79 columns (rejoined first);
//! - `(name` opens an input file and `)` closes it, which is how an error is
//!   attributed to the file it happened in;
//! - an error is `! message`, then context lines, one of them `l.<n> source`;
//! - warnings are `LaTeX Warning: …`, `Package x Warning: …`, and boxes are
//!   `Overfull \hbox …`.
//!
//! Parenthesis tracking drifts if text itself contains an unbalanced `(`, so
//! it is never done inside a diagnostic's own paragraph, and a drifted stack
//! costs at worst a wrong file name, never a wrong message.

#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

/// More than this and the user is looking at noise, and a runaway log could
/// otherwise grow the manifest without bound.
const MAX_DIAGNOSTICS: usize = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum DiagnosticKind {
    UndefinedControlSequence,
    MissingFile,
    /// `! LaTeX Error: …` that is not a missing file.
    LatexError,
    /// `! Package x Error: …`
    PackageError,
    /// Any other `! …` TeX error (`Missing $ inserted`, runaway argument, …).
    TexError,
    /// The engine gave up (`Emergency stop`) or crashed.
    Fatal,
    UndefinedReference,
    UndefinedCitation,
    OverfullBox,
    UnderfullBox,
    LatexWarning,
    PackageWarning,
    BibtexError,
    BibtexWarning,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub severity: Severity,
    pub kind: DiagnosticKind,
    /// Project-relative path, when the log let us work it out.
    pub file: Option<String>,
    pub line: Option<u32>,
    pub message: String,
    /// The source TeX was reading when it stopped, if it said.
    pub context: Option<String>,
}

impl Diagnostic {
    /// `main.tex:12: message`, or just the message when there is no location.
    #[must_use]
    pub fn headline(&self) -> String {
        match (&self.file, self.line) {
            (Some(f), Some(l)) => format!("{f}:{l}: {}", self.message),
            (Some(f), None) => format!("{f}: {}", self.message),
            _ => self.message.clone(),
        }
    }
}

/// The most useful one-line summary: the first error, else the first warning.
#[must_use]
pub fn summarize(diagnostics: &[Diagnostic]) -> Option<String> {
    diagnostics
        .iter()
        .find(|d| d.severity == Severity::Error)
        .or_else(|| diagnostics.first())
        .map(Diagnostic::headline)
}

static LINE_CONTEXT: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"^l\.(\d+) ?(.*)$").ok());
static INPUT_LINE: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"on input line (\d+)").ok());
static BOX_LINE: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"(?:at lines?|detected at line) (\d+)").ok());
static PACKAGE_WARNING: LazyLock<Option<Regex>> = LazyLock::new(|| {
    Regex::new(r"^(?:Package|Class) [^ ]+ Warning:|^LaTeX (?:Font )?Warning:").ok()
});
static BIB_LOCATION: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"^(.*?)-+line (\d+) of file (.+)$").ok());

/// Parse a TeX `.log`.
///
/// `resolve` maps a file name as printed in the log (`chapters/a`, often
/// without extension) to a project-relative path, or `None` when it is not a
/// project file (a package, a class, a font definition).
#[must_use]
pub fn parse_tex_log(log: &str, resolve: &dyn Fn(&str) -> Option<String>) -> Vec<Diagnostic> {
    let lines = unwrap_lines(log);
    let mut out: Vec<Diagnostic> = Vec::new();
    let mut stack: Vec<Open> = Vec::new();
    let mut i = 0;

    while i < lines.len() && out.len() < MAX_DIAGNOSTICS {
        let line = lines[i].as_str();

        if let Some(rest) = line.strip_prefix("! ") {
            i = read_error(&lines, i, rest, &stack, &mut out);
        } else if is_warning_start(line) {
            i = read_warning(&lines, i, &stack, &mut out);
        } else if line.starts_with("Overfull \\") || line.starts_with("Underfull \\") {
            i = read_box(&lines, i, &stack, &mut out);
        } else {
            track_files(line, resolve, &mut stack);
            i += 1;
        }
    }

    dedupe(out)
}

/// Parse a BibTeX `.blg`.
#[must_use]
pub fn parse_bibtex_log(blg: &str) -> Vec<Diagnostic> {
    let lines: Vec<&str> = blg.lines().collect();
    let mut out = Vec::new();

    for (i, line) in lines.iter().enumerate() {
        if out.len() >= MAX_DIAGNOSTICS {
            break;
        }
        if let Some(caps) = BIB_LOCATION.as_ref().and_then(|re| re.captures(line)) {
            // The offending source follows as ` : …` lines.
            let context: Vec<&str> = lines[i + 1..]
                .iter()
                .take_while(|l| l.starts_with(" :"))
                .map(|l| l.trim_start_matches(" :").trim())
                .filter(|l| !l.is_empty())
                .collect();
            out.push(Diagnostic {
                severity: Severity::Error,
                kind: DiagnosticKind::BibtexError,
                file: Some(caps[3].trim().trim_start_matches("./").to_owned()),
                line: caps[2].parse().ok(),
                message: caps[1].trim().to_owned(),
                context: (!context.is_empty()).then(|| context.join(" ")),
            });
        } else if let Some(msg) = line.strip_prefix("Warning--") {
            out.push(Diagnostic {
                severity: Severity::Warning,
                kind: DiagnosticKind::BibtexWarning,
                file: None,
                line: None,
                message: msg.trim().to_owned(),
                context: None,
            });
        }
    }
    dedupe(out)
}

/// Undo TeX's hard wrap: a line of exactly 79 characters continues on the
/// next one.
fn unwrap_lines(log: &str) -> Vec<String> {
    const MAX_PRINT_LINE: usize = 79;
    let mut out = Vec::new();
    let mut current = String::new();
    for line in log.lines() {
        current.push_str(line);
        if line.chars().count() == MAX_PRINT_LINE {
            continue;
        }
        out.push(std::mem::take(&mut current));
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

fn is_warning_start(line: &str) -> bool {
    PACKAGE_WARNING.as_ref().is_some_and(|re| re.is_match(line))
}

/// What a `(` opened.
enum Open {
    /// A project file, by project-relative path.
    Project(String),
    /// A file that is not the project's: a package, a class, a font definition.
    Other,
    /// Ordinary text such as `(size option)`; tracked only so its `)` pops
    /// itself rather than a file.
    Text,
}

/// Update the open-file stack for one ordinary log line.
fn track_files(line: &str, resolve: &dyn Fn(&str) -> Option<String>, stack: &mut Vec<Open>) {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'(' => {
                let rest = &line[i + 1..];
                let end = rest
                    .find(|c: char| c.is_whitespace() || c == '(' || c == ')')
                    .unwrap_or(rest.len());
                let token = &rest[..end];
                // Only something that looks like a path opens a file.
                stack.push(if token.contains('.') || token.contains('/') {
                    resolve(token.trim_start_matches("./")).map_or(Open::Other, Open::Project)
                } else {
                    Open::Text
                });
                i += 1 + end;
            }
            b')' => {
                stack.pop();
                i += 1;
            }
            _ => i += 1,
        }
    }
}

/// The file an event belongs to: the innermost project file, and whether the
/// line number TeX gave refers to it (it does not if a package is on top).
fn location(stack: &[Open]) -> (Option<String>, bool) {
    let mut files = stack.iter().rev().filter(|o| !matches!(o, Open::Text));
    match files.next() {
        Some(Open::Project(f)) => (Some(f.clone()), true),
        Some(_) => (
            files.find_map(|o| match o {
                Open::Project(f) => Some(f.clone()),
                _ => None,
            }),
            false,
        ),
        None => (None, false),
    }
}

fn read_error(
    lines: &[String],
    start: usize,
    rest: &str,
    stack: &[Open],
    out: &mut Vec<Diagnostic>,
) -> usize {
    // Tectonic prints `! ! LaTeX Error: …..` for some errors.
    let mut message = rest.trim_start_matches("! ").trim().to_owned();
    if message.ends_with("..") {
        message.pop();
    }

    let mut next = start + 1;
    while lines.get(next).is_some_and(|l| l.starts_with('*')) {
        message.push(' ');
        message.push_str(lines[next].trim());
        next += 1;
    }

    // The `l.<n>` line is a few lines down, after TeX's macro context.
    let mut line_no = None;
    let mut context = None;
    let window_end = (next + 14).min(lines.len());
    for j in next..window_end {
        if lines[j].starts_with("! ") {
            break;
        }
        if let Some(caps) = LINE_CONTEXT.as_ref().and_then(|re| re.captures(&lines[j])) {
            line_no = caps[1].parse::<u32>().ok();
            let before = caps[2].trim_end();
            let after = lines.get(j + 1).map_or("", |l| l.trim());
            context = Some(match (before.is_empty(), after.is_empty()) {
                (false, false) => format!("{before} ⟨here⟩ {after}"),
                (false, true) => before.to_owned(),
                (true, false) => after.to_owned(),
                (true, true) => String::new(),
            })
            .filter(|c| !c.is_empty());
            next = j + 2;
            break;
        }
    }

    let (file, line_belongs_to_file) = location(stack);
    out.push(Diagnostic {
        severity: Severity::Error,
        kind: classify_error(&message),
        file,
        line: if line_belongs_to_file { line_no } else { None },
        message,
        context,
    });
    next
}

fn classify_error(message: &str) -> DiagnosticKind {
    if message.starts_with("Undefined control sequence") {
        DiagnosticKind::UndefinedControlSequence
    } else if message.starts_with("Emergency stop") || message.starts_with("Fatal error") {
        DiagnosticKind::Fatal
    } else if (message.contains("File `") && message.contains("not found"))
        || message.starts_with("I can't find file")
        || message.starts_with("Unable to load picture")
    {
        DiagnosticKind::MissingFile
    } else if message.starts_with("LaTeX Error:") {
        DiagnosticKind::LatexError
    } else if message.starts_with("Package ") && message.contains(" Error:") {
        DiagnosticKind::PackageError
    } else {
        DiagnosticKind::TexError
    }
}

fn read_warning(
    lines: &[String],
    start: usize,
    stack: &[Open],
    out: &mut Vec<Diagnostic>,
) -> usize {
    // A warning is one paragraph; continuation lines are `(package) text`.
    let mut next = start;
    let mut parts = Vec::new();
    while let Some(l) = lines.get(next) {
        if l.trim().is_empty() {
            break;
        }
        let text = if next == start {
            l.as_str()
        } else {
            l.trim_start().split_once(") ").map_or(l.trim(), |(_, t)| t)
        };
        parts.push(text.trim().to_owned());
        next += 1;
    }
    let message = parts.join(" ");
    let message = message.split_whitespace().collect::<Vec<_>>().join(" ");

    // End-of-run summaries repeat the specific warnings above them.
    let summary = message.contains("There were undefined references")
        || message.contains("There were multiply-defined labels")
        || message.contains("Label(s) may have changed");
    if !summary {
        let (file, belongs) = location(stack);
        let line = INPUT_LINE
            .as_ref()
            .and_then(|re| re.captures(&message))
            .and_then(|c| c[1].parse().ok());
        out.push(Diagnostic {
            severity: Severity::Warning,
            kind: classify_warning(&message),
            file,
            // "on input line N" is relative to the file being read.
            line: if belongs { line } else { None },
            message,
            context: None,
        });
    }
    next
}

fn classify_warning(message: &str) -> DiagnosticKind {
    if message.contains("Reference `") && message.contains("undefined") {
        DiagnosticKind::UndefinedReference
    } else if message.contains("Citation `") && message.contains("undefined") {
        DiagnosticKind::UndefinedCitation
    } else if message.starts_with("Package ") || message.starts_with("Class ") {
        DiagnosticKind::PackageWarning
    } else {
        DiagnosticKind::LatexWarning
    }
}

fn read_box(lines: &[String], start: usize, stack: &[Open], out: &mut Vec<Diagnostic>) -> usize {
    let head = lines[start].trim();
    let mut next = start + 1;
    // The offending text follows until a blank line.
    let mut context = None;
    while lines.get(next).is_some_and(|l| !l.trim().is_empty()) {
        if context.is_none() {
            context = Some(lines[next].trim().to_owned()).filter(|c| !c.is_empty());
        }
        next += 1;
    }

    let (file, belongs) = location(stack);
    let line = BOX_LINE
        .as_ref()
        .and_then(|re| re.captures(head))
        .and_then(|c| c[1].parse().ok());
    out.push(Diagnostic {
        severity: Severity::Warning,
        kind: if head.starts_with("Overfull") {
            DiagnosticKind::OverfullBox
        } else {
            DiagnosticKind::UnderfullBox
        },
        file,
        line: if belongs { line } else { None },
        message: head.to_owned(),
        context,
    });
    next
}

/// Reruns and multi-pass compiles repeat the same warning.
fn dedupe(items: Vec<Diagnostic>) -> Vec<Diagnostic> {
    let mut seen = std::collections::HashSet::new();
    items
        .into_iter()
        .filter(|d| seen.insert((d.kind, d.file.clone(), d.line, d.message.clone())))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> String {
        std::fs::read_to_string(format!(
            "{}/tests/fixtures/logs/{name}",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap()
    }

    /// The project files the fixtures talk about.
    fn resolver(token: &str) -> Option<String> {
        const FILES: &[&str] = &["main.tex", "chapters/a.tex", "chapters/c.tex", "refs.bib"];
        let candidates = [token.to_owned(), format!("{token}.tex")];
        candidates.into_iter().find(|c| FILES.contains(&c.as_str()))
    }

    fn parse(name: &str) -> Vec<Diagnostic> {
        parse_tex_log(&fixture(name), &resolver)
    }

    fn errors(d: &[Diagnostic]) -> Vec<&Diagnostic> {
        d.iter().filter(|d| d.severity == Severity::Error).collect()
    }

    #[test]
    fn undefined_control_sequence_in_an_included_file() {
        let d = parse("undefined_cs.log");
        let e = errors(&d);
        assert_eq!(e.len(), 1, "{d:#?}");
        assert_eq!(e[0].kind, DiagnosticKind::UndefinedControlSequence);
        // The file name in the log has no extension: `(chapters/a`.
        assert_eq!(e[0].file.as_deref(), Some("chapters/a.tex"));
        assert_eq!(e[0].line, Some(2));
        assert_eq!(e[0].message, "Undefined control sequence.");
        assert_eq!(e[0].context.as_deref(), Some("\\undefinedmacro"));
        assert_eq!(
            e[0].headline(),
            "chapters/a.tex:2: Undefined control sequence."
        );
    }

    #[test]
    fn missing_package_points_at_the_main_file() {
        let d = parse("missing_sty.log");
        let e = errors(&d);
        assert_eq!(e.len(), 1, "{d:#?}");
        assert_eq!(e[0].kind, DiagnosticKind::MissingFile);
        assert_eq!(e[0].file.as_deref(), Some("main.tex"));
        assert_eq!(e[0].line, Some(3));
        // The doubled `! ` and trailing `..` Tectonic prints are tidied.
        assert_eq!(
            e[0].message,
            "LaTeX Error: File `nonexistentpkg.sty' not found."
        );
    }

    #[test]
    fn missing_input_and_image_are_missing_files() {
        for (name, needle) in [
            ("missing_input.log", "nothere"),
            ("missing_image.log", "nope.png"),
        ] {
            let d = parse(name);
            let e = errors(&d);
            assert_eq!(e.len(), 1, "{name}: {d:#?}");
            assert_eq!(e[0].kind, DiagnosticKind::MissingFile, "{name}");
            assert!(e[0].message.contains(needle), "{name}: {}", e[0].message);
            assert_eq!(e[0].file.as_deref(), Some("main.tex"));
        }
        // missing_image also carries LaTeX's own warning about the file.
        assert!(
            parse("missing_image.log")
                .iter()
                .any(|d| d.severity == Severity::Warning && d.message.contains("nope.png"))
        );
    }

    #[test]
    fn plain_tex_errors_keep_their_context() {
        let d = parse("missing_dollar.log");
        let e = errors(&d);
        assert_eq!(e[0].kind, DiagnosticKind::TexError);
        assert_eq!(e[0].message, "Missing $ inserted.");
        assert_eq!(e[0].line, Some(3));
        assert_eq!(e[0].context.as_deref(), Some("x_ ⟨here⟩ 1 and y^2"));
    }

    #[test]
    fn runaway_argument_and_emergency_stop() {
        let d = parse("runaway.log");
        assert_eq!(
            errors(&d)[0].message,
            "File ended while scanning use of \\textbf ."
        );

        let d = parse("no_document.log");
        let e = errors(&d);
        assert_eq!(e[0].kind, DiagnosticKind::Fatal);
        assert!(
            e[0].message.contains("no legal \\end found"),
            "{}",
            e[0].message
        );
    }

    #[test]
    fn an_error_inside_a_package_is_attributed_to_the_project_file_without_a_line() {
        // `l.99` is a line of graphics.sty, not of main.tex; claiming
        // main.tex:99 would send the user to a line that does not exist.
        let d = parse("package_error.log");
        let e = errors(&d);
        assert_eq!(e.len(), 1, "{d:#?}");
        assert_eq!(e[0].file.as_deref(), Some("main.tex"));
        assert_eq!(e[0].line, None);
        assert!(
            e[0].message.contains("Unknown option `nonsenseoption'"),
            "{}",
            e[0].message
        );
    }

    #[test]
    fn environment_mismatch_is_a_latex_error() {
        let d = parse("latex_error_env.log");
        let e = errors(&d);
        assert_eq!(e[0].kind, DiagnosticKind::LatexError);
        assert_eq!(e[0].line, Some(5));
        assert!(
            e[0].message.contains("ended by \\end{enumerate}"),
            "{}",
            e[0].message
        );
    }

    #[test]
    fn warnings_are_found_classified_and_located() {
        let d = parse("warnings.log");
        assert!(errors(&d).is_empty(), "{d:#?}");

        let by_kind = |k| d.iter().find(|x| x.kind == k);
        let r = by_kind(DiagnosticKind::UndefinedReference).expect("undefined ref");
        assert_eq!((r.file.as_deref(), r.line), (Some("main.tex"), Some(4)));
        assert!(r.message.contains("`nope'"), "{}", r.message);

        let c = by_kind(DiagnosticKind::UndefinedCitation).expect("undefined cite");
        assert!(c.message.contains("`ghost'"), "{}", c.message);
        assert_eq!(c.line, Some(4));

        let o = by_kind(DiagnosticKind::OverfullBox).expect("overfull hbox");
        assert_eq!(o.line, Some(6));
        assert!(o.message.contains("148.01pt too wide"), "{}", o.message);

        // The "There were undefined references" summary adds nothing.
        assert!(
            !d.iter().any(|x| x.message.contains("There were")),
            "{d:#?}"
        );
    }

    #[test]
    fn a_clean_run_has_no_diagnostics() {
        // bibtex.log's only complaint is the citation warning.
        let d = parse("bibtex.log");
        assert!(errors(&d).is_empty(), "{d:#?}");
        assert_eq!(d.len(), 1, "{d:#?}");
        assert_eq!(d[0].kind, DiagnosticKind::UndefinedCitation);
    }

    #[test]
    fn bibtex_errors_carry_file_and_line_and_warnings_do_not() {
        let d = parse_bibtex_log(&fixture("bibtex.blg"));
        assert_eq!(d.len(), 2, "{d:#?}");

        assert_eq!(d[0].kind, DiagnosticKind::BibtexError);
        assert_eq!(d[0].file.as_deref(), Some("refs.bib"));
        assert_eq!(d[0].line, Some(10));
        assert!(
            d[0].message.starts_with("I was expecting"),
            "{}",
            d[0].message
        );
        assert!(d[0].context.as_deref().unwrap().contains("title={Broken}"));

        assert_eq!(d[1].kind, DiagnosticKind::BibtexWarning);
        assert!(
            d[1].message
                .contains("didn't find a database entry for \"ghost\"")
        );
    }

    #[test]
    fn wrapped_lines_are_rejoined() {
        let long = format!("{}{}", "a".repeat(79), "bcd");
        let lines = unwrap_lines(&format!("{long}\nnext\n"));
        assert_eq!(
            lines,
            vec![format!("{}bcd", "a".repeat(79)), "next".to_owned()]
        );
    }

    #[test]
    fn identical_warnings_from_reruns_collapse() {
        let log = "\nLaTeX Warning: Reference `x' on page 1 undefined on input line 4.\n\n\
                   LaTeX Warning: Reference `x' on page 1 undefined on input line 4.\n\n";
        assert_eq!(parse_tex_log(log, &|_| None).len(), 1);
    }

    #[test]
    fn parentheses_in_warning_text_do_not_corrupt_the_file_stack() {
        let log = "(main.tex\n\nPackage foo Warning: this (see the manual\n(foo) for details on input line 3.\n\n\
                   ! Undefined control sequence.\nl.7 \\bad\n\n";
        let d = parse_tex_log(log, &|t| (t == "main.tex").then(|| t.to_owned()));
        let e = d.iter().find(|d| d.severity == Severity::Error).unwrap();
        assert_eq!((e.file.as_deref(), e.line), (Some("main.tex"), Some(7)));
    }

    #[test]
    fn garbage_input_never_panics() {
        for junk in [
            "",
            "\n\n",
            "(",
            ")))",
            "! ",
            "l.5",
            "Overfull \\",
            "\u{0}\u{1}",
            "((((main.tex",
        ] {
            let _ = parse_tex_log(junk, &|_| None);
            let _ = parse_bibtex_log(junk);
        }
    }

    #[test]
    fn summary_prefers_the_first_error() {
        let d = parse("undefined_cs.log");
        assert_eq!(
            summarize(&d).as_deref(),
            Some("chapters/a.tex:2: Undefined control sequence.")
        );
        assert_eq!(summarize(&[]), None);
    }

    #[test]
    #[allow(clippy::pedantic)]
    fn mutated_logs_never_panic() {
        let seed = fixture("undefined_cs.log");
        let bytes = seed.as_bytes();
        let mut state = 99u64;
        let mut next = move || {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (state >> 33) as usize
        };
        for _ in 0..1500 {
            let mut b = bytes.to_vec();
            for _ in 0..(1 + next() % 10) {
                if b.is_empty() {
                    break;
                }
                let at = next() % b.len();
                match next() % 3 {
                    0 => b[at] = b"()!\n lbl{}%"[next() % 11],
                    1 => {
                        b.remove(at);
                    }
                    _ => b.truncate(at.max(1)),
                }
            }
            let text = String::from_utf8_lossy(&b);
            let _ = parse_tex_log(&text, &|n| Some(n.to_owned()));
            let _ = parse_bibtex_log(&text);
        }
    }
}
