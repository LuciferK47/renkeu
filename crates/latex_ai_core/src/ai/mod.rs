use crate::compile::AgentEdit;
use genai::Client;
use genai::chat::{ChatMessage, ChatRequest, MessageContent};
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
    #[error("No content in AI response")]
    EmptyResponse,
}

pub trait EditPlanner: Send + Sync {
    fn plan_edits(
        &self,
        instruction: &str,
        file_content: &str,
        cancel: CancellationToken,
    ) -> impl std::future::Future<Output = Result<Vec<AgentEdit>, AiError>> + Send;

    fn visual_fix(
        &self,
        instruction: &str,
        b64_image: &str,
        file_path: Option<&str>,
        line: Option<u32>,
        cancel: CancellationToken,
    ) -> impl std::future::Future<Output = Result<Vec<AgentEdit>, AiError>> + Send;
}

pub struct GeminiPlanner {
    client: Arc<Client>,
    model: String,
}

impl GeminiPlanner {
    pub fn new(model: impl Into<String>) -> Self {
        let client = Client::builder().build();
            
        Self {
            client: Arc::new(client),
            model: model.into(),
        }
    }
}

impl EditPlanner for GeminiPlanner {
    fn plan_edits(
        &self,
        instruction: &str,
        file_content: &str,
        cancel: CancellationToken,
    ) -> impl std::future::Future<Output = Result<Vec<AgentEdit>, AiError>> + Send {
        async move {
            let system_prompt = r#"
You are a LaTeX expert. You must output ONLY a JSON array of AgentEdit objects. Do not include markdown codeblocks or any other text.
Format of AgentEdit:
{
  "file_path": "main.tex",
  "op": "replace_range",
  "text": "old text to replace",
  "insert_text": "new text to insert"
}
"#;

        let user_prompt = format!(
            "Instruction: {}\n\nFile Content:\n```latex\n{}\n```",
            instruction, file_content
        );

        let req = ChatRequest::new(vec![
            ChatMessage::system(system_prompt),
            ChatMessage::user(user_prompt),
        ]);

        let res = tokio::select! {
            r = self.client.exec_chat(&self.model, req, None) => r.map_err(|e| AiError::Request(e.to_string()))?,
            _ = cancel.cancelled() => return Err(AiError::Cancelled),
        };

        if let Some(MessageContent::Text(text)) = res.content {
            // Very naive json extraction for now
            let json_str = text.trim().trim_start_matches("```json").trim_start_matches("```").trim_end_matches("```").trim();
            let edits: Vec<AgentEdit> = serde_json::from_str(json_str)?;
            Ok(edits)
        } else {
            Err(AiError::EmptyResponse)
        }
        }
    }

    fn visual_fix(
        &self,
        instruction: &str,
        b64_image: &str,
        file_path: Option<&str>,
        line: Option<u32>,
        cancel: CancellationToken,
    ) -> impl std::future::Future<Output = Result<Vec<AgentEdit>, AiError>> + Send {
        async move {
            let system_prompt = r#"
You are a LaTeX expert. You must output ONLY a JSON array of AgentEdit objects. Do not include markdown codeblocks or any other text.
Format of AgentEdit:
{
  "file_path": "main.tex",
  "op": "replace_range",
  "text": "old text to replace",
  "insert_text": "new text to insert"
}
"#;

            let file_context = file_path.unwrap_or("unknown.tex");
            let line_context = line.map_or("unknown".to_string(), |l| l.to_string());
            
            let user_prompt = format!(
                "Instruction: {}\n\nFile Context: {}\nLine Context: {}\n\nImage Base64 (cropped area of issue):\n{}",
                instruction, file_context, line_context, b64_image
            );

            let req = ChatRequest::new(vec![
                ChatMessage::system(system_prompt),
                ChatMessage::user(user_prompt),
            ]);

            let res = tokio::select! {
                r = self.client.exec_chat(&self.model, req, None) => r.map_err(|e| AiError::Request(e.to_string()))?,
                _ = cancel.cancelled() => return Err(AiError::Cancelled),
            };

            if let Some(MessageContent::Text(text)) = res.content {
                let json_str = text.trim().trim_start_matches("```json").trim_start_matches("```").trim_end_matches("```").trim();
                let edits: Vec<AgentEdit> = serde_json::from_str(json_str)?;
                Ok(edits)
            } else {
                Err(AiError::EmptyResponse)
            }
        }
    }
}
