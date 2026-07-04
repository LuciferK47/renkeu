use axum::{
    extract::{Path, State},
    response::sse::{Event, Sse},
    Json,
};
use serde_json::json;
use tokio_stream::Stream;
use std::convert::Infallible;
use tracing::instrument;
use crate::AppState;

use serde::Deserialize;
use uuid::Uuid;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Deserialize)]
pub struct FrontendCompileRequest {
    pub file_path: String,
}

#[instrument(skip(_state))]
pub async fn compile(
    State(_state): State<AppState>,
    Json(req): Json<FrontendCompileRequest>,
) -> Json<serde_json::Value> {
    let session_id = Uuid::new_v4().to_string();
    
    // In a real implementation we might save this job to a DB.
    // For now, we return the job ID (session_id) so the client can connect to the stream.
    Json(json!({ "status": "compiling", "session_id": session_id }))
}

#[instrument(skip(state))]
pub async fn compile_stream(
    Path(id): Path<String>,
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let (tx, rx) = tokio::sync::mpsc::channel(10);
    
    // Note: Since we didn't receive the file_path in this stream endpoint,
    // and we don't have a DB to look up the session_id yet, we just compile main.tex
    // which is a hack. The actual fix requires implementing the DB queue.
    tokio::spawn(async move {
        if let Err(e) = tx.send(Ok(Event::default().event("status").data("compiling"))).await {
            tracing::warn!(error = ?e, "SSE send failed");
            return;
        }
        
        let file_path = std::env::current_dir().unwrap_or_default().join("examples/sample.tex");
        let content = match tokio::fs::read_to_string(&file_path).await {
            Ok(c) => c,
            Err(_) => String::new(),
        };
        
        let cancel = CancellationToken::new();
        match state.queue.submit(&content, &cancel).await {
            Ok(result) => {
                let status = if result.success { "success" } else { "failed" };
                if let Err(e) = tx.send(Ok(Event::default().event("done").data(status))).await {
                    tracing::warn!(error = ?e, "SSE send failed");
                }
            }
            Err(_) => {
                let _ = tx.send(Ok(Event::default().event("error").data("Compile failed"))).await;
            }
        }
    });

    Sse::new(tokio_stream::wrappers::ReceiverStream::new(rx))
}
