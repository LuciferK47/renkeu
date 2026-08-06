//! Workspace path sandbox (masterplan.md §11.2): every user-supplied path is
//! validated against the workspace root before any read or write.

use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum SecurityError {
    #[error("null byte in path")]
    NullByte,
    #[error("absolute path outside workspace")]
    AbsoluteOutsideWorkspace,
    #[error("path contains parent directory component")]
    ParentDir,
    #[error("path not found or invalid")]
    NotFound,
    #[error("path escapes workspace")]
    EscapesWorkspace,
    #[error("symlink escapes workspace")]
    SymlinkEscape,
    #[error("forbidden file extension: {0}")]
    ForbiddenExtension(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessKind {
    Read,
    Write,
}

/// Extensions the engine may write. Everything else is rejected so an AI edit
/// can never produce an executable, dotenv, or key file.
const ALLOWED_WRITE_EXTENSIONS: &[&str] = &["tex", "bib", "sty", "cls", "tikz"];

/// Resolve `requested` inside `workspace_root`, rejecting traversal, symlink
/// escapes, and (for writes) non-allowlisted extensions.
///
/// The target must exist — resolution canonicalizes to defeat `..`/symlink
/// tricks. Callers creating brand-new files must validate the parent instead.
///
/// # Errors
/// A [`SecurityError`] naming the first check that failed.
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Canonicalized tempdir with one `doc.tex` inside.
    fn workspace() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        std::fs::write(root.join("doc.tex"), "x").unwrap();
        (dir, root)
    }

    #[test]
    fn accepts_relative_path_inside_workspace() {
        let (_dir, root) = workspace();
        let p = validate_path(&root, "doc.tex", AccessKind::Read).unwrap();
        assert_eq!(p, root.join("doc.tex"));
        // "./" prefix is equivalent.
        assert!(validate_path(&root, "./doc.tex", AccessKind::Read).is_ok());
    }

    #[test]
    fn rejects_traversal_and_null_bytes() {
        let (_dir, root) = workspace();
        assert!(matches!(
            validate_path(&root, "../etc/passwd", AccessKind::Read),
            Err(SecurityError::ParentDir)
        ));
        assert!(matches!(
            validate_path(&root, "doc\0.tex", AccessKind::Read),
            Err(SecurityError::NullByte)
        ));
    }

    #[test]
    fn rejects_absolute_path_outside_workspace() {
        let (_dir, root) = workspace();
        assert!(matches!(
            validate_path(&root, "/etc/hostname", AccessKind::Read),
            Err(SecurityError::AbsoluteOutsideWorkspace)
        ));
    }

    #[test]
    fn rejects_symlink_pointing_out_of_workspace() {
        let (_dir, root) = workspace();
        std::os::unix::fs::symlink("/etc/hostname", root.join("sneaky.tex")).unwrap();
        let err = validate_path(&root, "sneaky.tex", AccessKind::Read).unwrap_err();
        // canonicalize resolves the link, so either escape variant is correct
        assert!(
            matches!(
                err,
                SecurityError::EscapesWorkspace | SecurityError::SymlinkEscape
            ),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn write_allowlist_blocks_dangerous_extensions() {
        let (_dir, root) = workspace();
        std::fs::write(root.join("run.sh"), "#!/bin/sh").unwrap();
        assert!(matches!(
            validate_path(&root, "run.sh", AccessKind::Write),
            Err(SecurityError::ForbiddenExtension(_))
        ));
        // Reading it is fine; only writes are extension-gated.
        assert!(validate_path(&root, "run.sh", AccessKind::Read).is_ok());
        assert!(validate_path(&root, "doc.tex", AccessKind::Write).is_ok());
    }

    #[test]
    fn missing_file_is_not_found() {
        let (_dir, root) = workspace();
        assert!(matches!(
            validate_path(&root, "ghost.tex", AccessKind::Read),
            Err(SecurityError::NotFound)
        ));
    }
}
