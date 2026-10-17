//! LLM edit-planning service.
//!
//! Provider-neutral: `genai` picks the provider adapter from the model name
//! (e.g. `llama-3.3-70b-versatile` -> Groq, authenticated via `GROQ_API_KEY`).
//! Planning is one-shot per call — iteration context (compile errors) is folded
//! into the instruction by the agent loop, so no chat history lives here.

use crate::compile::AgentEdit;
use crate::config::ApiKey;
use genai::adapter::AdapterKind;
use genai::resolver::AuthData;
use genai::{Client, ModelIden};
use genai::chat::{
    ChatMessage, ChatOptions, ChatRequest, ChatResponseFormat, ContentPart, MessageContent,
};
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
    /// No API key is configured, so there is no model to ask.
    #[error("AI is not configured: add an API key in Settings")]
    Disabled,
    #[error("AI request timed out after {secs}s")]
    Timeout { secs: u64 },
}

/// Boxed future so [`EditPlanner`] stays object-safe: routes hold an
/// `Arc<dyn EditPlanner>`, letting tests inject a scripted planner
/// (masterplan.md §12 M2 tests).
pub type PlanFuture<'a> =
    Pin<Box<dyn Future<Output = Result<Vec<AgentEdit>, AiError>> + Send + 'a>>;

/// Something that can turn an instruction into a batch of [`AgentEdit`]s.
pub trait EditPlanner: Send + Sync {
    /// Whether this planner can currently serve requests. Routes check it
    /// before opening a stream so a missing API key is a clean 503, not a
    /// 200 stream that ends in an error event.
    fn is_available(&self) -> bool {
        true
    }

    /// Propose edits fulfilling `instruction` against `file_content`.
    fn plan_edits<'a>(
        &'a self,
        instruction: &'a str,
        file_content: &'a str,
        cancel: CancellationToken,
    ) -> PlanFuture<'a>;

    /// Propose edits for a visual defect shown in a PDF crop (M3).
    fn visual_fix<'a>(
        &'a self,
        req: VisualFixRequest<'a>,
        cancel: CancellationToken,
    ) -> PlanFuture<'a>;
}

/// Everything the planner needs to reason about a visual defect: what the user
/// asked, what the region looks like, and the source that produced it.
#[derive(Debug, Clone, Copy)]
pub struct VisualFixRequest<'a> {
    pub instruction: &'a str,
    /// Base64 PNG of the cropped PDF region.
    pub png_base64: &'a str,
    /// Source file the `SyncTeX` hit resolved to, when known.
    pub file_path: Option<&'a str>,
    /// 1-based source line of the hit, when known.
    pub line: Option<u32>,
    /// Source text around `line` for context.
    pub snippet: &'a str,
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
///
/// Text planning and vision use separate models (masterplan §2.1 pins a
/// distinct VLM): a text-only model rejects image parts outright, and not
/// every provider offers a vision model at all. `genai` routes each by name,
/// so the vision model may live with a different provider than the text one.
pub struct LlmPlanner {
    client: Arc<Client>,
    model: String,
    vision_model: Option<String>,
}

impl LlmPlanner {
    /// Text-only planner. [`Self::visual_fix`] will degrade to a description
    /// of the region rather than sending the crop.
    #[must_use]
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            client: Arc::new(Client::builder().build()),
            model: model.into(),
            vision_model: None,
        }
    }

    /// Planner that authenticates with `key` instead of reading the
    /// environment, which is what lets the key be changed at runtime.
    ///
    /// The key is offered only to Groq models: a vision model on another
    /// provider must not receive a credential issued for Groq.
    #[must_use]
    pub fn with_api_key(model: impl Into<String>, key: &ApiKey) -> Self {
        let secret = key.expose().to_owned();
        let client = Client::builder()
            .with_auth_resolver_fn(move |model_iden: ModelIden| {
                Ok((model_iden.adapter_kind == AdapterKind::Groq)
                    .then(|| AuthData::from_single(secret)))
            })
            .build();
        Self {
            client: Arc::new(client),
            model: model.into(),
            vision_model: None,
        }
    }

    /// Route [`Self::visual_fix`] to a vision-capable model, which is what
    /// lets it actually see the PDF crop.
    #[must_use]
    pub fn with_vision_model(mut self, model: Option<String>) -> Self {
        self.vision_model = model.filter(|m| !m.trim().is_empty());
        self
    }

    /// Which model this planner sends text requests to.
    #[must_use]
    pub fn model(&self) -> &str {
        &self.model
    }

    /// The vision model, if one is configured.
    #[must_use]
    pub fn vision_model(&self) -> Option<&str> {
        self.vision_model.as_deref()
    }

    async fn request_edits(
        &self,
        user_prompt: String,
        cancel: CancellationToken,
    ) -> Result<Vec<AgentEdit>, AiError> {
        self.request(&self.model, MessageContent::from_text(user_prompt), cancel)
            .await
    }

    /// Send a user message (text, or text + image parts) and decode the reply
    /// as a strict `AgentEdit` array.
    async fn request(
        &self,
        model: &str,
        content: MessageContent,
        cancel: CancellationToken,
    ) -> Result<Vec<AgentEdit>, AiError> {
        let req = ChatRequest::new(vec![
            ChatMessage::system(SYSTEM_PROMPT),
            ChatMessage::user(content),
        ]);
        // Ask the provider to constrain output to JSON. The system prompt
        // still spells out the shape — JsonMode guarantees syntax, not schema.
        let options = ChatOptions::default().with_response_format(ChatResponseFormat::JsonMode);

        let res = tokio::select! {
            r = self.client.exec_chat(model, req, Some(&options)) => {
                r.map_err(|e| AiError::Request(e.to_string()))?
            }
            () = cancel.cancelled() => return Err(AiError::Cancelled),
        };

        let Some(MessageContent::Text(text)) = res.content else {
            return Err(AiError::EmptyResponse);
        };
        parse_edits(&text)
    }
}

/// Decode the model's reply into edits.
///
/// OpenAI-style JSON mode forces the response to be a top-level *object*, so a
/// bare array is literally impossible for those providers no matter what the
/// prompt says. Three shapes are therefore accepted:
///
/// 1. `[ {edit}, … ]`               — what the prompt asks for;
/// 2. `{ "edits": [ {edit}, … ] }`  — the usual JSON-mode wrapping;
/// 3. `{edit}`                      — a lone edit, which Groq returns for a
///    single-change request.
///
/// Anything else is a shape error the route reports as 422.
fn parse_edits(text: &str) -> Result<Vec<AgentEdit>, AiError> {
    let json = extract_json(text);

    // The direct decode is the documented path; keep its error to report if
    // every fallback also fails.
    let direct = match serde_json::from_str::<Vec<AgentEdit>>(json) {
        Ok(edits) => return Ok(edits),
        Err(e) => e,
    };

    let value: serde_json::Value = serde_json::from_str(json)?;

    if let Some(array) = value
        .get("edits")
        .or_else(|| value.get("agent_edits"))
        .or_else(|| value.get("changes"))
    {
        return Ok(serde_json::from_value(array.clone())?);
    }

    if let Ok(single) = serde_json::from_value::<AgentEdit>(value) {
        return Ok(vec![single]);
    }

    Err(AiError::Parse(direct))
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
        req: VisualFixRequest<'a>,
        cancel: CancellationToken,
    ) -> PlanFuture<'a> {
        Box::pin(async move {
            let VisualFixRequest {
                instruction,
                png_base64,
                file_path,
                line,
                snippet,
            } = req;

            let file_context = file_path.unwrap_or("the document");
            let line_context = line.map_or_else(|| "unknown".to_owned(), |l| l.to_string());

            // With a vision model the crop goes as a real image part. Without
            // one, sending base64 in the prompt text would be pure noise — no
            // model can see it — so we degrade honestly to the source context
            // and say so.
            let (model, content) = if let Some(vision) = self.vision_model.as_deref() {
                {
                    let prompt = format!(
                        "Instruction: {instruction}\n\n\
                         The attached image is a crop of the rendered PDF showing the problem area.\n\
                         It was produced by {file_context} around line {line_context}.\n\n\
                         Source around that line:\n```latex\n{snippet}\n```\n\n\
                         Return edits that change ONLY what is needed to fix what the image shows."
                    );
                    let content = MessageContent::from_parts(vec![
                        ContentPart::from_text(prompt),
                        ContentPart::from_image_base64("image/png", png_base64.to_owned()),
                    ]);
                    (vision, content)
                }
            } else {
                {
                    tracing::info!(
                        "no vision model configured (ai.vision_model / LATEX_AI_VISION_MODEL); \
                         falling back to a text-only visual fix"
                    );
                    let prompt = format!(
                        "Instruction: {instruction}\n\n\
                         The user selected a region of the rendered PDF produced by \
                         {file_context} around line {line_context}. The rendered image is \
                         not available to you, so reason from the source alone.\n\n\
                         Source around that line:\n```latex\n{snippet}\n```\n\n\
                         Return edits that change ONLY what is needed to fix a layout \
                         problem at that location."
                    );
                    (self.model.as_str(), MessageContent::from_text(prompt))
                }
            };

            self.request(model, content, cancel).await
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
    fn accepts_a_wrapped_edits_array() {
        // OpenAI-style JSON mode forces an object wrapper.
        let json = r#"{"edits":[{"file_path":"a.tex","op":"insert","insert_text":"x"}]}"#;
        assert_eq!(parse_edits(json).unwrap().len(), 1);
    }

    #[test]
    fn accepts_a_lone_edit_object() {
        // What Groq actually returns for a single-change request under
        // response_format=json_object — a bare array is impossible there.
        let json =
            r#"{"file_path":"main.tex","op":"replace_range","text":"old","insert_text":"new"}"#;
        let edits = parse_edits(json).unwrap();
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].file_path, "main.tex");
        assert_eq!(edits[0].insert_text.as_deref(), Some("new"));
    }

    #[test]
    fn rejects_a_shape_it_cannot_use() {
        assert!(parse_edits(r#"{"answer":"no edits needed"}"#).is_err());
        assert!(parse_edits("not json at all").is_err());
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
