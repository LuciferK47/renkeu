//! Deterministic, in-process application of [`AgentEdit`]s to text.
//!
//! Shared by the compile scratchpad and durable storage so both paths behave
//! identically (CONTEXT.md §2 — no duplicated logic), and replaces the former
//! shell-out to `patch(1)`, which violated the no-subprocess principle
//! (masterplan.md §1).
//!
//! # Resolve, then splice
//!
//! Every edit in a batch is resolved to byte ranges against the **original**
//! text, overlaps are rejected, and only then are the splices applied. Edits
//! therefore never see each other's output: line numbers always mean "the
//! file the planner was shown", and a batch gives the same result in any
//! order. (The earlier apply-one-then-the-next scheme shifted every later
//! line number and made a batch's meaning depend on its order.)
//!
//! Semantics per [`AgentOp`]:
//! - `RewriteFile` — `payload` becomes the whole file; it must be the only
//!   edit for that file.
//! - `ReplaceRange` — with `start_line`/`end_line` (1-based, inclusive):
//!   replace that line span with `insert_text`; otherwise replace an
//!   occurrence of `text` (never all of them — a global replace silently
//!   mangles repeated snippets). Several edits with the *same* `text` take
//!   successive occurrences, in batch order.
//! - `Insert` — insert `insert_text` before 1-based `line`
//!   (`line == len + 1` appends); without `line`, append at end.
//! - `ApplyUnifiedDiff` — apply a unified diff from `payload`.
//!
//! The file's byte-order mark, line-ending style (LF or CRLF) and trailing
//! newline are preserved; text the planner supplies is converted to the
//! file's line endings.

use crate::compile::{AgentEdit, AgentOp};
use std::collections::{BTreeMap, HashMap};

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
    /// Edits `first` and `second` (1-based positions in the batch) change
    /// the same text, so there is no single right result.
    #[error("edits {first} and {second} change overlapping text")]
    Overlap { first: usize, second: usize },
    #[error("`rewrite_file` cannot be combined with other edits to the same file")]
    RewriteNotAlone,
    #[error("edit targets `{0}`, which is not part of the project files given")]
    UnknownFile(String),
    #[error("{path}: {source}")]
    InFile {
        path: String,
        source: Box<ApplyError>,
    },
}

/// Apply a batch; the whole batch fails if any edit fails, so callers get
/// all-or-nothing semantics.
///
/// # Errors
/// Returns the first [`ApplyError`] encountered; `content` is never partially
/// consumed by the caller in that case.
pub fn apply_edits(content: &str, edits: &[&AgentEdit]) -> Result<String, ApplyError> {
    let doc = Doc::new(content);
    let mut splices: Vec<Splice> = Vec::new();
    let mut anchor_uses: HashMap<String, usize> = HashMap::new();

    for (index, edit) in edits.iter().enumerate() {
        match edit.op {
            AgentOp::RewriteFile => {
                if edits.len() > 1 {
                    return Err(ApplyError::RewriteNotAlone);
                }
                let payload = edit
                    .payload
                    .as_deref()
                    .ok_or(ApplyError::MissingField("payload"))?;
                splices.push(doc.rewrite(payload, index));
            }
            AgentOp::ReplaceRange => {
                splices.push(replace_range(&doc, edit, index, &mut anchor_uses)?);
            }
            AgentOp::Insert => splices.push(insert(&doc, edit, index)?),
            AgentOp::ApplyUnifiedDiff => {
                let diff = edit
                    .payload
                    .as_deref()
                    .ok_or(ApplyError::MissingField("payload"))?;
                splices.extend(diff_splices(&doc, diff, index)?);
            }
        }
    }

    splice_all(content, splices)
}

/// Apply a single edit. See module docs for per-op semantics.
///
/// # Errors
/// [`ApplyError`] when required fields are absent, the target text/range does
/// not exist, or a diff fails to apply.
pub fn apply_edit(content: &str, edit: &AgentEdit) -> Result<String, ApplyError> {
    apply_edits(content, &[edit])
}

/// Apply a batch that may touch several files, all-or-nothing.
///
/// `files` maps project-relative path to current content. Returns the new
/// content of every file an edit targeted. An edit naming a file that is not
/// in `files` is an error rather than a silent skip: creating files is a
/// separate, explicit operation.
///
/// # Errors
/// The first failure, wrapped in [`ApplyError::InFile`] with the file's path.
pub fn apply_to_files(
    files: &BTreeMap<String, String>,
    edits: &[AgentEdit],
) -> Result<BTreeMap<String, String>, ApplyError> {
    let mut by_file: BTreeMap<&str, Vec<&AgentEdit>> = BTreeMap::new();
    for edit in edits {
        by_file
            .entry(edit.file_path.as_str())
            .or_default()
            .push(edit);
    }

    let mut out = BTreeMap::new();
    for (path, group) in by_file {
        let content = files
            .get(path)
            .ok_or_else(|| ApplyError::UnknownFile(path.to_owned()))?;
        let updated = apply_edits(content, &group).map_err(|source| ApplyError::InFile {
            path: path.to_owned(),
            source: Box::new(source),
        })?;
        out.insert(path.to_owned(), updated);
    }
    Ok(out)
}

/// The edits that turn `original` into `modified`: one per contiguous region
/// that changed, expressed in **original** line numbers.
///
/// This is how an agent run that took several iterations is shown for
/// review. The iterations' own edits build on each other (the second one's
/// line numbers refer to the text after the first), so they cannot be
/// applied one by one to the user's file; the net change can, as independent
/// hunks that never overlap, so the user can accept some and not others.
///
/// Applying the result to `original` gives `modified`, provided both end
/// with a newline (or both do not) — a change to only the final newline is
/// not representable as a line edit.
#[must_use]
pub fn edits_from_diff(file_path: &str, original: &str, modified: &str) -> Vec<AgentEdit> {
    use similar::TextDiff;

    struct Region {
        old_start: usize,
        old_len: usize,
        new_start: usize,
        new_len: usize,
    }

    let blank = |op: AgentOp| AgentEdit {
        file_path: file_path.to_owned(),
        op,
        payload: None,
        text: None,
        start_line: None,
        end_line: None,
        line: None,
        insert_text: None,
        expected_etag: None,
        hunk_id: None,
    };

    if original == modified {
        return Vec::new();
    }
    if original.is_empty() {
        let mut edit = blank(AgentOp::RewriteFile);
        edit.payload = Some(modified.to_owned());
        return vec![edit];
    }

    let new_lines: Vec<&str> = modified.split_inclusive('\n').collect();
    let diff = TextDiff::from_lines(original, modified);

    // Walk the script by lengths alone. The `*_index` fields `similar`
    // reports on some ops are hints that are not always consistent with the
    // script order (a property test caught a deletion "at" new line 4), so
    // positions are tracked here instead.
    let mut regions: Vec<Region> = Vec::new();
    let (mut old_pos, mut new_pos) = (0usize, 0usize);
    let mut open: Option<Region> = None;
    for op in diff.ops() {
        let (old_len, new_len, equal) = match *op {
            similar::DiffOp::Equal { len, .. } => (len, len, true),
            similar::DiffOp::Delete { old_len, .. } => (old_len, 0, false),
            similar::DiffOp::Insert { new_len, .. } => (0, new_len, false),
            similar::DiffOp::Replace {
                old_len, new_len, ..
            } => (old_len, new_len, false),
        };
        if equal {
            regions.extend(open.take());
        } else {
            let region = open.get_or_insert(Region {
                old_start: old_pos,
                old_len: 0,
                new_start: new_pos,
                new_len: 0,
            });
            region.old_len += old_len;
            region.new_len += new_len;
        }
        old_pos += old_len;
        new_pos += new_len;
    }
    regions.extend(open);

    let mut edits = Vec::new();
    for region in regions {
        let (old_start, old_end) = (region.old_start, region.old_start + region.old_len);
        let new_range = region.new_start..region.new_start + region.new_len;

        // The block keeps its own final line ending: `lines_of` drops exactly
        // one, so a block that ends in a blank line (`"c\n\n"`) still has it.
        let text: String = new_lines[new_range].concat();

        let edit = if old_start == old_end {
            let mut e = blank(AgentOp::Insert);
            e.line = std::num::NonZeroU64::new(old_start as u64 + 1);
            e.insert_text = Some(text);
            e
        } else {
            let mut e = blank(AgentOp::ReplaceRange);
            e.start_line = std::num::NonZeroU64::new(old_start as u64 + 1);
            e.end_line = std::num::NonZeroU64::new(old_end as u64);
            e.insert_text = Some(text);
            e
        };
        edits.push(edit);
    }
    edits
}

/// A line number that fits `usize` (saturating on 32-bit targets).
fn to_usize(n: u64) -> usize {
    usize::try_from(n).unwrap_or(usize::MAX)
}

/// An edit that cannot be moved onto the file as it is now.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum RebaseError {
    /// Edit `edit` (1-based) touches lines that changed since the proposal.
    #[error("edit {edit} touches lines that have changed since the proposal was made")]
    Conflict { edit: usize },
}

/// Move `edits`, written against `base`, onto `current`.
///
/// Line-numbered edits are re-pointed using a line diff of `base` -> `current`:
/// an edit moves only if every line it replaces — or the line it inserts
/// before — is unchanged in `current`. Anything else is a [`RebaseError`]
/// rather than a guess. Text-anchored edits need no change (they search
/// `current` anyway); whole-file edits move only if the file is untouched.
///
/// # Errors
/// The first edit that cannot be moved.
pub fn rebase_edits(
    base: &str,
    current: &str,
    edits: &[AgentEdit],
) -> Result<Vec<AgentEdit>, RebaseError> {
    use similar::{DiffOp, TextDiff};

    let base_len = base.split_inclusive('\n').count();
    let current_len = current.split_inclusive('\n').count();
    let mut map: Vec<Option<usize>> = vec![None; base_len];
    // Positions are tracked by walking the script, not read from the ops'
    // index fields (see `edits_from_diff`).
    let (mut old_pos, mut new_pos) = (0usize, 0usize);
    for op in TextDiff::from_lines(base, current).ops() {
        match *op {
            DiffOp::Equal { len, .. } => {
                for k in 0..len {
                    map[old_pos + k] = Some(new_pos + k);
                }
                old_pos += len;
                new_pos += len;
            }
            DiffOp::Delete { old_len, .. } => old_pos += old_len,
            DiffOp::Insert { new_len, .. } => new_pos += new_len,
            DiffOp::Replace {
                old_len, new_len, ..
            } => {
                old_pos += old_len;
                new_pos += new_len;
            }
        }
    }

    let mut out = Vec::with_capacity(edits.len());
    for (i, edit) in edits.iter().enumerate() {
        let conflict = || RebaseError::Conflict { edit: i + 1 };
        let mut moved = edit.clone();
        moved.expected_etag = None;
        moved.hunk_id = None;

        match edit.op {
            AgentOp::ReplaceRange if edit.start_line.is_some() && edit.end_line.is_some() => {
                let (Some(start), Some(end)) = (edit.start_line, edit.end_line) else {
                    return Err(conflict());
                };
                let (s, e) = (to_usize(start.get()), to_usize(end.get()));
                let first = map.get(s - 1).copied().flatten().ok_or_else(conflict)?;
                for k in s..=e {
                    if map.get(k - 1).copied().flatten() != Some(first + (k - s)) {
                        return Err(conflict());
                    }
                }
                moved.start_line = std::num::NonZeroU64::new(first as u64 + 1);
                moved.end_line = std::num::NonZeroU64::new((first + (e - s)) as u64 + 1);
            }
            AgentOp::Insert if edit.line.is_some() => {
                let line = edit.line.map_or(1, |l| to_usize(l.get()));
                let target = if line > base_len {
                    current_len
                } else {
                    map.get(line - 1).copied().flatten().ok_or_else(conflict)?
                };
                moved.line = std::num::NonZeroU64::new(target as u64 + 1);
            }
            AgentOp::RewriteFile | AgentOp::ApplyUnifiedDiff if base != current => {
                return Err(conflict());
            }
            _ => {}
        }
        out.push(moved);
    }
    Ok(out)
}

/// One resolved change: bytes `start..end` of the original become `text`.
struct Splice {
    start: usize,
    end: usize,
    text: String,
    /// Position of the originating edit in the batch, for error messages.
    edit: usize,
}

fn splice_all(content: &str, mut splices: Vec<Splice>) -> Result<String, ApplyError> {
    // Sorted by position; ties keep batch order (sort is stable), so several
    // insertions at one point land in the order they were proposed.
    splices.sort_by_key(|s| (s.start, s.end));
    for pair in splices.windows(2) {
        if pair[0].end > pair[1].start {
            return Err(ApplyError::Overlap {
                first: pair[0].edit.min(pair[1].edit) + 1,
                second: pair[0].edit.max(pair[1].edit) + 1,
            });
        }
    }

    let mut out = String::with_capacity(content.len());
    let mut cursor = 0;
    for s in &splices {
        out.push_str(&content[cursor..s.start]);
        out.push_str(&s.text);
        cursor = s.end;
    }
    out.push_str(&content[cursor..]);
    Ok(out)
}

/// The original text with its lines indexed, so line numbers map to bytes
/// without touching line endings or the byte-order mark.
struct Doc<'a> {
    text: &'a str,
    /// Byte length of a leading BOM (0 or 3); it belongs to no line.
    bom: usize,
    eol: &'static str,
    /// Byte offset where each line starts. A line includes its terminator.
    starts: Vec<usize>,
}

impl<'a> Doc<'a> {
    fn new(text: &'a str) -> Self {
        let bom = if text.starts_with('\u{feff}') { 3 } else { 0 };
        let mut starts = Vec::new();
        if text.len() > bom {
            starts.push(bom);
            for (i, b) in text.bytes().enumerate() {
                if b == b'\n' && i + 1 < text.len() {
                    starts.push(i + 1);
                }
            }
        }
        let lf = text.matches('\n').count();
        let crlf = text.matches("\r\n").count();
        let eol = if crlf > 0 && crlf * 2 >= lf {
            "\r\n"
        } else {
            "\n"
        };
        Self {
            text,
            bom,
            eol,
            starts,
        }
    }

    fn line_count(&self) -> usize {
        self.starts.len()
    }

    /// Byte offset of 0-based line `i`; one past the last line is the end.
    fn line_start(&self, i: usize) -> usize {
        self.starts.get(i).copied().unwrap_or(self.text.len())
    }

    fn ends_with_newline(&self) -> bool {
        self.text.ends_with('\n')
    }

    /// Line `i` (0-based) without its terminator.
    fn line(&self, i: usize) -> &str {
        let raw = &self.text[self.line_start(i)..self.line_start(i + 1)];
        raw.strip_suffix('\n')
            .map_or(raw, |r| r.strip_suffix('\r').unwrap_or(r))
    }

    /// `s` with this file's line endings.
    fn native(&self, s: &str) -> String {
        let lf = s.replace("\r\n", "\n");
        if self.eol == "\r\n" {
            lf.replace('\n', "\r\n")
        } else {
            lf
        }
    }

    fn rewrite(&self, payload: &str, edit: usize) -> Splice {
        let mut body = self.native(payload.trim_start_matches('\u{feff}'));
        if self.ends_with_newline() && !body.is_empty() && !body.ends_with('\n') {
            body.push_str(self.eol);
        }
        Splice {
            start: self.bom,
            end: self.text.len(),
            text: body,
            edit,
        }
    }

    /// Replace 0-based lines `from..to` with `new_lines`, keeping the file's
    /// line endings and its trailing-newline state.
    fn line_splice(&self, from: usize, to: usize, new_lines: &[String], edit: usize) -> Splice {
        let start = self.line_start(from);
        let end = self.line_start(to);
        let n = self.line_count();

        let mut prefix = "";
        let terminated = if to > from {
            // Replacing real lines: end the block the way the last one ended.
            self.text[..end].ends_with('\n')
        } else if from < n {
            true
        } else if n == 0 {
            false
        } else {
            // Appending after a last line that has no terminator.
            if !self.ends_with_newline() {
                prefix = self.eol;
            }
            self.ends_with_newline()
        };

        let mut text = String::new();
        if !new_lines.is_empty() {
            text.push_str(prefix);
            text.push_str(&new_lines.join(self.eol));
            if terminated {
                text.push_str(self.eol);
            }
        }
        Splice {
            start,
            end,
            text,
            edit,
        }
    }
}

/// Split planner-supplied text into lines (one trailing newline is ignored).
fn lines_of(text: &str) -> Vec<String> {
    let lf = text.replace("\r\n", "\n");
    if lf.is_empty() {
        return Vec::new();
    }
    lf.strip_suffix('\n')
        .unwrap_or(&lf)
        .split('\n')
        .map(str::to_owned)
        .collect()
}

fn replace_range(
    doc: &Doc<'_>,
    edit: &AgentEdit,
    index: usize,
    anchor_uses: &mut HashMap<String, usize>,
) -> Result<Splice, ApplyError> {
    let replacement = edit.insert_text.as_deref().unwrap_or("");

    if let (Some(start), Some(end)) = (edit.start_line, edit.end_line) {
        let (start, end) = (start.get(), end.get());
        let len = doc.line_count() as u64;
        if start > end || end > len {
            return Err(ApplyError::RangeOutOfBounds { start, end, len });
        }
        let from = usize::try_from(start - 1).map_err(|_| ApplyError::RangeOutOfBounds {
            start,
            end,
            len,
        })?;
        let to =
            usize::try_from(end).map_err(|_| ApplyError::RangeOutOfBounds { start, end, len })?;
        return Ok(doc.line_splice(from, to, &lines_of(replacement), index));
    }

    let target = edit
        .text
        .as_deref()
        .filter(|t| !t.is_empty())
        .ok_or(ApplyError::MissingField("text"))?;
    // The planner writes `\n`; a CRLF file needs `\r\n` to match.
    let needle = doc.native(target);
    let nth = {
        let used = anchor_uses.entry(needle.clone()).or_insert(0);
        let n = *used;
        *used += 1;
        n
    };
    let (start, _) = doc
        .text
        .match_indices(needle.as_str())
        .nth(nth)
        .ok_or(ApplyError::TargetNotFound)?;
    Ok(Splice {
        start,
        end: start + needle.len(),
        text: doc.native(replacement),
        edit: index,
    })
}

fn insert(doc: &Doc<'_>, edit: &AgentEdit, index: usize) -> Result<Splice, ApplyError> {
    let text = edit
        .insert_text
        .as_deref()
        .ok_or(ApplyError::MissingField("insert_text"))?;

    let Some(line) = edit.line else {
        // Append raw text, separated from the last line if it has no newline.
        let mut body = String::new();
        if doc.text.len() > doc.bom && !doc.ends_with_newline() {
            body.push_str(doc.eol);
        }
        body.push_str(&doc.native(text));
        let end = doc.text.len();
        return Ok(Splice {
            start: end,
            end,
            text: body,
            edit: index,
        });
    };

    let at = usize::try_from(line.get()).unwrap_or(usize::MAX) - 1;
    if at > doc.line_count() {
        return Err(ApplyError::RangeOutOfBounds {
            start: line.get(),
            end: line.get(),
            len: doc.line_count() as u64,
        });
    }
    // An empty `insert_text` inserts one blank line, not nothing.
    let lines = if text.is_empty() {
        vec![String::new()]
    } else {
        lines_of(text)
    };
    Ok(doc.line_splice(at, at, &lines, index))
}

/// One parsed `@@` hunk: the old-side anchor line plus ordered ops.
struct Hunk {
    old_start: u64,
    /// `(kind, line)` where kind is ' ', '-', or '+'.
    ops: Vec<(char, String)>,
}

fn diff_splices(doc: &Doc<'_>, diff: &str, edit: usize) -> Result<Vec<Splice>, ApplyError> {
    let hunks = parse_unified_diff(diff)?;
    let mut splices = Vec::new();
    let mut cursor = 0usize; // first line a later hunk may start at

    for hunk in &hunks {
        let old_lines: Vec<&str> = hunk
            .ops
            .iter()
            .filter(|(k, _)| *k == ' ' || *k == '-')
            .map(|(_, l)| l.as_str())
            .collect();

        let anchor = locate_hunk(doc, cursor, hunk.old_start, &old_lines)
            .ok_or(ApplyError::HunkMismatch(hunk.old_start))?;
        cursor = anchor + old_lines.len();

        // Splice only from the first to the last changed line: surrounding
        // context stays untouched, so two hunks that merely share context
        // do not count as overlapping.
        let Some(first) = hunk.ops.iter().position(|(k, _)| *k != ' ') else {
            continue;
        };
        let last = hunk
            .ops
            .iter()
            .rposition(|(k, _)| *k != ' ')
            .unwrap_or(first);
        let old_side = |ops: &[(char, String)]| ops.iter().filter(|(k, _)| *k != '+').count();

        let from = anchor + old_side(&hunk.ops[..first]);
        let to = from + old_side(&hunk.ops[first..=last]);
        let new_lines: Vec<String> = hunk.ops[first..=last]
            .iter()
            .filter(|(k, _)| *k != '-')
            .map(|(_, l)| l.clone())
            .collect();
        splices.push(doc.line_splice(from, to, &new_lines, edit));
    }
    Ok(splices)
}

/// Find where a hunk's old-side lines occur: first try the position the diff
/// header claims, then fall back to a unique match so slightly stale line
/// numbers still apply.
fn locate_hunk(doc: &Doc<'_>, cursor: usize, old_start: u64, old_lines: &[&str]) -> Option<usize> {
    let n = doc.line_count();
    let claimed = usize::try_from(old_start.saturating_sub(1)).ok()?;
    let matches_at = |at: usize| -> bool {
        n >= at + old_lines.len()
            && old_lines
                .iter()
                .enumerate()
                .all(|(i, want)| doc.line(at + i) == *want)
    };

    if claimed >= cursor && matches_at(claimed) {
        return Some(claimed);
    }
    let candidates: Vec<usize> = (cursor..=n.saturating_sub(old_lines.len()))
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

    // ---- resolve-then-splice ------------------------------------------

    fn lines_edit(start: u64, end: u64, with: &str) -> AgentEdit {
        let mut e = edit(AgentOp::ReplaceRange);
        e.start_line = Some(nz(start));
        e.end_line = Some(nz(end));
        e.insert_text = Some(with.into());
        e
    }

    fn text_edit(text: &str, with: &str) -> AgentEdit {
        let mut e = edit(AgentOp::ReplaceRange);
        e.text = Some(text.into());
        e.insert_text = Some(with.into());
        e
    }

    fn insert_edit(line: u64, text: &str) -> AgentEdit {
        let mut e = edit(AgentOp::Insert);
        e.line = Some(nz(line));
        e.insert_text = Some(text.into());
        e
    }

    #[test]
    fn line_numbers_always_refer_to_the_original_file() {
        // The first edit grows the file by two lines. Applied sequentially,
        // the second edit's "line 4" would have hit the wrong line.
        let first = lines_edit(2, 2, "B1\nB2\nB3");
        let second = lines_edit(4, 4, "D");
        assert_eq!(
            apply_edits("a\nb\nc\nd\ne\n", &[&first, &second]).unwrap(),
            "a\nB1\nB2\nB3\nc\nD\ne\n"
        );
    }

    #[test]
    fn overlapping_edits_are_rejected_not_guessed() {
        let a = lines_edit(2, 3, "X");
        let b = lines_edit(3, 4, "Y");
        assert_eq!(
            apply_edits("1\n2\n3\n4\n5\n", &[&a, &b]),
            Err(ApplyError::Overlap {
                first: 1,
                second: 2
            })
        );
        // An insertion strictly inside a replaced span overlaps it too.
        let inside = insert_edit(3, "new");
        let span = lines_edit(2, 4, "Z");
        assert!(matches!(
            apply_edits("1\n2\n3\n4\n5\n", &[&span, &inside]),
            Err(ApplyError::Overlap { .. })
        ));
    }

    #[test]
    fn touching_edits_and_insertions_at_a_boundary_are_fine() {
        let a = lines_edit(2, 2, "B");
        let b = lines_edit(3, 3, "C");
        let ins = insert_edit(3, "between");
        // 2 and 3 are adjacent; the insertion sits between them.
        assert_eq!(
            apply_edits("a\nb\nc\nd\n", &[&a, &b, &ins]).unwrap(),
            "a\nB\nbetween\nC\nd\n"
        );
    }

    #[test]
    fn repeated_anchors_take_successive_occurrences_in_batch_order() {
        let first = text_edit("x", "1");
        let second = text_edit("x", "2");
        assert_eq!(apply_edits("x x x", &[&first, &second]).unwrap(), "1 2 x");
        // A third one for a text with only two occurrences cannot be placed.
        let third = text_edit("x", "3");
        assert_eq!(
            apply_edits("x x", &[&first, &second, &third]),
            Err(ApplyError::TargetNotFound)
        );
    }

    #[test]
    fn rewrite_cannot_be_mixed_with_other_edits() {
        let mut rewrite = edit(AgentOp::RewriteFile);
        rewrite.payload = Some("all new".into());
        let other = text_edit("a", "b");
        assert_eq!(
            apply_edits("a", &[&rewrite, &other]),
            Err(ApplyError::RewriteNotAlone)
        );
    }

    #[test]
    fn insert_text_anchor_and_line_edits_mix_in_one_batch() {
        let by_text = text_edit("beta", "BETA");
        let by_line = lines_edit(1, 1, "ALPHA");
        let appended = {
            let mut e = edit(AgentOp::Insert);
            e.insert_text = Some("tail".into());
            e
        };
        assert_eq!(
            apply_edits("alpha\nbeta\n", &[&by_text, &by_line, &appended]).unwrap(),
            "ALPHA\nBETA\ntail"
        );
    }

    #[test]
    fn two_hunks_of_one_diff_apply_together_and_share_context() {
        let mut e = edit(AgentOp::ApplyUnifiedDiff);
        e.payload =
            Some("@@ -1,3 +1,3 @@\n a\n-b\n+B\n c\n@@ -5,3 +5,3 @@\n e\n-f\n+F\n g\n".into());
        assert_eq!(
            apply_edit("a\nb\nc\nd\ne\nf\ng\n", &e).unwrap(),
            "a\nB\nc\nd\ne\nF\ng\n"
        );
    }

    #[test]
    fn a_diff_and_a_line_edit_on_the_same_line_conflict() {
        let mut diff = edit(AgentOp::ApplyUnifiedDiff);
        diff.payload = Some("@@ -1,2 +1,2 @@\n a\n-b\n+B\n".into());
        let clash = lines_edit(2, 2, "other");
        assert!(matches!(
            apply_edits("a\nb\nc\n", &[&diff, &clash]),
            Err(ApplyError::Overlap { .. })
        ));
        // But a diff and an edit on a different line are independent.
        let apart = lines_edit(3, 3, "C");
        assert_eq!(
            apply_edits("a\nb\nc\n", &[&diff, &apart]).unwrap(),
            "a\nB\nC\n"
        );
    }

    // ---- file format preservation -------------------------------------

    #[test]
    fn crlf_files_stay_crlf() {
        let content = "a\r\nb\r\nc\r\n";
        // Line edit: the planner's "\n" becomes the file's "\r\n".
        assert_eq!(
            apply_edit(content, &lines_edit(2, 2, "B1\nB2")).unwrap(),
            "a\r\nB1\r\nB2\r\nc\r\n"
        );
        // Text anchors written with "\n" still match.
        assert_eq!(
            apply_edit(content, &text_edit("a\nb", "X")).unwrap(),
            "X\r\nc\r\n"
        );
        assert_eq!(
            apply_edit(content, &insert_edit(2, "mid")).unwrap(),
            "a\r\nmid\r\nb\r\nc\r\n"
        );
        // Line matching in diffs ignores the carriage return.
        let mut diff = edit(AgentOp::ApplyUnifiedDiff);
        diff.payload = Some("@@ -1,3 +1,3 @@\n a\n-b\n+B\n c\n".into());
        assert_eq!(apply_edit(content, &diff).unwrap(), "a\r\nB\r\nc\r\n");
    }

    #[test]
    fn a_byte_order_mark_survives_every_kind_of_edit() {
        let content = "\u{feff}a\nb\n";
        assert_eq!(
            apply_edit(content, &lines_edit(1, 1, "A")).unwrap(),
            "\u{feff}A\nb\n"
        );
        assert_eq!(
            apply_edit(content, &insert_edit(1, "top")).unwrap(),
            "\u{feff}top\na\nb\n"
        );
        let mut rewrite = edit(AgentOp::RewriteFile);
        rewrite.payload = Some("new\n".into());
        assert_eq!(apply_edit(content, &rewrite).unwrap(), "\u{feff}new\n");
        // A payload that carries its own BOM does not double it.
        rewrite.payload = Some("\u{feff}new\n".into());
        assert_eq!(apply_edit(content, &rewrite).unwrap(), "\u{feff}new\n");
    }

    #[test]
    fn trailing_newline_state_is_preserved() {
        // No trailing newline: replacing or appending after the last line
        // must not invent one.
        assert_eq!(apply_edit("a\nb", &lines_edit(2, 2, "B")).unwrap(), "a\nB");
        assert_eq!(apply_edit("a\nb", &insert_edit(3, "c")).unwrap(), "a\nb\nc");
        // With one, it stays.
        assert_eq!(
            apply_edit("a\nb\n", &lines_edit(2, 2, "B")).unwrap(),
            "a\nB\n"
        );
        assert_eq!(
            apply_edit("a\nb\n", &insert_edit(3, "c")).unwrap(),
            "a\nb\nc\n"
        );
        // A rewrite whose payload forgot the final newline gets it back.
        let mut rewrite = edit(AgentOp::RewriteFile);
        rewrite.payload = Some("x".into());
        assert_eq!(apply_edit("a\n", &rewrite).unwrap(), "x\n");
    }

    #[test]
    fn deleting_lines_and_empty_files_behave() {
        assert_eq!(
            apply_edit("a\nb\nc\n", &lines_edit(2, 2, "")).unwrap(),
            "a\nc\n"
        );
        assert_eq!(apply_edit("", &insert_edit(1, "first")).unwrap(), "first");
        let mut append = edit(AgentOp::Insert);
        append.insert_text = Some("first".into());
        assert_eq!(apply_edit("", &append).unwrap(), "first");
        // An empty insert_text inserts a blank line, not nothing.
        assert_eq!(apply_edit("a\n", &insert_edit(1, "")).unwrap(), "\na\n");
    }

    // ---- multiple files -----------------------------------------------

    fn project() -> BTreeMap<String, String> {
        BTreeMap::from([
            ("main.tex".to_owned(), "\\input{ch1}\n".to_owned()),
            ("ch1.tex".to_owned(), "Hello\n".to_owned()),
            ("untouched.tex".to_owned(), "same\n".to_owned()),
        ])
    }

    fn for_file(path: &str, mut e: AgentEdit) -> AgentEdit {
        e.file_path = path.into();
        e
    }

    #[test]
    fn edits_are_routed_to_their_files_and_only_those_are_returned() {
        let edits = [
            for_file("ch1.tex", text_edit("Hello", "Hi")),
            for_file("main.tex", insert_edit(2, "\\end")),
        ];
        let out = apply_to_files(&project(), &edits).unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out["ch1.tex"], "Hi\n");
        assert_eq!(out["main.tex"], "\\input{ch1}\n\\end\n");
    }

    #[test]
    fn one_failing_file_fails_the_whole_batch_and_names_it() {
        let edits = [
            for_file("ch1.tex", text_edit("Hello", "Hi")),
            for_file("main.tex", text_edit("absent", "x")),
        ];
        let err = apply_to_files(&project(), &edits).unwrap_err();
        assert_eq!(
            err,
            ApplyError::InFile {
                path: "main.tex".into(),
                source: Box::new(ApplyError::TargetNotFound)
            }
        );
        assert!(err.to_string().starts_with("main.tex:"), "{err}");
    }

    #[test]
    fn an_edit_for_an_unknown_file_is_an_error_not_a_skip() {
        let edits = [for_file("nope.tex", text_edit("a", "b"))];
        assert_eq!(
            apply_to_files(&project(), &edits),
            Err(ApplyError::UnknownFile("nope.tex".into()))
        );
    }

    // ---- properties -----------------------------------------------------

    mod props {
        use super::*;
        use proptest::prelude::*;

        /// A file of `n` distinct lines plus, for a random subset of its
        /// lines, either a replacement or an insertion before it — never two
        /// edits on the same line, so the batch cannot overlap.
        fn scenario() -> impl Strategy<Value = (String, Vec<AgentEdit>)> {
            (3usize..14).prop_flat_map(|n| {
                let content: String = (1..=n).map(|i| format!("line {i}\n")).collect();
                proptest::collection::vec(0u8..3, n).prop_map(move |kinds| {
                    let edits = kinds
                        .iter()
                        .enumerate()
                        .filter_map(|(i, kind)| {
                            let line = i as u64 + 1;
                            match kind {
                                1 => Some(lines_edit(
                                    line,
                                    line,
                                    &format!("new {line}\nextra {line}"),
                                )),
                                2 => Some(insert_edit(line, &format!("ins {line}"))),
                                _ => None,
                            }
                        })
                        .collect();
                    (content.clone(), edits)
                })
            })
        }

        proptest! {
            #[test]
            fn batch_result_does_not_depend_on_edit_order(
                (content, edits) in scenario(),
                seed in any::<u64>(),
            ) {
                let refs: Vec<&AgentEdit> = edits.iter().collect();
                let expected = apply_edits(&content, &refs).unwrap();

                // Deterministic shuffle from `seed`.
                let mut shuffled = refs.clone();
                let mut state = seed | 1;
                for i in (1..shuffled.len()).rev() {
                    state ^= state << 13;
                    state ^= state >> 7;
                    state ^= state << 17;
                    shuffled.swap(i, (state % (i as u64 + 1)) as usize);
                }
                // Insertions at one point could differ, but this scenario
                // has at most one edit per line, so order cannot matter.
                prop_assert_eq!(apply_edits(&content, &shuffled).unwrap(), expected);
            }

            #[test]
            fn a_single_line_edit_changes_only_that_line(
                n in 2usize..12,
                pick in 0usize..100,
            ) {
                let content: String = (1..=n).map(|i| format!("l{i}\n")).collect();
                let line = (pick % n) as u64 + 1;
                let out = apply_edit(&content, &lines_edit(line, line, "CHANGED")).unwrap();
                let before: Vec<_> = content.lines().collect();
                let after: Vec<_> = out.lines().collect();
                prop_assert_eq!(before.len(), after.len());
                for (i, (b, a)) in before.iter().zip(&after).enumerate() {
                    if i as u64 + 1 == line {
                        prop_assert_eq!(*a, "CHANGED");
                    } else {
                        prop_assert_eq!(b, a);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod review_tests {
    use super::*;

    fn refs(edits: &[AgentEdit]) -> Vec<&AgentEdit> {
        edits.iter().collect()
    }

    fn roundtrip(original: &str, modified: &str) {
        let edits = edits_from_diff("a.tex", original, modified);
        assert_eq!(
            apply_edits(original, &refs(&edits)).unwrap(),
            modified,
            "edits: {edits:#?}"
        );
    }

    #[test]
    fn a_net_change_becomes_independent_original_numbered_hunks() {
        let original = "one\ntwo\nthree\nfour\nfive\n";
        let modified = "one\nTWO\nthree\nfour\nfive\nsix\n";
        let edits = edits_from_diff("a.tex", original, modified);

        assert_eq!(edits.len(), 2, "{edits:#?}");
        assert_eq!(edits[0].op, AgentOp::ReplaceRange);
        assert_eq!(
            (
                edits[0].start_line.map(std::num::NonZero::get),
                edits[0].end_line.map(std::num::NonZero::get)
            ),
            (Some(2), Some(2))
        );
        assert_eq!(edits[0].insert_text.as_deref(), Some("TWO\n"));
        assert_eq!(edits[1].op, AgentOp::Insert);
        assert_eq!(
            edits[1].line.map(std::num::NonZero::get),
            Some(6),
            "append after the last line"
        );

        // Each hunk applies on its own, which is what per-hunk review needs.
        assert_eq!(
            apply_edits(original, &[&edits[0]]).unwrap(),
            "one\nTWO\nthree\nfour\nfive\n"
        );
        assert_eq!(
            apply_edits(original, &[&edits[1]]).unwrap(),
            "one\ntwo\nthree\nfour\nfive\nsix\n"
        );
        roundtrip(original, modified);
    }

    #[test]
    fn deletions_insertions_and_replacements_round_trip() {
        roundtrip("a\nb\nc\n", "a\nc\n"); // delete
        roundtrip("a\nc\n", "a\nb\nc\n"); // insert
        roundtrip("a\nb\nc\n", "x\ny\nz\nw\n"); // replace everything
        roundtrip("a\nb\nc\n", "a\n\n\nc\n"); // blank lines
        roundtrip("a\nb", "a\nB"); // both without a final newline
        roundtrip("a\r\nb\r\nc\r\n", "a\r\nB1\r\nB2\r\nc\r\n"); // CRLF
    }

    #[test]
    fn identical_text_has_no_edits_and_an_empty_original_is_a_rewrite() {
        assert_eq!(edits_from_diff("a.tex", "same\n", "same\n").len(), 0);
        let edits = edits_from_diff("a.tex", "", "new\n");
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].op, AgentOp::RewriteFile);
        assert_eq!(apply_edits("", &refs(&edits)).unwrap(), "new\n");
    }

    mod props {
        use super::*;
        use proptest::prelude::*;

        fn text() -> impl Strategy<Value = String> {
            // Few distinct lines, so edits really do overlap and repeat.
            proptest::collection::vec(prop::sample::select(vec!["a", "b", "c", "", "d d"]), 1..14)
                .prop_map(|lines| lines.iter().map(|l| format!("{l}\n")).collect())
        }

        proptest! {
            #[test]
            fn applying_the_diff_edits_reproduces_the_new_text(a in text(), b in text()) {
                let edits = edits_from_diff("a.tex", &a, &b);
                prop_assert_eq!(apply_edits(&a, &refs(&edits)).unwrap(), b);
            }

            /// Whenever a line edit is moved, it lands on the same text.
            #[test]
            fn a_rebased_range_points_at_the_lines_it_pointed_at(
                base in text(),
                current in text(),
                pick in 0usize..100,
                width in 1usize..4,
            ) {
                let n = base.lines().count();
                let start = pick % n;
                let end = (start + width).min(n);
                let edit = {
                    let mut e = edits_from_diff("a.tex", "x\n", "y\n").remove(0);
                    e.start_line = std::num::NonZeroU64::new(start as u64 + 1);
                    e.end_line = std::num::NonZeroU64::new(end as u64);
                    e
                };
                if let Ok(moved) = rebase_edits(&base, &current, &[edit]) {
                    let s = moved[0].start_line.unwrap().get() as usize - 1;
                    let e = moved[0].end_line.unwrap().get() as usize;
                    let before: Vec<&str> = base.lines().collect();
                    let after: Vec<&str> = current.lines().collect();
                    prop_assert_eq!(&before[start..end], &after[s..e]);
                }
            }
        }
    }

    fn line_edit(start: u64, end: u64, with: &str) -> AgentEdit {
        let mut e = edits_from_diff("a.tex", "x\n", "y\n").remove(0);
        e.start_line = std::num::NonZeroU64::new(start);
        e.end_line = std::num::NonZeroU64::new(end);
        e.insert_text = Some(with.to_owned());
        e
    }

    #[test]
    fn rebase_moves_line_edits_past_unrelated_changes() {
        let base = "1\n2\n3\n4\n5\n";
        // Someone added two lines at the top and changed line 5.
        let current = "new a\nnew b\n1\n2\n3\n4\nFIVE\n";

        let moved = rebase_edits(base, current, &[line_edit(2, 3, "X")]).unwrap();
        assert_eq!(moved[0].start_line.map(std::num::NonZero::get), Some(4));
        assert_eq!(moved[0].end_line.map(std::num::NonZero::get), Some(5));
        assert_eq!(
            apply_edits(current, &refs(&moved)).unwrap(),
            "new a\nnew b\n1\nX\n4\nFIVE\n"
        );
    }

    #[test]
    fn rebase_refuses_edits_on_lines_that_changed() {
        let base = "1\n2\n3\n";
        let current = "1\nTWO\n3\n";
        assert_eq!(
            rebase_edits(
                base,
                current,
                &[line_edit(3, 3, "ok"), line_edit(2, 3, "clash")]
            ),
            Err(RebaseError::Conflict { edit: 2 })
        );
        // A range that the intervening edit split in two is also a conflict.
        let split = "1\n2\nINSERTED\n3\n";
        assert!(rebase_edits(base, split, &[line_edit(2, 3, "x")]).is_err());
    }

    #[test]
    fn rebase_handles_insertions_and_leaves_text_anchors_alone() {
        let base = "a\nb\n";
        let current = "z\na\nb\n";
        let mut insert = line_edit(1, 1, "");
        insert.op = AgentOp::Insert;
        insert.start_line = None;
        insert.end_line = None;
        insert.line = std::num::NonZeroU64::new(2);
        insert.insert_text = Some("mid".to_owned());
        let append = {
            let mut e = insert.clone();
            e.line = std::num::NonZeroU64::new(3); // after the last base line
            e
        };
        let anchored = {
            let mut e = line_edit(1, 1, "B");
            e.start_line = None;
            e.end_line = None;
            e.text = Some("b".to_owned());
            e
        };

        let moved = rebase_edits(base, current, &[insert, append, anchored]).unwrap();
        assert_eq!(moved[0].line.map(std::num::NonZero::get), Some(3));
        assert_eq!(
            moved[1].line.map(std::num::NonZero::get),
            Some(4),
            "still an append"
        );
        assert_eq!(moved[2].text.as_deref(), Some("b"));
        assert_eq!(
            apply_edits(current, &refs(&moved)).unwrap(),
            "z\na\nmid\nB\nmid\n"
        );
    }

    #[test]
    fn rebase_moves_whole_file_edits_only_if_the_file_is_untouched() {
        let mut rewrite = line_edit(1, 1, "");
        rewrite.op = AgentOp::RewriteFile;
        rewrite.payload = Some("all new\n".to_owned());
        assert!(rebase_edits("same\n", "same\n", &[rewrite.clone()]).is_ok());
        assert!(rebase_edits("same\n", "different\n", &[rewrite]).is_err());
    }

    #[test]
    fn rebase_clears_request_metadata_so_ids_are_recomputed() {
        let mut e = line_edit(1, 1, "z");
        e.expected_etag = Some("old".to_owned());
        e.hunk_id = Some("stale".to_owned());
        let moved = rebase_edits("a\n", "a\n", &[e]).unwrap();
        assert!(moved[0].expected_etag.is_none() && moved[0].hunk_id.is_none());
    }
}
