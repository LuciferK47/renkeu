use std::path::{Path, PathBuf};
use latex_ai_core::compile::AgentEdit;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ScratchpadError {
    #[error("IO Error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Apply Error: {0}")]
    Apply(String),
}

pub struct ScratchPad {
    pub root: PathBuf,
    pub main_file: PathBuf,
}

impl ScratchPad {
    pub fn create_for(session_id: &str, _workspace: &Path, main_file: &Path) -> Result<Self, ScratchpadError> {
        let root = std::env::temp_dir()
            .join(".latex-ai")
            .join("scratch")
            .join(session_id);

        std::fs::create_dir_all(&root)?;
        
        // Setup initial scratchpad state by copying workspace
        // For M2, we can just copy the main_file for now or the whole workspace.
        // A robust solution copies the bundle. Let's just copy the main file for MVP.
        let file_name = main_file
            .file_name()
            .ok_or_else(|| ScratchpadError::Apply("main_file has no filename".into()))?;
        let dest_main = root.join(file_name);
        std::fs::copy(main_file, &dest_main)?;
        
        Ok(Self {
            root,
            main_file: dest_main,
        })
    }

    pub async fn apply_edits(&self, edits: &[AgentEdit]) -> Result<(), ScratchpadError> {
        // Simple apply for now.
        // Needs proper diff patching logic later.
        for edit in edits {
            let path = self.root.join(&edit.file_path);
            let mut content = tokio::fs::read_to_string(&path).await.unwrap_or_default();
            
            match edit.op {
                latex_ai_core::compile::AgentOp::RewriteFile => {
                    if let Some(payload) = &edit.payload {
                        content = payload.clone();
                    }
                }
                latex_ai_core::compile::AgentOp::ReplaceRange => {
                    if let Some(target) = &edit.text {
                        if let Some(replacement) = &edit.insert_text {
                            content = content.replace(target, replacement);
                        }
                    }
                }
                latex_ai_core::compile::AgentOp::Insert => {
                    if let Some(replacement) = &edit.insert_text {
                        content.push_str(replacement);
                    }
                }
                latex_ai_core::compile::AgentOp::ApplyUnifiedDiff => {
                    if let Some(payload) = &edit.payload {
                        let patch_file = path.with_extension("patch");
                        tokio::fs::write(&patch_file, payload).await?;
                        let tmp_file = path.with_extension("patch_target");
                        tokio::fs::write(&tmp_file, &content).await?;
                        let status = std::process::Command::new("patch")
                            .arg(&tmp_file)
                            .arg(&patch_file)
                            .status()?;
                        if status.success() {
                            content = tokio::fs::read_to_string(&tmp_file).await?;
                        } else {
                            return Err(ScratchpadError::Apply("Patch failed".into()));
                        }
                        let _ = tokio::fs::remove_file(&patch_file).await;
                        let _ = tokio::fs::remove_file(&tmp_file).await;
                    }
                }
                _ => return Err(ScratchpadError::Apply("Unsupported AgentOp".into())),
            }
            tokio::fs::write(&path, content).await?;
        }
        Ok(())
    }

    pub fn pdf_path(&self) -> PathBuf {
        self.main_file.with_extension("pdf")
    }

    pub fn synctex_path(&self) -> PathBuf {
        self.main_file.with_extension("synctex.gz")
    }
}
