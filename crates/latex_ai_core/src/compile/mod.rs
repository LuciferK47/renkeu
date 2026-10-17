//! Wire types shared between the AI planner, scratchpad, storage, and routes.

use serde::{Deserialize, Serialize};
use std::num::NonZeroU64;

/// Kind of change an [`AgentEdit`] performs. Semantics live in
/// [`crate::apply`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
#[non_exhaustive]
pub enum AgentOp {
    RewriteFile,
    ReplaceRange,
    Insert,
    ApplyUnifiedDiff,
}

impl AgentOp {
    /// The wire name, also fed into [`AgentEdit::hunk_id`].
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            Self::RewriteFile => "rewrite_file",
            Self::ReplaceRange => "replace_range",
            Self::Insert => "insert",
            Self::ApplyUnifiedDiff => "apply_unified_diff",
        }
    }
}

/// One structured edit proposed by the AI (masterplan.md §8.1).
///
/// `hunk_id` is server-computed (see [`AgentEdit::hunk_id`]) but remains a
/// plain optional field: proposals are sent to the client WITH it, and the
/// client echoes edits back verbatim on apply-selected — a
/// `skip_deserializing` attribute here would make `deny_unknown_fields`
/// reject that round-trip.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AgentEdit {
    pub file_path: String,
    pub op: AgentOp,
    pub payload: Option<String>,
    pub text: Option<String>,
    pub start_line: Option<NonZeroU64>,
    pub end_line: Option<NonZeroU64>,
    pub line: Option<NonZeroU64>,
    pub insert_text: Option<String>,
    pub expected_etag: Option<String>,
    #[serde(default)]
    pub hunk_id: Option<String>,
}

/// Identity of an edit: blake3 over everything that defines what it does,
/// first 16 hex chars (masterplan.md §13). Two edits share an id only if they
/// are the same change to the same file.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, Hash)]
pub struct HunkId(pub String);

/// Why an [`AgentEdit`] is not usable. Routes surface these as 422 rather
/// than discovering the problem half-way through applying a batch.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum EditValidationError {
    #[error("file_path must not be empty")]
    EmptyFilePath,
    #[error("start_line {start} is after end_line {end}")]
    InvertedRange { start: u64, end: u64 },
    #[error("{op} requires `{field}`")]
    MissingField {
        op: &'static str,
        field: &'static str,
    },
}

impl AgentEdit {
    /// Identity of this edit: file, operation, anchor (search text and/or
    /// line numbers), replacement text and payload.
    ///
    /// The id used to be a hash of the search text alone, which gave every
    /// line-based edit and every insertion the same id (an empty anchor) and
    /// made two different replacements of one anchor indistinguishable, so
    /// selecting one hunk selected them all. `expected_etag` and `hunk_id`
    /// are deliberately excluded: they describe the request, not the change.
    #[must_use]
    pub fn hunk_id(&self) -> HunkId {
        let mut hasher = blake3::Hasher::new();
        let mut field = |bytes: Option<&[u8]>| match bytes {
            // A presence byte and a length keep ("ab","c") distinct from
            // ("a","bc") and None distinct from Some("").
            Some(b) => {
                hasher.update(&[1]);
                hasher.update(&(b.len() as u64).to_le_bytes());
                hasher.update(b);
            }
            None => {
                hasher.update(&[0]);
            }
        };
        let num = |n: Option<NonZeroU64>| n.map(|n| n.get().to_le_bytes());

        field(Some(self.file_path.as_bytes()));
        field(Some(self.op.tag().as_bytes()));
        field(self.text.as_deref().map(str::as_bytes));
        field(self.payload.as_deref().map(str::as_bytes));
        field(self.insert_text.as_deref().map(str::as_bytes));
        field(num(self.start_line).as_ref().map(<[u8; 8]>::as_slice));
        field(num(self.end_line).as_ref().map(<[u8; 8]>::as_slice));
        field(num(self.line).as_ref().map(<[u8; 8]>::as_slice));

        let hex = hasher.finalize().to_hex();
        HunkId(hex[..16].to_owned())
    }

    /// Structural check before anything touches the filesystem
    /// (masterplan.md §12 M4 audit: security).
    ///
    /// # Errors
    /// [`EditValidationError`] naming the first problem found.
    pub fn validate(&self) -> Result<(), EditValidationError> {
        if self.file_path.trim().is_empty() {
            return Err(EditValidationError::EmptyFilePath);
        }

        // A reversed range would otherwise be caught only at apply time, or
        // silently splice nothing.
        if let (Some(start), Some(end)) = (self.start_line, self.end_line) {
            if start > end {
                return Err(EditValidationError::InvertedRange {
                    start: start.get(),
                    end: end.get(),
                });
            }
        }

        let missing = |op, field| Err(EditValidationError::MissingField { op, field });
        match self.op {
            AgentOp::RewriteFile if self.payload.is_none() => missing("rewrite_file", "payload"),
            AgentOp::ApplyUnifiedDiff if self.payload.is_none() => {
                missing("apply_unified_diff", "payload")
            }
            AgentOp::Insert if self.insert_text.is_none() => missing("insert", "insert_text"),
            // replace_range needs either a line range or search text; the
            // range case is covered above.
            AgentOp::ReplaceRange
                if self.text.is_none()
                    && (self.start_line.is_none() || self.end_line.is_none()) =>
            {
                missing("replace_range", "text or start_line/end_line")
            }
            _ => Ok(()),
        }
    }
}

/// Collapse edits that share a [`HunkId`], keeping first occurrence order.
///
/// Two prompts asking for the same change produce identical hunks, and
/// applying one twice would either fail on the second pass or double-apply.
/// Only truly identical edits collapse: different replacements of the same
/// anchor are different changes and both survive.
#[must_use]
pub fn dedupe_hunks<'a>(edits: &[&'a AgentEdit]) -> Vec<&'a AgentEdit> {
    let mut seen = std::collections::HashSet::new();
    edits
        .iter()
        .filter(|edit| seen.insert(edit.hunk_id()))
        .copied()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hunk_id_is_stable_and_16_hex() {
        let edit: AgentEdit =
            serde_json::from_str(r#"{"file_path":"a.tex","op":"replace_range","text":"same"}"#)
                .unwrap();
        let a = edit.hunk_id();
        let b = edit.hunk_id();
        assert_eq!(a, b);
        assert_eq!(a.0.len(), 16);
        assert!(a.0.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn client_echo_with_hunk_id_still_deserializes() {
        // Round-trip: server serializes hunk_id into proposals; the client
        // echoes it back on apply-selected. Must NOT be an unknown field.
        let json = r#"{"file_path":"a.tex","op":"insert","insert_text":"x","hunk_id":"abcd"}"#;
        let edit: AgentEdit = serde_json::from_str(json).unwrap();
        assert_eq!(edit.hunk_id.as_deref(), Some("abcd"));
    }

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

    fn nz(v: u64) -> std::num::NonZeroU64 {
        std::num::NonZeroU64::new(v).unwrap()
    }

    #[test]
    fn rejects_an_inverted_line_range() {
        let mut e = edit(AgentOp::ReplaceRange);
        e.start_line = Some(nz(9));
        e.end_line = Some(nz(4));
        e.insert_text = Some("x".into());
        assert_eq!(
            e.validate(),
            Err(EditValidationError::InvertedRange { start: 9, end: 4 })
        );

        // Equal bounds are a valid single-line range.
        e.start_line = Some(nz(4));
        assert!(e.validate().is_ok());
    }

    #[test]
    fn rejects_missing_op_fields_and_empty_paths() {
        assert!(matches!(
            edit(AgentOp::RewriteFile).validate(),
            Err(EditValidationError::MissingField { .. })
        ));
        assert!(matches!(
            edit(AgentOp::Insert).validate(),
            Err(EditValidationError::MissingField { .. })
        ));
        assert!(matches!(
            edit(AgentOp::ReplaceRange).validate(),
            Err(EditValidationError::MissingField { .. })
        ));

        let mut blank = edit(AgentOp::Insert);
        blank.insert_text = Some("x".into());
        blank.file_path = "  ".into();
        assert_eq!(blank.validate(), Err(EditValidationError::EmptyFilePath));
    }

    fn replace(text: &str, with: &str) -> AgentEdit {
        let mut e = edit(AgentOp::ReplaceRange);
        e.text = Some(text.into());
        e.insert_text = Some(with.into());
        e
    }

    #[test]
    fn only_truly_identical_hunks_collapse() {
        let a = replace("same anchor", "first");
        let duplicate = a.clone();
        let other_replacement = replace("same anchor", "second");
        let other_anchor = replace("different anchor", "third");

        assert_eq!(a.hunk_id(), duplicate.hunk_id());
        assert_ne!(a.hunk_id(), other_replacement.hunk_id());

        let deduped = dedupe_hunks(&[&a, &duplicate, &other_replacement, &other_anchor]);
        let replacements: Vec<_> = deduped.iter().map(|e| e.insert_text.as_deref()).collect();
        assert_eq!(replacements, [Some("first"), Some("second"), Some("third")]);
    }

    #[test]
    fn hunk_ids_are_stable_across_replans() {
        // The same plan produced twice yields the same ids, which is what
        // lets a user's selection survive a requery.
        assert_eq!(replace("\\textwidth", "a").hunk_id(), replace("\\textwidth", "a").hunk_id());
    }

    #[test]
    fn line_based_edits_and_insertions_no_longer_collide() {
        // Before: every edit without `text` hashed the empty string, so all
        // of these shared one id and selecting one selected all.
        let mut by_lines_a = edit(AgentOp::ReplaceRange);
        by_lines_a.start_line = Some(nz(1));
        by_lines_a.end_line = Some(nz(2));
        by_lines_a.insert_text = Some("x".into());
        let mut by_lines_b = by_lines_a.clone();
        by_lines_b.start_line = Some(nz(5));
        by_lines_b.end_line = Some(nz(6));
        let mut ins_a = edit(AgentOp::Insert);
        ins_a.line = Some(nz(1));
        ins_a.insert_text = Some("x".into());
        let mut ins_b = ins_a.clone();
        ins_b.line = Some(nz(2));
        let mut other_file = ins_a.clone();
        other_file.file_path = "chapter.tex".into();

        let ids = [&by_lines_a, &by_lines_b, &ins_a, &ins_b, &other_file]
            .map(|e| e.hunk_id());
        let unique: std::collections::HashSet<_> = ids.iter().collect();
        assert_eq!(unique.len(), ids.len(), "{ids:?}");
    }

    #[test]
    fn field_boundaries_cannot_be_forged() {
        let mut a = edit(AgentOp::ReplaceRange);
        a.text = Some("ab".into());
        a.insert_text = Some("c".into());
        let mut b = edit(AgentOp::ReplaceRange);
        b.text = Some("a".into());
        b.insert_text = Some("bc".into());
        assert_ne!(a.hunk_id(), b.hunk_id());

        // None is not the same as an empty string.
        let mut none = edit(AgentOp::Insert);
        none.insert_text = Some(String::new());
        let mut empty_payload = none.clone();
        empty_payload.payload = Some(String::new());
        assert_ne!(none.hunk_id(), empty_payload.hunk_id());
    }

    #[test]
    fn request_metadata_does_not_change_identity() {
        let a = replace("x", "y");
        let mut b = a.clone();
        b.expected_etag = Some("etag".into());
        b.hunk_id = Some("whatever the client echoed".into());
        assert_eq!(a.hunk_id(), b.hunk_id());
    }

    #[test]
    fn unknown_fields_are_still_rejected() {
        let json = r#"{"file_path":"a.tex","op":"insert","bogus":1}"#;
        assert!(serde_json::from_str::<AgentEdit>(json).is_err());
    }
}
