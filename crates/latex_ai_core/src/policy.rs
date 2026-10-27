//! What an AI-proposed edit may introduce into a TeX file.
//!
//! Tectonic cannot be sandboxed from the inside (it reads absolute and `..`
//! paths by design, and system fonts need that), so the model is stopped from
//! *writing* the dangerous forms instead. A proposal is rejected when its
//! added text:
//!
//! - references a file outside the project from an input-family command
//!   (`\input{/etc/passwd}`, `\includegraphics{../../secret}`, `\input{|cmd}`);
//! - opens a file for writing (`\openout`) or escapes to a shell (`\write18`);
//! - changes category codes (`\catcode`), which can defeat every other check
//!   by making the lexer read a safe-looking line differently.
//!
//! Only what the edit *adds* is judged, and a line that already exists in the
//! original file is let through: the lint prevents introducing a hazard, it
//! does not make a document that already has one un-editable. This is a
//! guard against a model going wrong, not a sandbox against a hostile author
//! of the project itself.

use crate::compile::{AgentEdit, AgentOp};
use regex::Regex;
use std::collections::HashSet;
use std::sync::LazyLock;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum PolicyViolation {
    #[error(
        "proposed text line {line}: `\\{command}` may only name files inside the project, not `{path}`"
    )]
    PathEscape {
        command: String,
        path: String,
        line: usize,
    },
    #[error("proposed text line {line}: `\\{command}` is not allowed in AI edits")]
    ForbiddenCommand { command: String, line: usize },
    #[error(
        "proposed text line {line}: {what} is not allowed when fitting a page limit; venues ban it"
    )]
    LayoutTrick { what: &'static str, line: usize },
}

/// Commands that read or load a file named by their argument. Matched by
/// exact name: `\includegraphics` must not be mistaken for `\include`.
const PATH_COMMANDS: &[&str] = &[
    "input",
    "include",
    "includeonly",
    "includegraphics",
    "includepdf",
    "bibliography",
    "addbibresource",
    "lstinputlisting",
    "verbatiminput",
    "openin",
    "InputIfFileExists",
    "usepackage",
    "RequirePackage",
    "documentclass",
    "LoadClass",
    "import",
    "subimport",
    "subfile",
];

static COMMAND_WITH_ARG: LazyLock<Option<Regex>> = LazyLock::new(|| {
    // \name, optional star, optional [options], then {arg} or (for \input)
    // a bare token. Whether `name` is a path command is decided in code.
    Regex::new(r"\\([A-Za-z]+)\*?(?:\s*\[[^\]]*\])*\s*(?:\{([^}]*)\}|([^\s{}\\]+))?").ok()
});

static FORBIDDEN: LazyLock<Option<Regex>> = LazyLock::new(|| {
    Regex::new(r"\\(openout|write\s*18|catcode|directlua|luaexec)(?:[^A-Za-z]|$)").ok()
});

/// Reject `edit` if the text it adds breaks the policy.
///
/// `original` is the file the edit is for; lines already in it are exempt.
///
/// # Errors
/// The first [`PolicyViolation`] found.
pub fn lint_edit(edit: &AgentEdit, original: &str) -> Result<(), PolicyViolation> {
    let added = added_text(edit);
    if added.is_empty() {
        return Ok(());
    }
    let existing: HashSet<&str> = original.lines().map(str::trim).collect();

    for (i, raw) in added.lines().enumerate() {
        if existing.contains(raw.trim()) {
            continue;
        }
        let line = strip_comment(raw);
        let number = i + 1;

        if let Some(m) = FORBIDDEN.as_ref().and_then(|re| re.captures(line)) {
            let command = m[1].split_whitespace().collect::<String>();
            return Err(PolicyViolation::ForbiddenCommand {
                command,
                line: number,
            });
        }
        if let Some(re) = COMMAND_WITH_ARG.as_ref() {
            for caps in re.captures_iter(line) {
                if !PATH_COMMANDS.contains(&&caps[1]) {
                    continue;
                }
                let arg = caps
                    .get(2)
                    .or_else(|| caps.get(3))
                    .map_or("", |m| m.as_str());
                for path in arg.split(',').map(str::trim).filter(|p| !p.is_empty()) {
                    if escapes_project(path) {
                        return Err(PolicyViolation::PathEscape {
                            command: caps[1].to_owned(),
                            path: path.to_owned(),
                            line: number,
                        });
                    }
                }
            }
        }
    }
    Ok(())
}

/// [`lint_edit`] over a batch, all against the same `original`.
///
/// # Errors
/// The first [`PolicyViolation`] found.
pub fn lint_edits<'a>(
    edits: impl IntoIterator<Item = &'a AgentEdit>,
    original: &str,
) -> Result<(), PolicyViolation> {
    edits.into_iter().try_for_each(|e| lint_edit(e, original))
}

/// The text an edit puts into the file (anchors and removed lines excluded).
pub(crate) fn added_text(edit: &AgentEdit) -> String {
    match edit.op {
        AgentOp::RewriteFile => edit.payload.clone().unwrap_or_default(),
        AgentOp::ApplyUnifiedDiff => edit
            .payload
            .as_deref()
            .unwrap_or("")
            .lines()
            .filter(|l| l.starts_with('+') && !l.starts_with("+++"))
            .map(|l| &l[1..])
            .collect::<Vec<_>>()
            .join("\n"),
        _ => edit.insert_text.clone().unwrap_or_default(),
    }
}

/// Drop a trailing `% comment` (a `%` not escaped by an odd run of
/// backslashes), so a commented-out hazard is not flagged.
pub(crate) fn strip_comment(line: &str) -> &str {
    let bytes = line.as_bytes();
    let mut backslashes = 0;
    for (i, b) in bytes.iter().enumerate() {
        match b {
            b'\\' => backslashes += 1,
            b'%' if backslashes % 2 == 0 => return &line[..i],
            _ => backslashes = 0,
        }
    }
    line
}

/// Whether a TeX file argument points outside the project.
fn escapes_project(path: &str) -> bool {
    let bytes = path.as_bytes();
    let drive = bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':';
    path.starts_with(['/', '\\', '~', '|'])
        || drive
        || path.split(['/', '\\']).any(|part| part == "..")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn insert(text: &str) -> AgentEdit {
        AgentEdit {
            file_path: "main.tex".into(),
            op: AgentOp::Insert,
            payload: None,
            text: None,
            start_line: None,
            end_line: None,
            line: None,
            insert_text: Some(text.into()),
            expected_etag: None,
            hunk_id: None,
        }
    }

    fn verdict(text: &str) -> Result<(), PolicyViolation> {
        lint_edit(&insert(text), "")
    }

    #[test]
    fn project_relative_paths_pass() {
        for ok in [
            r"\input{chapters/intro}",
            r"\include{ch1}",
            r"\includegraphics[width=0.5\linewidth]{figs/a.png}",
            r"\includegraphics[width={0.5\linewidth}]{fig.pdf}",
            r"\bibliography{refs,more/refs2}",
            r"\usepackage{amsmath,graphicx}",
            r"\documentclass[12pt]{article}",
            r"\input chapter1",
            r"\section{Why ../ in prose is fine}",
            r"\input{my..file}",
            "plain text with no commands",
        ] {
            assert_eq!(verdict(ok), Ok(()), "wrongly rejected: {ok}");
        }
    }

    #[test]
    fn paths_that_leave_the_project_are_rejected() {
        for (bad, path) in [
            (r"\input{/etc/passwd}", "/etc/passwd"),
            (r"\input{../../secrets}", "../../secrets"),
            (r"\input{a/../../b}", "a/../../b"),
            (r"\input{~/notes}", "~/notes"),
            (r"\input{|cat /etc/passwd}", "|cat /etc/passwd"),
            (r"\input /etc/hosts", "/etc/hosts"),
            (r"\include{C:\Users\x}", r"C:\Users\x"),
            (
                r"\includegraphics[width=1cm]{/home/u/.ssh/id_rsa}",
                "/home/u/.ssh/id_rsa",
            ),
            (r"\bibliography{ok,../outside}", "../outside"),
            (r"\usepackage{../evil}", "../evil"),
            (r"\lstinputlisting{/etc/shadow}", "/etc/shadow"),
            (r"\input*{/abs}", "/abs"),
            // A hazard hiding behind a harmless command on the same line.
            (r"\textbf{x}\input{/etc/passwd}", "/etc/passwd"),
            (r"\input{a}\input{/etc/passwd}", "/etc/passwd"),
            (r"\section\include{/etc/passwd}", "/etc/passwd"),
        ] {
            match verdict(bad) {
                Err(PolicyViolation::PathEscape { path: p, .. }) => assert_eq!(p, path, "{bad}"),
                other => panic!("`{bad}` should be a path escape, got {other:?}"),
            }
        }
    }

    #[test]
    fn writing_and_shell_commands_are_forbidden() {
        for (bad, cmd) in [
            (r"\openout3=out.txt", "openout"),
            (r"\immediate\openout\mystream=x", "openout"),
            (r"\immediate\write18{rm -rf ~}", "write18"),
            (r"\write 18{ls}", "write18"),
            (r"\catcode`\%=12", "catcode"),
            (r"\directlua{os.execute('x')}", "directlua"),
        ] {
            match verdict(bad) {
                Err(PolicyViolation::ForbiddenCommand { command, .. }) => {
                    assert_eq!(command, cmd, "{bad}");
                }
                other => panic!("`{bad}` should be forbidden, got {other:?}"),
            }
        }
        // Longer command names that merely start the same are not hits.
        assert_eq!(verdict(r"\openoutputfoo"), Ok(()));
        assert_eq!(verdict(r"\write19{x}"), Ok(()));
    }

    #[test]
    fn commented_out_hazards_are_ignored_but_escaped_percent_is_not() {
        assert_eq!(verdict(r"% \input{/etc/passwd}"), Ok(()));
        assert_eq!(verdict(r"text % \openout3=x"), Ok(()));
        // \% is a literal percent, so what follows is live code.
        assert!(verdict(r"50\% \input{/etc/passwd}").is_err());
        // An even run of backslashes does not escape the percent.
        assert_eq!(verdict(r"\\% \input{/etc/passwd}"), Ok(()));
    }

    #[test]
    fn only_what_the_edit_adds_is_judged() {
        let original = "\\input{/legacy/shared}\n\\catcode`\\@=11\n";
        // The model keeps those lines while editing something else.
        let mut rewrite = insert("");
        rewrite.op = AgentOp::RewriteFile;
        rewrite.payload = Some("\\input{/legacy/shared}\n\\catcode`\\@=11\nnew text\n".into());
        assert_eq!(lint_edit(&rewrite, original), Ok(()));

        // But introducing one is rejected.
        rewrite.payload = Some("\\input{/legacy/shared}\n\\input{/another}\n".into());
        assert!(lint_edit(&rewrite, original).is_err());

        // The anchor text being replaced is not "added".
        let mut replace = insert("");
        replace.op = AgentOp::ReplaceRange;
        replace.text = Some(r"\input{/etc/passwd}".into());
        replace.insert_text = Some(r"\input{safe}".into());
        assert_eq!(lint_edit(&replace, r"\input{/etc/passwd}"), Ok(()));
    }

    #[test]
    fn only_added_diff_lines_are_checked() {
        let mut diff = insert("");
        diff.op = AgentOp::ApplyUnifiedDiff;
        diff.payload = Some(
            "--- a/main.tex\n+++ b/main.tex\n@@ -1,2 +1,2 @@\n-\\input{/etc/passwd}\n+\\input{ok}\n x\n".into(),
        );
        assert_eq!(lint_edit(&diff, ""), Ok(()));

        diff.payload = Some("@@ -1,1 +1,2 @@\n x\n+\\input{/etc/passwd}\n".into());
        assert!(matches!(
            lint_edit(&diff, ""),
            Err(PolicyViolation::PathEscape { .. })
        ));
    }

    #[test]
    fn violations_report_the_line_within_the_proposed_text() {
        let err = verdict("fine\nalso fine\n\\input{/x}\n").unwrap_err();
        assert!(
            matches!(err, PolicyViolation::PathEscape { line: 3, .. }),
            "{err:?}"
        );
        assert!(err.to_string().contains("line 3"), "{err}");
    }

    #[test]
    fn batches_stop_at_the_first_bad_edit() {
        let good = insert(r"\input{ok}");
        let bad = insert(r"\input{/etc/passwd}");
        assert!(lint_edits([&good, &bad], "").is_err());
        assert!(lint_edits([&good, &good], "").is_ok());
    }
}
