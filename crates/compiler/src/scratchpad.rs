//! Per-session scratch working tree (masterplan.md §12 M2, Q3): agent edits
//! are applied to a copy under `~/.latex-ai/scratch/<session_id>/` and
//! compiled there, so the user's file is only touched after review/approval.
//!
//! Lifecycle per masterplan Q3: explicit [`ScratchPad::cleanup`] on
//! session close, plus an LRU cap of [`MAX_SESSIONS`] session dirs enforced at
//! creation time so abandoned sessions cannot accumulate forever.

use latex_ai_core::apply;
use latex_ai_core::compile::AgentEdit;
use std::path::{Path, PathBuf};
use thiserror::Error;

/// LRU cap on retained session dirs (masterplan Q3 default).
const MAX_SESSIONS: usize = 10;

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ScratchpadError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("main file has no file name: {0}")]
    NoFileName(PathBuf),
    #[error("edit failed to apply: {0}")]
    Apply(#[from] apply::ApplyError),
}

pub struct ScratchPad {
    pub root: PathBuf,
    pub main_file: PathBuf,
}

/// Default scratch root: `~/.latex-ai/scratch`, overridable for tests and
/// containers via `LATEX_AI_HOME`; falls back to the system temp dir when no
/// home is available.
#[must_use]
pub fn default_scratch_root() -> PathBuf {
    let base = std::env::var_os("LATEX_AI_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".latex-ai")))
        .unwrap_or_else(|| std::env::temp_dir().join(".latex-ai"));
    base.join("scratch")
}

impl ScratchPad {
    /// Create a session scratchpad under the default root and copy
    /// `main_file` into it.
    ///
    /// # Errors
    /// IO errors creating directories or copying the main file.
    pub fn create_for(session_id: &str, main_file: &Path) -> Result<Self, ScratchpadError> {
        Self::create_in(&default_scratch_root(), session_id, main_file)
    }

    /// [`Self::create_for`] with an explicit root (used by tests).
    ///
    /// # Errors
    /// IO errors creating directories or copying the main file.
    pub fn create_in(
        scratch_root: &Path,
        session_id: &str,
        main_file: &Path,
    ) -> Result<Self, ScratchpadError> {
        let root = scratch_root.join(session_id);
        std::fs::create_dir_all(&root)?;

        let file_name = main_file
            .file_name()
            .ok_or_else(|| ScratchpadError::NoFileName(main_file.to_path_buf()))?;
        let dest_main = root.join(file_name);
        std::fs::copy(main_file, &dest_main)?;

        evict_oldest_sessions(scratch_root, &root);

        Ok(Self {
            root,
            main_file: dest_main,
        })
    }

    /// Apply `edits` to the scratch copy (never the user's file) via
    /// [`latex_ai_core::apply`] — all-or-nothing per file, no subprocesses.
    ///
    /// # Errors
    /// [`ScratchpadError::Apply`] when an edit does not apply; the scratch
    /// file is left unmodified in that case.
    pub async fn apply_edits(&self, edits: &[AgentEdit]) -> Result<(), ScratchpadError> {
        // All edits in an M2 batch target the session's single main file; the
        // AI-supplied `file_path` is deliberately ignored so a malicious or
        // confused plan cannot write outside the scratch dir.
        let content = tokio::fs::read_to_string(&self.main_file).await?;
        let edit_refs: Vec<&AgentEdit> = edits.iter().collect();
        let new_content = apply::apply_edits(&content, &edit_refs)?;
        tokio::fs::write(&self.main_file, new_content).await?;
        Ok(())
    }

    /// Current content of the scratch main file.
    ///
    /// # Errors
    /// IO errors reading the file.
    pub async fn read_main(&self) -> Result<String, ScratchpadError> {
        Ok(tokio::fs::read_to_string(&self.main_file).await?)
    }

    /// Remove this session's scratch tree (masterplan Q3: cleanup on
    /// session close). Idempotent.
    pub fn cleanup(&self) {
        if let Err(e) = std::fs::remove_dir_all(&self.root) {
            if e.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(root = %self.root.display(), error = %e, "scratchpad cleanup failed");
            }
        }
    }

    #[must_use]
    pub fn pdf_path(&self) -> PathBuf {
        self.main_file.with_extension("pdf")
    }

    #[must_use]
    pub fn synctex_path(&self) -> PathBuf {
        self.main_file.with_extension("synctex.gz")
    }
}

/// Keep at most [`MAX_SESSIONS`] session dirs: delete the oldest (by mtime)
/// beyond the cap, never touching `current`. Best-effort — eviction failures
/// are logged, not fatal.
fn evict_oldest_sessions(scratch_root: &Path, current: &Path) {
    let Ok(entries) = std::fs::read_dir(scratch_root) else {
        return;
    };
    let mut sessions: Vec<(std::time::SystemTime, PathBuf)> = entries
        .flatten()
        .filter(|e| e.path().is_dir() && e.path() != current)
        .filter_map(|e| {
            let modified = e.metadata().and_then(|m| m.modified()).ok()?;
            Some((modified, e.path()))
        })
        .collect();

    if sessions.len() < MAX_SESSIONS {
        return;
    }
    sessions.sort_by_key(|(mtime, _)| *mtime);
    let excess = sessions.len() + 1 - MAX_SESSIONS; // +1 for `current`
    for (_, path) in sessions.into_iter().take(excess) {
        if let Err(e) = std::fs::remove_dir_all(&path) {
            tracing::warn!(path = %path.display(), error = %e, "scratch LRU eviction failed");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use latex_ai_core::compile::AgentOp;

    fn edit_replacing(text: &str, with: &str) -> AgentEdit {
        AgentEdit {
            file_path: "main.tex".into(),
            op: AgentOp::ReplaceRange,
            payload: None,
            text: Some(text.into()),
            start_line: None,
            end_line: None,
            line: None,
            insert_text: Some(with.into()),
            expected_etag: None,
            hunk_id: None,
        }
    }

    #[tokio::test]
    async fn copies_main_file_and_applies_edits_in_scratch_only() {
        let workspace = tempfile::tempdir().unwrap();
        let scratch = tempfile::tempdir().unwrap();
        let user_file = workspace.path().join("main.tex");
        std::fs::write(&user_file, "hello world\n").unwrap();

        let pad = ScratchPad::create_in(scratch.path(), "s1", &user_file).unwrap();
        pad.apply_edits(&[edit_replacing("world", "scratch")])
            .await
            .unwrap();

        assert_eq!(pad.read_main().await.unwrap(), "hello scratch\n");
        assert_eq!(
            std::fs::read_to_string(&user_file).unwrap(),
            "hello world\n",
            "user file must stay untouched"
        );

        pad.cleanup();
        assert!(!pad.root.exists(), "cleanup removes the session dir");
        pad.cleanup(); // idempotent
    }

    #[tokio::test]
    async fn failed_edit_leaves_scratch_unchanged() {
        let workspace = tempfile::tempdir().unwrap();
        let scratch = tempfile::tempdir().unwrap();
        let user_file = workspace.path().join("main.tex");
        std::fs::write(&user_file, "content\n").unwrap();

        let pad = ScratchPad::create_in(scratch.path(), "s2", &user_file).unwrap();
        let err = pad
            .apply_edits(&[edit_replacing("absent", "x")])
            .await
            .unwrap_err();
        assert!(matches!(err, ScratchpadError::Apply(_)));
        assert_eq!(pad.read_main().await.unwrap(), "content\n");
    }

    #[tokio::test]
    async fn lru_cap_evicts_oldest_sessions() {
        let workspace = tempfile::tempdir().unwrap();
        let scratch = tempfile::tempdir().unwrap();
        let user_file = workspace.path().join("main.tex");
        std::fs::write(&user_file, "x\n").unwrap();

        for i in 0..(MAX_SESSIONS + 3) {
            ScratchPad::create_in(scratch.path(), &format!("s{i:02}"), &user_file).unwrap();
            // mtime granularity: space creations apart just enough to order.
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }

        let remaining = std::fs::read_dir(scratch.path()).unwrap().count();
        assert!(
            remaining <= MAX_SESSIONS,
            "expected <= {MAX_SESSIONS} sessions, found {remaining}"
        );
        assert!(
            !scratch.path().join("s00").exists(),
            "oldest session should have been evicted"
        );
        assert!(
            scratch
                .path()
                .join(format!("s{:02}", MAX_SESSIONS + 2))
                .exists(),
            "newest session must survive"
        );
    }
}
