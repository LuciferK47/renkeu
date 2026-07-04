use axum::{
    extract::State,
    response::sse::{Event, Sse},
    Json,
};
use latex_ai_core::ai::EditPlanner;
use serde::Deserialize;
use std::convert::Infallible;
use tokio_stream::Stream;
use tokio_util::sync::CancellationToken;
use tracing::instrument;
use crate::AppState;
use latex_ai_compiler::scratchpad::ScratchPad;
// latex_ai_core document was incorrect, it should be in core? Wait, there is no LatexEngine in core. Let's remove it and use apply_edits on ScratchPad.

#[derive(Deserialize, Debug)]
pub struct PlanRequest {
    pub instruction: String,
    pub file_path: String,
}

#[derive(Deserialize, Debug)]
pub struct ApplySelectedRequest {
    pub file_path: String,
    pub edits: Vec<latex_ai_core::compile::AgentEdit>,
    pub selected_hunk_ids: Vec<String>,
}

#[derive(serde::Serialize)]
pub struct ApplySelectedResponse {
    pub success: bool,
    pub error: Option<String>,
    pub history_id: Option<i64>,
}

#[instrument(skip(state))]
pub async fn apply_selected(
    State(state): State<AppState>,
    Json(req): Json<ApplySelectedRequest>,
) -> axum::response::Json<ApplySelectedResponse> {
    let mut selected_edits = Vec::new();
    for edit in &req.edits {
        let hunk_id = edit.hunk_id();
        if req.selected_hunk_ids.contains(&hunk_id.0) {
            selected_edits.push(edit);
        }
    }
    
    let path = std::path::Path::new(&req.file_path);
    match state.store.atomic_apply_selected(path, &selected_edits, "apply-selected").await {
        Ok(outcome) => axum::response::Json(ApplySelectedResponse {
            success: true,
            error: None,
            history_id: Some(outcome.history_id),
        }),
        Err(e) => axum::response::Json(ApplySelectedResponse {
            success: false,
            error: Some(e.to_string()),
            history_id: None,
        }),
    }
}


#[instrument(skip(state))]
pub async fn plan_stream(
    State(state): State<AppState>,
    Json(req): Json<PlanRequest>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let cancel = CancellationToken::new();
    let cancel_clone = cancel.clone();
    let (tx, rx) = tokio::sync::mpsc::channel(10);
    
    let instruction = req.instruction.clone();
    tokio::spawn(async move {
        let safe_instruction = match latex_ai_core::prompt_safety::sanitize(&instruction, 10000) {
            Ok(s) => s,
            Err(e) => {
                if let Err(se) = tx.send(Ok(Event::default().event("error").data(e.to_string()))).await { tracing::warn!(error = ?se, "SSE send failed"); }
                return;
            }
        };
        let file_path = match std::env::current_dir() {
            Ok(dir) => dir.join(&req.file_path),
            Err(_e) => {
                if let Err(e) = tx.send(Ok(Event::default().event("error").data("Internal error: cannot determine working directory"))).await { tracing::warn!(error = ?e, "SSE send failed"); }
                return;
            }
        };
        let content = match tokio::fs::read_to_string(&file_path).await {
            Ok(c) => c,
            Err(_) => String::new(),
        };
        
        if let Err(e) = tx.send(Ok(Event::default().event("progress").data("Thinking..."))).await { tracing::warn!(error = ?e, "SSE send failed"); }
        
        match state.planner.plan_edits(&safe_instruction, &content, cancel_clone).await {
            Ok(edits) => {
                match serde_json::to_string(&edits) {
                    Ok(edits_json) => {
                        if let Err(e) = tx.send(Ok(Event::default().event("done").data(edits_json))).await {
                            tracing::warn!(error = ?e, "SSE send failed");
                        }
                    }
                    Err(e) => {
                        if let Err(se) = tx.send(Ok(Event::default().event("error").data(e.to_string()))).await {
                            tracing::warn!(error = ?se, "SSE send failed");
                        }
                    }
                }
            }
            Err(e) => {
                if let Err(e) = tx.send(Ok(Event::default().event("error").data(e.to_string()))).await { tracing::warn!(error = ?e, "SSE send failed"); }
            }
        }
    });

    Sse::new(tokio_stream::wrappers::ReceiverStream::new(rx))
}

#[instrument(skip(state))]
pub async fn agent_loop_stream(
    State(state): State<AppState>,
    Json(req): Json<PlanRequest>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let cancel = CancellationToken::new();
    let cancel_clone = cancel.clone();
    let (tx, rx) = tokio::sync::mpsc::channel(10);
    
    let instruction = req.instruction.clone();
    tokio::spawn(async move {
        let file_path = match std::env::current_dir() {
            Ok(dir) => dir.join(&req.file_path),
            Err(_) => {
                if let Err(e) = tx.send(Ok(Event::default().event("error").data("Internal error"))).await { tracing::warn!(error = ?e, "SSE send failed"); }
                return;
            }
        };
        
        let mut content = match tokio::fs::read_to_string(&file_path).await {
            Ok(c) => c,
            Err(_) => String::new(),
        };
        
        let mut instruction = match latex_ai_core::prompt_safety::sanitize(&req.instruction, 10000) {
            Ok(s) => s,
            Err(e) => {
                if let Err(se) = tx.send(Ok(Event::default().event("error").data(e.to_string()))).await { tracing::warn!(error = ?se, "SSE send failed"); }
                return;
            }
        };
        let max_iters = 3;
        
        for i in 0..max_iters {
            if let Err(e) = tx.send(Ok(Event::default().event("progress").data(format!("Iteration {}/{} - Thinking...", i+1, max_iters)))).await { tracing::warn!(error = ?e, "SSE send failed"); }
            
            let mut edits = match state.planner.plan_edits(&instruction, &content, cancel_clone.clone()).await {
                Ok(edits) => edits,
                Err(e) => {
                    if let Err(e) = tx.send(Ok(Event::default().event("error").data(e.to_string()))).await { tracing::warn!(error = ?e, "SSE send failed"); }
                    break;
                }
            };
            
            for edit in &mut edits {
                edit.hunk_id = Some(edit.hunk_id().0);
            }
            
            if let Err(e) = tx.send(Ok(Event::default().event("progress").data("Applying edits to scratchpad..."))).await { tracing::warn!(error = ?e, "SSE send failed"); }
            
            let scratch = match ScratchPad::create_for(&uuid::Uuid::new_v4().to_string(), &file_path.parent().unwrap_or(std::path::Path::new("/")), &file_path) {
                Ok(s) => s,
                Err(e) => {
                    if let Err(e) = tx.send(Ok(Event::default().event("error").data(e.to_string()))).await { tracing::warn!(error = ?e, "SSE send failed"); }
                    break;
                }
            };
            
            if let Err(e) = scratch.apply_edits(&edits).await {
                if let Err(e) = tx.send(Ok(Event::default().event("error").data(e.to_string()))).await { tracing::warn!(error = ?e, "SSE send failed"); }
                break;
            }
            
            // Read the modified content from the scratchpad before compiling
            let compiled_content = tokio::fs::read_to_string(&scratch.main_file).await.unwrap_or_else(|_| content.clone());
            match state.queue.submit(&compiled_content, &cancel_clone).await {
                Ok(result) => {
                    if result.success {
                        if let Err(e) = tx.send(Ok(Event::default().event("progress").data("Success! Emitting proposal for review..."))).await { tracing::warn!(error = ?e, "SSE send failed"); }
                        
                        let proposal = serde_json::json!({
                            "summary": "AI proposed edits based on your request.",
                            "edit_count": edits.len(),
                            "edits": edits,
                        });
                        
                        match serde_json::to_string(&proposal) {
                            Ok(proposal_json) => {
                                if let Err(e) = tx.send(Ok(Event::default().event("proposal").data(proposal_json))).await {
                                    tracing::warn!(error = ?e, "SSE send failed");
                                }
                            }
                            Err(e) => {
                                if let Err(se) = tx.send(Ok(Event::default().event("error").data(e.to_string()))).await {
                                    tracing::warn!(error = ?se, "SSE send failed");
                                }
                            }
                        }
                        if let Err(e) = tx.send(Ok(Event::default().event("done").data(""))).await { tracing::warn!(error = ?e, "SSE send failed"); }
                        break;
                    }
                }
                Err(latex_ai_compiler::CompileError::Tectonic(msg)) => {
                    if let Err(e) = tx.send(Ok(Event::default().event("progress").data("Compile failed, replanning..."))).await { tracing::warn!(error = ?e, "SSE send failed"); }
                    instruction = format!("The previous edits resulted in compilation errors:\n{}\nPlease fix them.", msg);
                    content = tokio::fs::read_to_string(&scratch.main_file).await.unwrap_or(content);
                }
                Err(e) => {
                    if let Err(e) = tx.send(Ok(Event::default().event("error").data(e.to_string()))).await { tracing::warn!(error = ?e, "SSE send failed"); }
                    break;
                }
            }
        }
    });

    Sse::new(tokio_stream::wrappers::ReceiverStream::new(rx))
}
