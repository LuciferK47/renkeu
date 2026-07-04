use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SecurityError {
    #[error("Null byte in path")]
    NullByte,
    #[error("Absolute path outside workspace")]
    AbsoluteOutsideWorkspace,
    #[error("Path contains parent directory component")]
    ParentDir,
    #[error("Path not found or invalid")]
    NotFound,
    #[error("Path escapes workspace")]
    EscapesWorkspace,
    #[error("Symlink escapes workspace")]
    SymlinkEscape,
    #[error("Forbidden file extension: {0}")]
    ForbiddenExtension(String),
}

pub enum AccessKind {
    Read,
    Write,
}

const ALLOWED_WRITE_EXTENSIONS: &[&str] = &["tex", "bib", "sty", "cls", "tikz"];
#[allow(dead_code)]
const BLOCKED_EXTENSIONS: &[&str] = &["py", "sh", "exe", "env", "key", "pem", "cert"];

pub fn validate_path(
    workspace_root: &Path,
    requested: &str,
    kind: AccessKind,
) -> Result<PathBuf, SecurityError> {
    if requested.contains('\0') {
        return Err(SecurityError::NullByte);
    }
    
    let requested_path = Path::new(requested);
    if requested_path.is_absolute() && !requested_path.starts_with(workspace_root) {
        return Err(SecurityError::AbsoluteOutsideWorkspace);
    }
    
    if requested.contains("..") {
        return Err(SecurityError::ParentDir);
    }
    
    let stripped = requested.strip_prefix("./").unwrap_or(requested);
    let target = workspace_root.join(stripped);
    
    let abs = std::fs::canonicalize(&target).map_err(|_| SecurityError::NotFound)?;
    
    if !abs.starts_with(workspace_root) {
        return Err(SecurityError::EscapesWorkspace);
    }
    
    if let Ok(meta) = std::fs::symlink_metadata(&abs) {
        if meta.file_type().is_symlink() {
            let resolved = std::fs::canonicalize(&abs).map_err(|_| SecurityError::NotFound)?;
            if !resolved.starts_with(workspace_root) {
                return Err(SecurityError::SymlinkEscape);
            }
        }
    }
    
    if matches!(kind, AccessKind::Write) {
        let ext = abs
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
            
        if !ALLOWED_WRITE_EXTENSIONS.contains(&ext.as_str()) {
            return Err(SecurityError::ForbiddenExtension(ext));
        }
    }
    
    Ok(abs)
}
