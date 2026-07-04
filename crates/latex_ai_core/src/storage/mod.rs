use sqlx::SqlitePool;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum StorageError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Etag conflict. Expected {expected}, found {actual}")]
    EtagConflict { expected: String, actual: String },
}

pub struct SaveOutcome {
    pub new_etag: String,
    pub history_id: i64,
}

pub struct Store {
    pool: SqlitePool,
    workspace_root: PathBuf,
}

impl Store {
    pub fn new(pool: SqlitePool, workspace_root: PathBuf) -> Self {
        Self { pool, workspace_root }
    }

    pub async fn save_with_etag_history(
        &self,
        path: &Path,
        content: &str,
        expected_etag: Option<&str>,
        command: &str,
        edit_type: &str,
    ) -> Result<SaveOutcome, StorageError> {
        let full_path = self.workspace_root.join(path);
        
        if full_path.exists() {
            let content = tokio::fs::read_to_string(&full_path).await?;
            let hash = blake3::hash(content.as_bytes());
            let etag = hex::encode(hash.as_bytes());
            
            if let Some(expected) = expected_etag {
                if expected != etag {
                    return Err(StorageError::EtagConflict {
                        expected: expected.to_string(),
                        actual: etag,
                    });
                }
            }
        }


        let tmp = full_path.with_extension("tmp");
        tokio::fs::write(&tmp, content).await?;
        tokio::fs::rename(&tmp, &full_path).await?;
        let new_hash = blake3::hash(content.as_bytes());
        let new_etag = hex::encode(new_hash.as_bytes());
        
        // Create unified diff
        let old_content = tokio::fs::read_to_string(&full_path).await.unwrap_or_default();
        let diff = similar::TextDiff::from_lines(old_content.as_str(), content)
            .unified_diff()
            .header(&path.to_string_lossy(), &path.to_string_lossy())
            .to_string();

        // Save history
        let path_str = path.to_string_lossy();
        let id: i64 = sqlx::query_scalar!(
            "INSERT INTO history (file_path, command, new_text, diff, edit_type) VALUES (?, ?, ?, ?, ?) RETURNING id",
            path_str,
            command,
            content,
            diff,
            edit_type
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(SaveOutcome {
            new_etag,
            history_id: id,
        })
    }

    pub async fn atomic_apply_selected(
        &self,
        path: &Path,
        edits: &[&crate::compile::AgentEdit],
        history_tag: &str,
    ) -> Result<SaveOutcome, StorageError> {
        let full_path = self.workspace_root.join(path);
        
        let old_content = if full_path.exists() {
            tokio::fs::read_to_string(&full_path).await?
        } else {
            String::new()
        };
        let mut content = old_content.clone();
        if false {
            tokio::fs::read_to_string(&full_path).await?
        } else {
            String::new()
        };
        
        let hash = blake3::hash(content.as_bytes());
        let current_etag = hex::encode(hash.as_bytes());
        
        // Check expected_etag from edits if any
        if let Some(first_edit) = edits.first() {
            if let Some(expected) = &first_edit.expected_etag {
                if expected != &current_etag {
                    return Err(StorageError::EtagConflict {
                        expected: expected.clone(),
                        actual: current_etag,
                    });
                }
            }
        }
        
        // Apply edits
        for edit in edits {
            match edit.op {
                crate::compile::AgentOp::RewriteFile => {
                    if let Some(payload) = &edit.payload {
                        content = payload.clone();
                    }
                }
                crate::compile::AgentOp::ReplaceRange => {
                    if let Some(target) = &edit.text {
                        if let Some(replacement) = &edit.insert_text {
                            content = content.replace(target, replacement);
                        }
                    }
                }
                crate::compile::AgentOp::Insert => {
                    // Simple append if line not specified
                    if let Some(replacement) = &edit.insert_text {
                        content.push_str(replacement);
                    }
                }
                crate::compile::AgentOp::ApplyUnifiedDiff => {
                    // For ApplyUnifiedDiff we just write the diff to a tmp file and use patch command
                    if let Some(payload) = &edit.payload {
                        let patch_file = full_path.with_extension("patch");
                        tokio::fs::write(&patch_file, payload).await?;
                        let tmp_file = full_path.with_extension("patch_target");
                        tokio::fs::write(&tmp_file, &content).await?;
                        let status = std::process::Command::new("patch")
                            .arg(&tmp_file)
                            .arg(&patch_file)
                            .status()?;
                        if status.success() {
                            content = tokio::fs::read_to_string(&tmp_file).await?;
                        }
                        let _ = tokio::fs::remove_file(&patch_file).await;
                        let _ = tokio::fs::remove_file(&tmp_file).await;
                    }
                }
            }
        }

        let tmp = full_path.with_extension("tmp");
        tokio::fs::write(&tmp, &content).await?;
        tokio::fs::rename(&tmp, &full_path).await?;
        let new_hash = blake3::hash(content.as_bytes());
        let new_etag = hex::encode(new_hash.as_bytes());
        
        let diff = similar::TextDiff::from_lines(&old_content, &content)
            .unified_diff()
            .header(&path.to_string_lossy(), &path.to_string_lossy())
            .to_string();
        let path_str = path.to_string_lossy();
        let id: i64 = sqlx::query_scalar!(
            "INSERT INTO history (file_path, command, new_text, diff, edit_type) VALUES (?, ?, ?, ?, ?) RETURNING id",
            path_str,
            "apply selected",
            content,
            diff,
            history_tag
        )
        .fetch_one(&self.pool)
        .await?;
        
        Ok(SaveOutcome {
            new_etag,
            history_id: id,
        })
    }
}
