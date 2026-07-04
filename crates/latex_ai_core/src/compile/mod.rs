use serde::{Deserialize, Serialize};
use std::num::NonZeroU64;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
#[non_exhaustive]
pub enum AgentOp {
    RewriteFile,
    ReplaceRange,
    Insert,
    ApplyUnifiedDiff,
}

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
    #[serde(skip_deserializing)]
    pub hunk_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, Hash)]
pub struct HunkId(pub String);

impl AgentEdit {
    pub fn search_text(&self) -> &str {
        if let Some(text) = &self.text {
            return text;
        }
        if let Some(text) = &self.payload {
            return text;
        }
        ""
    }
    
    pub fn hunk_id(&self) -> HunkId {
        let hash = blake3::hash(self.search_text().as_bytes());
        let hex = hash.to_hex();
        HunkId(hex[..16].to_string())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CompileRequest {
    pub session_id: String,
    pub edits: Vec<AgentEdit>,
}
