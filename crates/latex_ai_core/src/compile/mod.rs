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

    #[test]
    fn unknown_fields_are_still_rejected() {
        let json = r#"{"file_path":"a.tex","op":"insert","bogus":1}"#;
        assert!(serde_json::from_str::<AgentEdit>(json).is_err());
    }
}
