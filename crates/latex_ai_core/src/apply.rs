//! Deterministic, in-process application of [`AgentEdit`]s to text.
//!
//! Shared by the compile scratchpad and durable storage so both paths behave
//! identically (CONTEXT.md §2 — no duplicated logic), and replaces the former
//! shell-out to `patch(1)`, which violated the no-subprocess principle
//! (masterplan.md §1).
//!
//! Semantics per [`AgentOp`]:
//! - `RewriteFile` — `payload` becomes the whole file.
//! - `ReplaceRange` — with `start_line`/`end_line` (1-based, inclusive):
//!   replace that line span with `insert_text`; otherwise replace the FIRST
//!   occurrence of `text` (never all occurrences — a global replace silently
//!   mangles repeated snippets).
//! - `Insert` — insert `insert_text` before 1-based `line`
//!   (`line == len + 1` appends); without `line`, append at end.
//! - `ApplyUnifiedDiff` — apply a unified diff from `payload`.

use crate::compile::{AgentEdit, AgentOp};

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ApplyError {
    #[error("edit is missing required field `{0}`")]
    MissingField(&'static str),
    #[error("search text not found in target file")]
    TargetNotFound,
    #[error("line range {start}..={end} out of bounds (file has {len} lines)")]
    RangeOutOfBounds { start: u64, end: u64, len: u64 },
    #[error("malformed unified diff: {0}")]
    BadDiff(String),
    #[error("unified diff hunk does not match file content near line {0}")]
    HunkMismatch(u64),
}

/// Apply a batch sequentially; the whole batch fails if any edit fails, so
/// callers get all-or-nothing semantics.
///
/// # Errors
/// Returns the first [`ApplyError`] encountered; `content` is never partially
/// consumed by the caller in that case.
pub fn apply_edits(content: &str, edits: &[&AgentEdit]) -> Result<String, ApplyError> {
    let mut current = content.to_owned();
    for edit in edits {
        current = apply_edit(&current, edit)?;
    }
    Ok(current)
}

/// Apply a single edit. See module docs for per-op semantics.
///
/// # Errors
/// [`ApplyError`] when required fields are absent, the target text/range does
/// not exist, or a diff fails to apply.
pub fn apply_edit(content: &str, edit: &AgentEdit) -> Result<String, ApplyError> {
    match edit.op {
        AgentOp::RewriteFile => edit
            .payload
            .clone()
            .ok_or(ApplyError::MissingField("payload")),
        AgentOp::ReplaceRange => replace_range(content, edit),
        AgentOp::Insert => insert(content, edit),
        AgentOp::ApplyUnifiedDiff => {
            let diff = edit
                .payload
                .as_deref()
                .ok_or(ApplyError::MissingField("payload"))?;
            apply_unified_diff(content, diff)
        }
    }
}

fn replace_range(content: &str, edit: &AgentEdit) -> Result<String, ApplyError> {
    if let (Some(start), Some(end)) = (edit.start_line, edit.end_line) {
        let replacement = edit.insert_text.as_deref().unwrap_or("");
        return splice_lines(content, start.get(), end.get(), replacement);
    }

    let target = edit
        .text
        .as_deref()
        .ok_or(ApplyError::MissingField("text"))?;
    let replacement = edit.insert_text.as_deref().unwrap_or("");
    if !content.contains(target) {
        return Err(ApplyError::TargetNotFound);
    }
    Ok(content.replacen(target, replacement, 1))
}

fn insert(content: &str, edit: &AgentEdit) -> Result<String, ApplyError> {
    let text = edit
        .insert_text
        .as_deref()
        .ok_or(ApplyError::MissingField("insert_text"))?;

    let Some(line) = edit.line else {
        let mut out = content.to_owned();
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(text);
        return Ok(out);
    };

    let (mut lines, trailing_newline) = split_lines(content);
    let idx = usize::try_from(line.get()).unwrap_or(usize::MAX) - 1;
    if idx > lines.len() {
        return Err(ApplyError::RangeOutOfBounds {
            start: line.get(),
            end: line.get(),
            len: lines.len() as u64,
        });
    }
    lines.insert(idx, text.to_owned());
    Ok(join_lines(&lines, trailing_newline))
}

fn splice_lines(
    content: &str,
    start: u64,
    end: u64,
    replacement: &str,
) -> Result<String, ApplyError> {
    let (lines, trailing_newline) = split_lines(content);
    let len = lines.len() as u64;
    if start == 0 || start > end || end > len {
        return Err(ApplyError::RangeOutOfBounds { start, end, len });
    }
    let start_idx =
        usize::try_from(start - 1).map_err(|_| ApplyError::RangeOutOfBounds { start, end, len })?;
    let end_idx =
        usize::try_from(end).map_err(|_| ApplyError::RangeOutOfBounds { start, end, len })?;

    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    out.extend_from_slice(&lines[..start_idx]);
    if !replacement.is_empty() {
        out.extend(replacement.lines().map(str::to_owned));
    }
    out.extend_from_slice(&lines[end_idx..]);
    Ok(join_lines(&out, trailing_newline))
}

/// One parsed `@@` hunk: the old-side anchor line plus ordered ops.
struct Hunk {
    old_start: u64,
    /// `(kind, line)` where kind is ' ', '-', or '+'.
    ops: Vec<(char, String)>,
}

fn apply_unified_diff(content: &str, diff: &str) -> Result<String, ApplyError> {
    let hunks = parse_unified_diff(diff)?;
    let (lines, trailing_newline) = split_lines(content);

    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    let mut cursor = 0usize; // next unconsumed index in `lines`

    for hunk in &hunks {
        let old_lines: Vec<&str> = hunk
            .ops
            .iter()
            .filter(|(k, _)| *k == ' ' || *k == '-')
            .map(|(_, l)| l.as_str())
            .collect();

        let anchor = locate_hunk(&lines, cursor, hunk.old_start, &old_lines)
            .ok_or(ApplyError::HunkMismatch(hunk.old_start))?;

        // Everything between the cursor and the hunk is untouched context.
        out.extend_from_slice(&lines[cursor..anchor]);

        let mut offset = anchor;
        for (kind, text) in &hunk.ops {
            match kind {
                ' ' => {
                    out.push(lines[offset].clone());
                    offset += 1;
                }
                '-' => offset += 1,
                '+' => out.push(text.clone()),
                _ => unreachable!("parser only emits ' ', '-', '+'"),
            }
        }
        cursor = offset;
    }

    out.extend_from_slice(&lines[cursor..]);
    Ok(join_lines(&out, trailing_newline))
}

/// Find where a hunk's old-side lines occur: first try the position the diff
/// header claims, then fall back to a unique whole-file match so slightly
/// stale line numbers still apply.
fn locate_hunk(
    lines: &[String],
    cursor: usize,
    old_start: u64,
    old_lines: &[&str],
) -> Option<usize> {
    let claimed = usize::try_from(old_start.saturating_sub(1)).ok()?;
    let matches_at = |at: usize| -> bool {
        lines.len() >= at + old_lines.len()
            && old_lines
                .iter()
                .zip(&lines[at..at + old_lines.len()])
                .all(|(want, have)| *want == have.as_str())
    };

    if claimed >= cursor && matches_at(claimed) {
        return Some(claimed);
    }
    let candidates: Vec<usize> = (cursor..=lines.len().saturating_sub(old_lines.len()))
        .filter(|&at| matches_at(at))
        .collect();
    match candidates.as_slice() {
        [only] => Some(*only),
        _ => None,
    }
}

fn parse_unified_diff(diff: &str) -> Result<Vec<Hunk>, ApplyError> {
    let mut hunks: Vec<Hunk> = Vec::new();

    for line in diff.lines() {
        if line.starts_with("--- ")
            || line.starts_with("+++ ")
            || line.starts_with("diff ")
            || line.starts_with("index ")
        {
            continue;
        }
        if let Some(header) = line.strip_prefix("@@") {
            let old_start = parse_hunk_header(header)
                .ok_or_else(|| ApplyError::BadDiff(format!("bad hunk header: {line}")))?;
            hunks.push(Hunk {
                old_start,
                ops: Vec::new(),
            });
            continue;
        }
        let Some(hunk) = hunks.last_mut() else {
            // Prose before the first hunk header (e.g. a comment) is ignored.
            continue;
        };
        if line.starts_with('\\') {
            continue; // "\ No newline at end of file"
        }
        match line.chars().next() {
            Some(' ') | None => hunk.ops.push((' ', line.get(1..).unwrap_or("").to_owned())),
            Some('-') => hunk.ops.push(('-', line[1..].to_owned())),
            Some('+') => hunk.ops.push(('+', line[1..].to_owned())),
            _ => return Err(ApplyError::BadDiff(format!("unexpected line: {line}"))),
        }
    }

    if hunks.is_empty() {
        return Err(ApplyError::BadDiff("no @@ hunks found".to_owned()));
    }
    Ok(hunks)
}

/// Extract the old-side start line from a hunk header body like
/// ` -12,3 +14,4 @@ optional section`.
fn parse_hunk_header(header: &str) -> Option<u64> {
    let after_minus = header.trim_start().strip_prefix('-')?;
    let digits: String = after_minus
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().ok()
}

fn split_lines(content: &str) -> (Vec<String>, bool) {
    let trailing = content.ends_with('\n');
    (content.lines().map(str::to_owned).collect(), trailing)
}

fn join_lines(lines: &[String], trailing_newline: bool) -> String {
    let mut out = lines.join("\n");
    if trailing_newline && !out.is_empty() {
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compile::{AgentEdit, AgentOp};
    use std::num::NonZeroU64;

    fn edit(op: AgentOp) -> AgentEdit {
        AgentEdit {
            file_path: "main.tex".into(),
            op,
            payload: None,
            text: None,
            start_line: None,
            end_line: None,
            line: None,
            insert_text: None,
            expected_etag: None,
            hunk_id: None,
        }
    }

    fn nz(v: u64) -> NonZeroU64 {
        NonZeroU64::new(v).unwrap()
    }

    #[test]
    fn rewrite_file_replaces_everything() {
        let mut e = edit(AgentOp::RewriteFile);
        e.payload = Some("new".into());
        assert_eq!(apply_edit("old", &e).unwrap(), "new");
    }

    #[test]
    fn rewrite_without_payload_is_an_error() {
        let e = edit(AgentOp::RewriteFile);
        assert_eq!(
            apply_edit("old", &e),
            Err(ApplyError::MissingField("payload"))
        );
    }

    #[test]
    fn replace_range_replaces_only_first_occurrence() {
        let mut e = edit(AgentOp::ReplaceRange);
        e.text = Some("x".into());
        e.insert_text = Some("y".into());
        assert_eq!(apply_edit("x and x", &e).unwrap(), "y and x");
    }

    #[test]
    fn replace_range_missing_target_errors() {
        let mut e = edit(AgentOp::ReplaceRange);
        e.text = Some("absent".into());
        e.insert_text = Some("y".into());
        assert_eq!(apply_edit("content", &e), Err(ApplyError::TargetNotFound));
    }

    #[test]
    fn replace_range_by_lines_splices_inclusive_span() {
        let mut e = edit(AgentOp::ReplaceRange);
        e.start_line = Some(nz(2));
        e.end_line = Some(nz(3));
        e.insert_text = Some("B\nC".into());
        assert_eq!(apply_edit("a\nb\nc\nd\n", &e).unwrap(), "a\nB\nC\nd\n");
    }

    #[test]
    fn replace_range_by_lines_out_of_bounds_errors() {
        let mut e = edit(AgentOp::ReplaceRange);
        e.start_line = Some(nz(3));
        e.end_line = Some(nz(9));
        e.insert_text = Some("x".into());
        assert!(matches!(
            apply_edit("a\nb\n", &e),
            Err(ApplyError::RangeOutOfBounds { .. })
        ));
    }

    #[test]
    fn insert_appends_without_line() {
        let mut e = edit(AgentOp::Insert);
        e.insert_text = Some("tail".into());
        assert_eq!(apply_edit("head\n", &e).unwrap(), "head\ntail");
    }

    #[test]
    fn insert_before_line() {
        let mut e = edit(AgentOp::Insert);
        e.line = Some(nz(2));
        e.insert_text = Some("mid".into());
        assert_eq!(apply_edit("a\nb\n", &e).unwrap(), "a\nmid\nb\n");
    }

    #[test]
    fn unified_diff_applies_at_claimed_position() {
        let mut e = edit(AgentOp::ApplyUnifiedDiff);
        e.payload =
            Some("--- a/main.tex\n+++ b/main.tex\n@@ -1,3 +1,3 @@\n a\n-b\n+B\n c\n".into());
        assert_eq!(apply_edit("a\nb\nc\n", &e).unwrap(), "a\nB\nc\n");
    }

    #[test]
    fn unified_diff_relocates_when_line_numbers_drift() {
        let mut e = edit(AgentOp::ApplyUnifiedDiff);
        // Header claims line 1 but the context only matches at line 3.
        e.payload = Some("@@ -1,2 +1,2 @@\n x\n-y\n+Y\n".into());
        assert_eq!(apply_edit("a\nb\nx\ny\n", &e).unwrap(), "a\nb\nx\nY\n");
    }

    #[test]
    fn unified_diff_mismatch_is_an_error_not_a_silent_noop() {
        let mut e = edit(AgentOp::ApplyUnifiedDiff);
        e.payload = Some("@@ -1,1 +1,1 @@\n-not here\n+never\n".into());
        assert!(matches!(
            apply_edit("a\nb\n", &e),
            Err(ApplyError::HunkMismatch(_))
        ));
    }

    #[test]
    fn batch_apply_is_sequential_and_all_or_nothing() {
        let mut first = edit(AgentOp::ReplaceRange);
        first.text = Some("a".into());
        first.insert_text = Some("z".into());
        let mut second = edit(AgentOp::ReplaceRange);
        second.text = Some("missing".into());
        second.insert_text = Some("_".into());

        assert_eq!(
            apply_edits("a b", &[&first, &second]),
            Err(ApplyError::TargetNotFound)
        );
        assert_eq!(apply_edits("a b", &[&first]).unwrap(), "z b");
    }
}
