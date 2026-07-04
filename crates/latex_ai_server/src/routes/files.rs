use axum::{extract::{State, Path}, Json};
use crate::error::ApiError;
use latex_ai_core::security::{validate_path, AccessKind};
use serde::Deserialize;
use serde::Serialize;
use tracing::instrument;
use crate::AppState;

#[derive(Serialize)]
pub struct FileList {
    pub files: Vec<String>,
}

#[instrument(skip(_state))]
pub async fn list_files(State(_state): State<AppState>) -> Json<FileList> {
    let workspace = std::env::current_dir().unwrap_or_default();
    
    let files = tokio::task::spawn_blocking(move || {
        let mut result = Vec::new();
        let mut dirs = vec![workspace.clone()];
        
        while let Some(dir) = dirs.pop() {
            if let Ok(entries) = std::fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        let name = entry.file_name();
                        let name_str = name.to_string_lossy();
                        if !name_str.starts_with('.') && name_str != "target" && name_str != "node_modules" {
                            dirs.push(path);
                        }
                    } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                        if matches!(ext, "tex" | "bib" | "sty" | "cls" | "tikz") {
                            if let Ok(rel) = path.strip_prefix(&workspace) {
                                result.push(rel.to_string_lossy().into_owned());
                            }
                        }
                    }
                }
            }
        }
        result
    })
    .await
    .unwrap_or_default();

    Json(FileList { files })
}


#[instrument(skip(_state))]
pub async fn read_file(
    State(_state): State<AppState>,
    Path(path): Path<String>,
) -> Result<String, ApiError> {
    let workspace = std::env::current_dir().map_err(|_| ApiError::BadRequest("No workspace".into()))?;
    
    let safe_path = validate_path(&workspace, &path, AccessKind::Read)
        .map_err(|e| ApiError::InvalidPath(e.to_string()))?;
        
    let content = tokio::fs::read_to_string(&safe_path)
        .await
        .map_err(|_| ApiError::BadRequest("File not found or unreadable".into()))?;
        
    Ok(content)
}

#[derive(Debug, Deserialize)]
pub struct WriteFileRequest {
    pub content: String,
}

#[instrument(skip(_state))]
pub async fn write_file(
    State(_state): State<AppState>,
    Path(path): Path<String>,
    Json(req): Json<WriteFileRequest>,
) -> Result<(), ApiError> {
    let workspace = std::env::current_dir().map_err(|_| ApiError::BadRequest("No workspace".into()))?;
    
    let safe_path = validate_path(&workspace, &path, AccessKind::Write)
        .map_err(|e| ApiError::InvalidPath(e.to_string()))?;
        
    tokio::fs::write(&safe_path, req.content)
        .await
        .map_err(|_| ApiError::BadRequest("Could not write file".into()))?;
        
    Ok(())
}
