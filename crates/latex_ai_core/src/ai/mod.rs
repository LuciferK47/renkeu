//! LLM edit-planning service.
//!
//! Provider-neutral: `genai` picks the provider adapter from the model name
//! (e.g. `llama-3.3-70b-versatile` -> Groq, authenticated via `GROQ_API_KEY`).
//! Planning is one-shot per call — iteration context (compile errors) is folded
//! into the instruction by the agent loop, so no chat history lives here.

use crate::compile::AgentEdit;
use genai::Client;
use genai::chat::{ChatMessage, ChatRequest, MessageContent};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum AiError {
    #[error("AI request failed: {0}")]
    Request(String),
    #[error("AI response parsing failed: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("AI operation cancelled")]
    Cancelled,
    #[error("no content in AI response")]
    EmptyResponse,
}

/// Boxed future so [`EditPlanner`] stays object-safe: routes hold an
/// `Arc<dyn EditPlanner>`, letting tests inject a scripted planner
/// (masterplan.md §12 M2 tests).
pub type PlanFuture<'a> =
    Pin<Box<dyn Future<Output = Result<Vec<AgentEdit>, AiError>> + Send + 'a>>;

/// Something that can turn an instruction into a batch of [`AgentEdit`]s.
pub trait EditPlanner: Send + Sync {
    /// Propose edits fulfilling `instruction` against `file_content`.
    fn plan_edits<'a>(
        &'a self,
        instruction: &'a str,
        file_content: &'a str,
        cancel: CancellationToken,
    ) -> PlanFuture<'a>;

    /// Propose edits for a visual defect. `b64_image` is a base64 PNG crop of
    /// the PDF region; `file_path`/`line` are the `SyncTeX` hit when known.
    fn visual_fix<'a>(
        &'a self,
        instruction: &'a str,
        b64_image: &'a str,
        file_path: Option<&'a str>,
        line: Option<u32>,
        cancel: CancellationToken,
    ) -> PlanFuture<'a>;
}

const SYSTEM_PROMPT: &str = r#"You are a LaTeX expert. You must output ONLY a JSON array of AgentEdit objects. Do not include markdown codeblocks or any other text.
Each AgentEdit looks like:
{
  "file_path": "main.tex",
  "op": "replace_range",
  "text": "exact text currently in the file",
  "insert_text": "replacement text"
}
Rules:
- "text" must be copied verbatim from the file content you were given.
- Use op "rewrite_file" with "payload" holding the full new file only when a
  targeted replacement is impossible.
- Propose the smallest edits that satisfy the instruction.
"#;

/// Planner backed by a remote LLM through [`genai`].
pub struct LlmPlanner {
    client: Arc<Client>,
    model: String,
}

impl LlmPlanner {
    #[must_use]
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            client: Arc::new(Client::builder().build()),
            model: model.into(),
        }
    }

    /// Which model this planner sends requests to.
    #[must_use]
    pub fn model(&self) -> &str {
        &self.model
    }

    async fn request_edits(
        &self,
        user_prompt: String,
        cancel: CancellationToken,
    ) -> Result<Vec<AgentEdit>, AiError> {
        let req = ChatRequest::new(vec![
            ChatMessage::system(SYSTEM_PROMPT),
            ChatMessage::user(user_prompt),
        ]);

        let res = tokio::select! {
            r = self.client.exec_chat(&self.model, req, None) => {
                r.map_err(|e| AiError::Request(e.to_string()))?
            }
            () = cancel.cancelled() => return Err(AiError::Cancelled),
        };

        if let Some(MessageContent::Text(text)) = res.content {
            Ok(serde_json::from_str(extract_json(&text))?)
        } else {
            Err(AiError::EmptyResponse)
        }
    }
}

/// Strip an optional markdown code fence — models occasionally wrap the JSON
/// despite the system prompt.
fn extract_json(text: &str) -> &str {
    text.trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim()
}

impl EditPlanner for LlmPlanner {
    fn plan_edits<'a>(
        &'a self,
        instruction: &'a str,
        file_content: &'a str,
        cancel: CancellationToken,
    ) -> PlanFuture<'a> {
        Box::pin(async move {
            let user_prompt = format!(
                "Instruction: {instruction}\n\nFile Content:\n```latex\n{file_content}\n```"
            );
            self.request_edits(user_prompt, cancel).await
        })
    }

    fn visual_fix<'a>(
        &'a self,
        instruction: &'a str,
        b64_image: &'a str,
        file_path: Option<&'a str>,
        line: Option<u32>,
        cancel: CancellationToken,
    ) -> PlanFuture<'a> {
        Box::pin(async move {
            let file_context = file_path.unwrap_or("unknown.tex");
            let line_context = line.map_or_else(|| "unknown".to_string(), |l| l.to_string());
            let user_prompt = format!(
                "Instruction: {instruction}\n\nFile Context: {file_context}\nLine Context: {line_context}\n\nImage Base64 (cropped area of issue):\n{b64_image}"
            );
            self.request_edits(user_prompt, cancel).await
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_json_strips_fences_and_whitespace() {
        assert_eq!(extract_json("[]"), "[]");
        assert_eq!(extract_json("```json\n[1]\n```"), "[1]");
        assert_eq!(extract_json("```\n[2]\n```"), "[2]");
        assert_eq!(extract_json("  [3]  "), "[3]");
    }

    #[test]
    fn edits_parse_from_model_shaped_output() {
        let text = r#"```json
[{"file_path":"main.tex","op":"replace_range","text":"a","insert_text":"b"}]
```"#;
        let edits: Vec<AgentEdit> = serde_json::from_str(extract_json(text)).unwrap();
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].file_path, "main.tex");
    }
}
