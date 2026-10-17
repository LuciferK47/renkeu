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
    #[error("access to this file is denied")]
    Denied,
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

/// Whether `path` has an extension the engine may write.
#[must_use]
pub fn has_writable_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| ALLOWED_WRITE_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

/// Secrets and VCS internals that must never be read through the API, sent
/// to a model, or exposed to the TeX engine, whatever their extension.
///
/// `rel` is workspace-relative. Matching is on names, so it holds for files
/// that do not exist yet.
#[must_use]
pub fn is_denied(rel: &Path) -> bool {
    const DENIED_DIRS: &[&str] = &[".git", ".ssh", ".aws", ".gnupg"];
    const DENIED_NAMES: &[&str] = &[
        ".netrc",
        ".npmrc",
        ".pgpass",
        "id_rsa",
        "id_dsa",
        "id_ecdsa",
        "id_ed25519",
    ];
    const DENIED_EXTENSIONS: &[&str] = &["pem", "key", "p12", "pfx", "keystore"];

    let in_denied_dir = rel.components().any(|c| {
        c.as_os_str()
            .to_str()
            .is_some_and(|n| DENIED_DIRS.contains(&n))
    });
    let Some(name) = rel.file_name().and_then(|n| n.to_str()) else {
        return in_denied_dir;
    };
    let lower = name.to_ascii_lowercase();
    let extension = Path::new(&lower).extension().and_then(|e| e.to_str());

    in_denied_dir
        || lower == ".env"
        || lower.starts_with(".env.")
        || DENIED_NAMES.contains(&lower.as_str())
        || extension.is_some_and(|e| DENIED_EXTENSIONS.contains(&e))
}

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

    // Per component, not by substring: `v1..2.tex` is a legal name.
    if requested_path
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(SecurityError::ParentDir);
    }

    let stripped = requested.strip_prefix("./").unwrap_or(requested);
    let target = workspace_root.join(stripped);

    let abs = std::fs::canonicalize(&target).map_err(|_| SecurityError::NotFound)?;

    if !abs.starts_with(workspace_root) {
        return Err(SecurityError::EscapesWorkspace);
    }

    // `canonicalize` already resolved every symlink, so a link pointing out
    // of the workspace was caught by the `starts_with` check above.
    if is_denied(abs.strip_prefix(workspace_root).unwrap_or(&abs)) {
        return Err(SecurityError::Denied);
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
        assert!(
            matches!(err, SecurityError::EscapesWorkspace),
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
    fn dots_inside_a_name_are_not_traversal() {
        let (_dir, root) = workspace();
        std::fs::write(root.join("v1..2.tex"), "x").unwrap();
        assert!(validate_path(&root, "v1..2.tex", AccessKind::Read).is_ok());
        assert!(matches!(
            validate_path(&root, "sub/../doc.tex", AccessKind::Read),
            Err(SecurityError::ParentDir)
        ));
    }

    #[test]
    fn secrets_are_denied_for_read_and_write() {
        let (_dir, root) = workspace();
        std::fs::write(root.join(".env"), "KEY=1").unwrap();
        std::fs::write(root.join("server.pem"), "x").unwrap();
        std::fs::create_dir(root.join(".git")).unwrap();
        std::fs::write(root.join(".git/config.tex"), "x").unwrap();

        for name in [".env", "server.pem", ".git/config.tex"] {
            assert!(
                matches!(
                    validate_path(&root, name, AccessKind::Read),
                    Err(SecurityError::Denied)
                ),
                "{name} was readable"
            );
        }
        assert!(validate_path(&root, "doc.tex", AccessKind::Read).is_ok());
    }

    #[test]
    fn denylist_matches_names_not_substrings() {
        assert!(is_denied(Path::new(".env.local")));
        assert!(is_denied(Path::new("a/b/id_ed25519")));
        assert!(is_denied(Path::new(".ssh/known_hosts")));
        assert!(!is_denied(Path::new("environment.tex")));
        assert!(!is_denied(Path::new("monkey.tex")));
        assert!(!is_denied(Path::new("gitbook/intro.tex")));
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
