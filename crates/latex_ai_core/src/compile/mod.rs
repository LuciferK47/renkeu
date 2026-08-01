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

/// One structured edit proposed by the AI (masterplan.md §8.1).
///
/// `hunk_id` is server-computed (blake3 of the search text) but remains a
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

/// Stable identity of an edit across replans: blake3 of the search text,
/// first 16 hex chars (masterplan.md §13).
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
    /// The text this edit anchors on (search text, else whole payload).
    #[must_use]
    pub fn search_text(&self) -> &str {
        if let Some(text) = &self.text {
            return text;
        }
        if let Some(payload) = &self.payload {
            return payload;
        }
        ""
    }

    #[must_use]
    pub fn hunk_id(&self) -> HunkId {
        let hash = blake3::hash(self.search_text().as_bytes());
        let hex = hash.to_hex();
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
/// Two prompts asking for the same change produce byte-identical hunks
/// (the id is a hash of the anchor text), and applying one twice would
/// either fail on the second pass or double-apply. Collapsing is the
/// behaviour masterplan §12 M4's audit gate asks for.
#[must_use]
pub fn dedupe_hunks<'a>(edits: &[&'a AgentEdit]) -> Vec<&'a AgentEdit> {
    let mut seen = std::collections::HashSet::new();
    edits
        .iter()
        .filter(|edit| seen.insert(edit.hunk_id()))
        .copied()
        .collect()
}

/// A compile job bound to a scratchpad session.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CompileRequest {
    pub session_id: String,
    pub edits: Vec<AgentEdit>,
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

    #[test]
    fn identical_hunks_collapse_but_distinct_ones_survive() {
        let mut a = edit(AgentOp::ReplaceRange);
        a.text = Some("same anchor".into());
        a.insert_text = Some("first".into());
        // Same anchor text -> same hunk id, even with a different replacement.
        let mut duplicate = a.clone();
        duplicate.insert_text = Some("second".into());
        let mut other = edit(AgentOp::ReplaceRange);
        other.text = Some("different anchor".into());
        other.insert_text = Some("third".into());

        assert_eq!(a.hunk_id(), duplicate.hunk_id());
        let deduped = dedupe_hunks(&[&a, &duplicate, &other]);
        assert_eq!(deduped.len(), 2);
        assert_eq!(deduped[0].insert_text.as_deref(), Some("first"));
        assert_eq!(deduped[1].insert_text.as_deref(), Some("third"));
    }

    #[test]
    fn hunk_ids_are_stable_across_replans() {
        // The same plan produced twice must yield the same id set, which is
        // what lets a user's selection survive a requery.
        let plan = |replacement: &str| {
            let mut e = edit(AgentOp::ReplaceRange);
            e.text = Some("\\textwidth".into());
            e.insert_text = Some(replacement.to_owned());
            e
        };
        assert_eq!(plan("a").hunk_id(), plan("b").hunk_id());
    }

    #[test]
    fn unknown_fields_are_still_rejected() {
        let json = r#"{"file_path":"a.tex","op":"insert","bogus":1}"#;
        assert!(serde_json::from_str::<AgentEdit>(json).is_err());
    }
}
