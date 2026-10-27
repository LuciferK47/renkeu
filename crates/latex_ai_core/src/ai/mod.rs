//! LLM edit-planning service.
//!
//! Provider-neutral: `genai` picks the provider adapter from the model name
//! (e.g. `llama-3.3-70b-versatile` -> Groq, authenticated via `GROQ_API_KEY`).
//! Planning is one-shot per call — iteration context (compile errors) is folded
//! into the instruction by the agent loop, so no chat history lives here.

use crate::compile::AgentEdit;
use crate::config::ApiKey;
use genai::adapter::AdapterKind;
use genai::chat::{
    ChatMessage, ChatOptions, ChatRequest, ChatResponseFormat, ContentPart, MessageContent,
};
use genai::resolver::AuthData;
use genai::{Client, ModelIden};
use std::fmt::Write as _;
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
    #[error("AI is not configured: add an API key or choose a local model in Settings")]
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

    /// Propose edits fulfilling the request against the files it shows.
    fn plan_edits<'a>(&'a self, req: PlanRequest<'a>, cancel: CancellationToken) -> PlanFuture<'a>;

    /// Propose edits for a visual defect shown in a PDF crop (M3).
    fn visual_fix<'a>(
        &'a self,
        req: VisualFixRequest<'a>,
        cancel: CancellationToken,
    ) -> PlanFuture<'a>;
}

/// One file as the planner sees it.
#[derive(Debug, Clone, Copy)]
pub struct PlanFile<'a> {
    /// Project-relative path; edits must use exactly this as `file_path`.
    pub path: &'a str,
    pub content: &'a str,
    /// 1-based lines worth looking at. A long file is then shown as windows
    /// around these (with the real line numbers and a note for each gap)
    /// instead of in full; empty means the whole file.
    pub focus: &'a [u32],
}

/// Everything the planner is told for one attempt.
#[derive(Debug, Clone, Copy)]
pub struct PlanRequest<'a> {
    pub instruction: &'a str,
    /// Files the planner may edit. The first is the one the user is in.
    pub files: &'a [PlanFile<'a>],
    /// The rest of the project, by name only, so it knows the layout.
    pub other_files: &'a [String],
    /// What the previous attempt got wrong (compiler errors, a refused edit).
    /// Empty on the first attempt.
    pub feedback: &'a str,
}

/// Per-file cap on what is sent, in bytes. A thesis chapter fits; a pasted
/// data file does not get to eat the whole context window.
const MAX_FILE_BYTES: usize = 60_000;

impl PlanRequest<'_> {
    /// The user message: instruction, project layout, every file with line
    /// numbers, and the last attempt's feedback.
    ///
    /// Line numbers let the model say "replace lines 12-14" instead of
    /// reproducing text it might misquote. They are a view only: they are not
    /// part of the file, and the system prompt says so.
    #[must_use]
    pub fn render_prompt(&self) -> String {
        let mut out = format!("Instruction: {}\n", self.instruction);

        if !self.other_files.is_empty() {
            out.push_str("\nOther project files (not shown): ");
            out.push_str(&self.other_files.join(", "));
            out.push('\n');
        }
        for file in self.files {
            let _ = write!(out, "\nFile: {}\n```latex\n", file.path);
            out.push_str(&number_lines(file.content, file.focus));
            out.push_str("```\n");
        }
        if !self.feedback.trim().is_empty() {
            out.push_str("\nYour previous attempt did not work:\n");
            out.push_str(self.feedback.trim());
            out.push('\n');
        }
        out
    }
}

/// Files at most this many lines are always shown whole.
const SHORT_FILE_LINES: usize = 200;
/// Lines shown before and after a focus line: a paragraph ends on its line
/// and began some lines above it.
const FOCUS_BEFORE: usize = 15;
const FOCUS_AFTER: usize = 2;
/// Windows are taken in the order the focus lines are given (most important
/// first) until this many bytes are shown, so a file of very long lines does
/// not turn into the whole file again. The first window is always shown.
const FOCUS_BUDGET_BYTES: usize = 7_000;

/// `content` with a right-aligned line number and ` | ` before every line,
/// cut off (with a note) after [`MAX_FILE_BYTES`]. With `focus` lines and a
/// long file, only the windows around them are shown.
fn number_lines(content: &str, focus: &[u32]) -> String {
    let lines: Vec<&str> = content.lines().collect();
    if !focus.is_empty() && lines.len() > SHORT_FILE_LINES {
        return number_windows(&lines, focus);
    }
    let shown = if content.len() > MAX_FILE_BYTES {
        let mut cut = MAX_FILE_BYTES;
        while !content.is_char_boundary(cut) {
            cut -= 1;
        }
        &content[..cut]
    } else {
        content
    };
    let total = shown.lines().count().max(1);
    let width = total.to_string().len();

    let mut out = String::with_capacity(shown.len() + total * (width + 3));
    for (i, line) in shown.lines().enumerate() {
        let _ = writeln!(out, "{:>width$} | {line}", i + 1);
    }
    if shown.len() < content.len() {
        out.push_str("[... the rest of this file is not shown ...]\n");
    }
    out
}

/// Windows of `lines` around the 1-based `focus` lines, numbered as in the
/// whole file, with a note where lines were left out.
fn number_windows(lines: &[&str], focus: &[u32]) -> String {
    let mut chosen: Vec<(usize, usize)> = Vec::new();
    let mut bytes = 0;
    for f in focus.iter().filter(|f| **f >= 1) {
        let at = *f as usize;
        let span = (
            at.saturating_sub(FOCUS_BEFORE).max(1),
            (at + FOCUS_AFTER).min(lines.len()),
        );
        if span.0 > span.1 {
            continue;
        }
        let size: usize = lines[span.0 - 1..span.1].iter().map(|l| l.len() + 1).sum();
        if !chosen.is_empty() && bytes + size > FOCUS_BUDGET_BYTES {
            continue;
        }
        bytes += size;
        chosen.push(span);
    }
    chosen.sort_unstable();
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (from, to) in chosen {
        match merged.last_mut() {
            Some(last) if from <= last.1 + 1 => last.1 = last.1.max(to),
            _ => merged.push((from, to)),
        }
    }
    let width = lines.len().to_string().len();
    let mut out = String::new();
    let mut next = 1;
    for (from, to) in merged {
        if from > next {
            let _ = writeln!(out, "[... lines {next}-{} are not shown ...]", from - 1);
        }
        for n in from..=to {
            let _ = writeln!(out, "{n:>width$} | {}", lines[n - 1]);
        }
        next = to + 1;
    }
    if next <= lines.len() {
        let _ = writeln!(out, "[... lines {next}-{} are not shown ...]", lines.len());
    }
    out
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
Each file is shown with line numbers in the form `  12 | text`. The number and the ` | ` are NOT part of the file; they only tell you where you are.
Each AgentEdit looks like one of:
{ "file_path": "main.tex", "op": "replace_range", "text": "exact text currently in the file", "insert_text": "replacement text" }
{ "file_path": "main.tex", "op": "replace_range", "start_line": 12, "end_line": 14, "insert_text": "replacement lines" }
{ "file_path": "main.tex", "op": "insert", "line": 5, "insert_text": "new lines" }
{ "file_path": "main.tex", "op": "rewrite_file", "payload": "the complete new contents of the file" }
Rules:
- "file_path" must be one of the files shown, spelled exactly as shown.
- Line numbers always refer to the files as shown to you, even when several edits are in one answer.
- "text" must be copied verbatim from the file WITHOUT the line numbers or the ` | `.
- Your answer is JSON, so every backslash in LaTeX must be written as two (`\\section`, `\\textbf`), and a line break inside a string as `\n`. A single backslash before `t`, `n`, `r`, `b` or `f` silently becomes a control character and breaks the document.
- Use op "rewrite_file" with "payload" holding the full new file only when a targeted replacement is impossible.
- Do not add \input or \include of files outside the project, and never use \openout, \write18 or \catcode.
- Propose the smallest edits that satisfy the instruction.
"#;

/// Whether `model` is served from this computer (Ollama) rather than by a
/// hosted service. `genai` falls back to Ollama for any name it does not
/// recognise as a hosted model, so this is also what lets the assistant run
/// with no API key at all.
#[must_use]
pub fn is_local_model(model: &str) -> bool {
    AdapterKind::from_model(model).is_ok_and(|kind| kind == AdapterKind::Ollama)
}

/// Say what to do when a request to a local model fails, instead of passing on
/// a bare transport error.
fn explain_request_error(model: &str, error: &str) -> String {
    if !is_local_model(model) {
        return error.to_owned();
    }
    let lower = error.to_lowercase();
    if lower.contains("not found") {
        format!("Ollama does not have the model `{model}`. Run `ollama pull {model}`, or pick an installed model in Settings. ({error})")
    } else if lower.contains("connect") || lower.contains("sending request") {
        format!("Ollama is not answering on localhost:11434. Start it (`ollama serve`) and try again. ({error})")
    } else {
        error.to_owned()
    }
}

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
                r.map_err(|e| AiError::Request(explain_request_error(model, &e.to_string())))?
            }
            () = cancel.cancelled() => return Err(AiError::Cancelled),
        };

        let Some(MessageContent::Text(text)) = res.content else {
            return Err(AiError::EmptyResponse);
        };
        parse_edits(&text).inspect_err(|e| {
            // Without the reply, "did not parse" cannot be told apart from a
            // model that needs a better prompt. Local log only, and cut short.
            let shown: String = text.chars().take(600).collect();
            tracing::warn!(error = %e, reply = %shown, "the model's reply was not usable edits");
        })
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
    fn plan_edits<'a>(&'a self, req: PlanRequest<'a>, cancel: CancellationToken) -> PlanFuture<'a> {
        Box::pin(async move { self.request_edits(req.render_prompt(), cancel).await })
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
    fn a_name_no_hosted_service_claims_is_a_local_model() {
        for local in ["qwen2.5:7b", "llama3.2", "qwen2.5:3b-8k", "mistral"] {
            assert!(is_local_model(local), "{local}");
        }
        for hosted in ["llama-3.3-70b-versatile", "gpt-4o", "claude-3-5-sonnet", "gemini-1.5-pro"] {
            assert!(!is_local_model(hosted), "{hosted}");
        }
    }

    #[test]
    fn a_failed_local_request_says_what_to_do_and_a_hosted_one_is_left_alone() {
        let down = explain_request_error("qwen2.5:7b", "error sending request for url");
        assert!(down.contains("ollama serve"), "{down}");
        let missing = explain_request_error("qwen2.5:7b", "model 'qwen2.5:7b' not found");
        assert!(missing.contains("ollama pull qwen2.5:7b"), "{missing}");
        assert_eq!(
            explain_request_error("llama-3.3-70b-versatile", "error sending request"),
            "error sending request"
        );
    }

    #[test]
    fn the_prompt_warns_that_latex_backslashes_must_be_doubled_in_json() {
        // A model that writes `\text` in a JSON string gets a tab and `ext`;
        // seen with a real model, so the prompt has to say it.
        assert!(SYSTEM_PROMPT.contains("every backslash"));
        assert!(SYSTEM_PROMPT.contains("two"));
    }

    #[test]
    fn the_prompt_shows_the_shape_of_every_operation_it_allows() {
        // A real model answered `{"payload": …}` alone: it had been told about
        // rewrite_file but never shown what one looks like.
        for op in ["replace_range", "insert", "rewrite_file"] {
            assert!(
                SYSTEM_PROMPT.contains(&format!(r#""op": "{op}""#)),
                "no example of {op}"
            );
        }
    }

    #[test]
    fn a_single_backslash_before_t_becomes_a_tab_in_json_and_a_doubled_one_does_not() {
        // Why the prompt rule matters: this is what the model's slip does.
        let single: serde_json::Value = serde_json::from_str(r#"{"s":"\text"}"#).unwrap();
        assert_eq!(single["s"], "\u{9}ext");
        let doubled: serde_json::Value = serde_json::from_str(r#"{"s":"\\text"}"#).unwrap();
        assert_eq!(doubled["s"], "\\text");
    }

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

    fn request<'a>(
        files: &'a [PlanFile<'a>],
        other: &'a [String],
        feedback: &'a str,
    ) -> PlanRequest<'a> {
        PlanRequest {
            instruction: "fix the table",
            files,
            other_files: other,
            feedback,
        }
    }

    #[test]
    fn the_prompt_numbers_every_line_of_every_file() {
        let files = [
            PlanFile {
                path: "main.tex",
                content: "a\nb\n",
                focus: &[],
            },
            PlanFile {
                path: "chapters/one.tex",
                content: "x\n",
                focus: &[],
            },
        ];
        let other = vec!["refs.bib".to_owned()];
        let prompt = request(&files, &other, "").render_prompt();

        assert!(
            prompt.starts_with("Instruction: fix the table\n"),
            "{prompt}"
        );
        assert!(
            prompt.contains("Other project files (not shown): refs.bib"),
            "{prompt}"
        );
        assert!(
            prompt.contains("File: main.tex\n```latex\n1 | a\n2 | b\n```"),
            "{prompt}"
        );
        assert!(
            prompt.contains("File: chapters/one.tex\n```latex\n1 | x\n```"),
            "{prompt}"
        );
        assert!(
            !prompt.contains("previous attempt"),
            "no feedback on the first try"
        );
    }

    #[test]
    fn line_numbers_are_right_aligned_to_the_widest() {
        let content: String = (1..=12).map(|i| format!("l{i}\n")).collect();
        let numbered = number_lines(&content, &[]);
        assert!(numbered.starts_with(" 1 | l1\n"), "{numbered}");
        assert!(numbered.contains("\n12 | l12\n"), "{numbered}");
    }

    #[test]
    fn feedback_is_included_only_when_there_is_some() {
        let files = [PlanFile {
            path: "main.tex",
            content: "x\n",
            focus: &[],
        }];
        let prompt =
            request(&files, &[], "main.tex:3: Undefined control sequence.").render_prompt();
        assert!(
            prompt.contains(
                "Your previous attempt did not work:\nmain.tex:3: Undefined control sequence."
            ),
            "{prompt}"
        );
        assert!(
            !request(&files, &[], "   ")
                .render_prompt()
                .contains("previous attempt")
        );
    }

    #[test]
    fn huge_files_are_cut_with_a_note_and_never_mid_character() {
        let content = "é".repeat(MAX_FILE_BYTES);
        let numbered = number_lines(&content, &[]);
        assert!(numbered.contains("the rest of this file is not shown"));
        assert!(numbered.len() < MAX_FILE_BYTES + 200);
    }

    #[test]
    fn the_system_prompt_explains_the_numbering_and_forbids_the_unsafe_forms() {
        assert!(SYSTEM_PROMPT.contains("NOT part of the file"));
        assert!(SYSTEM_PROMPT.contains("\\openout"));
    }

    #[test]
    fn a_long_file_with_focus_lines_is_shown_as_numbered_windows_with_gaps_noted() {
        let text: String = (1..=400).map(|n| format!("line {n}\n")).collect();
        let focus = [100u32, 110, 300];
        let files = [PlanFile {
            path: "main.tex",
            content: &text,
            focus: &focus,
        }];
        let prompt = PlanRequest {
            instruction: "x",
            files: &files,
            other_files: &[],
            feedback: "",
        }
        .render_prompt();
        assert!(
            prompt.contains("[... lines 1-84 are not shown ...]"),
            "{prompt}"
        );
        assert!(
            prompt.contains(" 85 | line 85\n"),
            "line numbers stay those of the whole file"
        );
        // 100 and 110 overlap into one window: 85..=112.
        assert!(
            prompt.contains("112 | line 112\n")
                && prompt.contains("[... lines 113-284 are not shown ...]")
        );
        assert!(
            prompt.contains("302 | line 302\n")
                && prompt.contains("[... lines 303-400 are not shown ...]")
        );
        assert!(!prompt.contains("| line 200\n"));
    }

    #[test]
    fn windows_stop_at_a_byte_budget_so_very_long_lines_do_not_bring_back_the_whole_file() {
        let text: String = (1..=300)
            .map(|n| format!("{n} {}\n", "word ".repeat(100)))
            .collect();
        let focus: Vec<u32> = (1..=10).map(|i| i * 28).collect();
        let files = [PlanFile {
            path: "main.tex",
            content: &text,
            focus: &focus,
        }];
        let shown = PlanRequest {
            instruction: "x",
            files: &files,
            other_files: &[],
            feedback: "",
        }
        .render_prompt();
        // 10 windows of 18 lines of 500 bytes would be 90 KB; the budget keeps it near 7 KB
        // (the first window is always shown, later ones only while they fit).
        assert!(shown.len() < 20_000, "{}", shown.len());
        assert!(
            shown.contains("| 28 word") || shown.contains(" 28 | 28 word"),
            "the most important window is there"
        );
    }

    #[test]
    fn a_short_file_is_shown_whole_even_with_focus_lines() {
        let text: String = (1..=50).map(|n| format!("line {n}\n")).collect();
        let files = [PlanFile {
            path: "a.tex",
            content: &text,
            focus: &[10],
        }];
        let prompt = PlanRequest {
            instruction: "x",
            files: &files,
            other_files: &[],
            feedback: "",
        }
        .render_prompt();
        assert!(prompt.contains("50 | line 50") && !prompt.contains("not shown"));
    }
}
