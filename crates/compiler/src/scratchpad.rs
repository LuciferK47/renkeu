//! A private mirror of the project for one agent run (masterplan §12 M2, Q3).
//!
//! Edits are applied here, never to the user's files, and compiled here, so
//! the project is touched only after a verified result is accepted. The
//! mirror lives for the whole run, which is the point: the second iteration's
//! edits land on top of the first's, so the planner is always correcting the
//! document it just changed (the first design copied the *original* file
//! afresh every iteration and silently discarded the earlier work).
//!
//! Source files (`.tex`, `.bib`, …) are copied; everything else (images,
//! fonts, PDFs) is linked to the original, because the agent never edits it
//! and copying a figure folder per run would be slow for no benefit. Edits
//! replace a file atomically instead of writing through, so they can never
//! reach the original via a link.
//!
//! Lifecycle: the directory is removed when the value is dropped — on
//! success, on cancel, on error, on panic — and at most [`MAX_SESSIONS`]
//! directories are kept, which also covers a process killed mid-run.

use crate::project::project_files;
use latex_ai_core::apply::{self, ApplyError};
use latex_ai_core::compile::AgentEdit;
use latex_ai_core::security::{has_writable_extension, is_denied};
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;
use thiserror::Error;

/// LRU cap on retained session dirs (masterplan Q3 default).
const MAX_SESSIONS: usize = 10;

/// Files copied rather than linked.
const COPIED_EXTENSIONS: &[&str] = &[
    "tex", "bib", "sty", "cls", "tikz", "bst", "def", "cfg", "clo",
];

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ScratchpadError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("edit failed to apply: {0}")]
    Apply(#[from] ApplyError),
    /// An edit named a file the agent may not change.
    #[error("`{0}` is not a source file the agent may edit")]
    BadPath(String),
}

pub struct ScratchProject {
    root: PathBuf,
    /// Content of each touched file as the project had it, recorded the first
    /// time an edit reaches the file.
    originals: Mutex<BTreeMap<String, String>>,
}

impl ScratchProject {
    /// Mirror `project_root` into `<scratch_root>/<session_id>`.
    ///
    /// # Errors
    /// IO errors creating the mirror or walking the project.
    pub fn create(
        scratch_root: &Path,
        session_id: &str,
        project_root: &Path,
    ) -> Result<Self, ScratchpadError> {
        let root = scratch_root.join(session_id);
        std::fs::create_dir_all(&root)?;
        // From here the directory is ours: if mirroring fails, Drop removes it.
        let this = Self {
            root,
            originals: Mutex::new(BTreeMap::new()),
        };

        for rel in project_files(project_root)? {
            let from = project_root.join(&rel);
            let to = this.root.join(&rel);
            if let Some(parent) = to.parent() {
                std::fs::create_dir_all(parent)?;
            }
            if is_source(&rel) {
                std::fs::copy(&from, &to)?;
            } else {
                link_or_copy(&from, &to)?;
            }
        }

        evict_oldest_sessions(scratch_root, &this.root);
        Ok(this)
    }

    /// The mirror's directory: compile from here.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Current content of a mirrored source file.
    ///
    /// # Errors
    /// [`ScratchpadError::BadPath`] for a path outside the mirror;
    /// IO errors reading it.
    pub async fn read(&self, rel: &str) -> Result<String, ScratchpadError> {
        let rel = checked(rel)?;
        Ok(tokio::fs::read_to_string(self.root.join(rel)).await?)
    }

    /// Apply `edits` to the mirror, all-or-nothing across every file they
    /// name. Returns the files that changed.
    ///
    /// # Errors
    /// [`ScratchpadError::BadPath`] for an edit aimed at a file the agent may
    /// not change, [`ScratchpadError::Apply`] when an edit does not apply. In
    /// both cases nothing was written.
    pub async fn apply(&self, edits: &[AgentEdit]) -> Result<Vec<String>, ScratchpadError> {
        let mut files = BTreeMap::new();
        for edit in edits {
            let rel = checked(&edit.file_path)?;
            let key = latex_ai_core::fsutil::to_slash(&rel);
            if let std::collections::btree_map::Entry::Vacant(slot) = files.entry(key) {
                let content = tokio::fs::read_to_string(self.root.join(&rel))
                    .await
                    .map_err(|_| ScratchpadError::BadPath(edit.file_path.clone()))?;
                slot.insert(content);
            }
        }

        // Pure and in memory: a failure here leaves the mirror as it was.
        let updated = apply::apply_to_files(&files, edits)?;

        let mut changed = Vec::new();
        for (rel, new_content) in updated {
            let old = &files[&rel];
            if *old == new_content {
                continue;
            }
            self.remember_original(&rel, old);
            latex_ai_core::fsutil::atomic_write(&self.root.join(&rel), new_content.as_bytes())
                .await?;
            changed.push(rel);
        }
        Ok(changed)
    }

    /// Every file the run has changed, as `(path, original, current)`.
    ///
    /// # Errors
    /// IO errors reading the mirror.
    pub async fn changes(&self) -> Result<Vec<(String, String, String)>, ScratchpadError> {
        let originals = self.lock().clone();
        let mut out = Vec::new();
        for (rel, original) in originals {
            let current = tokio::fs::read_to_string(self.root.join(&rel)).await?;
            if current != original {
                out.push((rel, original, current));
            }
        }
        Ok(out)
    }

    /// Remove the mirror now. Idempotent; also done on drop.
    pub fn cleanup(&self) {
        if let Err(e) = std::fs::remove_dir_all(&self.root)
            && e.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!(root = %self.root.display(), error = %e, "scratch cleanup failed");
        }
    }

    fn remember_original(&self, rel: &str, content: &str) {
        self.lock()
            .entry(rel.to_owned())
            .or_insert_with(|| content.to_owned());
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, String>> {
        match self.originals.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        }
    }
}

impl Drop for ScratchProject {
    fn drop(&mut self) {
        self.cleanup();
    }
}

fn is_source(rel: &Path) -> bool {
    rel.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| COPIED_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

#[cfg(unix)]
fn link_or_copy(from: &Path, to: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(from, to)
}

#[cfg(not(unix))]
fn link_or_copy(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::copy(from, to).map(|_| ())
}

/// A relative path the agent may edit: inside the project, not a secret, and
/// a source extension. Anything else — including whatever a confused or
/// hostile model puts in `file_path` — is refused.
fn checked(rel: &str) -> Result<PathBuf, ScratchpadError> {
    let bad = || ScratchpadError::BadPath(rel.to_owned());
    let path = Path::new(rel);
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::Normal(p) => out.push(p),
            Component::CurDir => {}
            _ => return Err(bad()),
        }
    }
    if out.as_os_str().is_empty() || is_denied(&out) || !has_writable_extension(&out) {
        return Err(bad());
    }
    Ok(out)
}

/// Keep at most [`MAX_SESSIONS`] session dirs: delete the oldest (by mtime)
/// beyond the cap, never touching `current`. Best-effort.
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

    fn replace(file: &str, text: &str, with: &str) -> AgentEdit {
        AgentEdit {
            file_path: file.into(),
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

    fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for (name, content) in files {
            let path = dir.path().join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, content).unwrap();
        }
        dir
    }

    #[tokio::test]
    async fn mirrors_the_whole_project_and_edits_only_the_mirror() {
        let p = project(&[
            ("main.tex", "hello world\n"),
            ("ch/a.tex", "a\n"),
            ("fig.png", "png"),
        ]);
        let scratch = tempfile::tempdir().unwrap();
        let pad = ScratchProject::create(scratch.path(), "s1", p.path()).unwrap();

        assert_eq!(pad.read("ch/a.tex").await.unwrap(), "a\n");
        assert!(
            pad.root().join("fig.png").exists(),
            "non-source files are linked in"
        );

        let changed = pad
            .apply(&[
                replace("main.tex", "world", "scratch"),
                replace("ch/a.tex", "a", "A"),
            ])
            .await
            .unwrap();
        assert_eq!(changed, ["ch/a.tex", "main.tex"]);
        assert_eq!(pad.read("main.tex").await.unwrap(), "hello scratch\n");
        assert_eq!(
            std::fs::read_to_string(p.path().join("main.tex")).unwrap(),
            "hello world\n"
        );
        assert_eq!(
            std::fs::read_to_string(p.path().join("ch/a.tex")).unwrap(),
            "a\n"
        );
    }

    #[tokio::test]
    async fn the_second_batch_builds_on_the_first() {
        let p = project(&[("main.tex", "one\n")]);
        let scratch = tempfile::tempdir().unwrap();
        let pad = ScratchProject::create(scratch.path(), "s2", p.path()).unwrap();

        pad.apply(&[replace("main.tex", "one", "two")])
            .await
            .unwrap();
        // This anchor exists only because the first batch ran.
        pad.apply(&[replace("main.tex", "two", "three")])
            .await
            .unwrap();

        assert_eq!(pad.read("main.tex").await.unwrap(), "three\n");
        let changes = pad.changes().await.unwrap();
        assert_eq!(
            changes,
            [(
                "main.tex".to_owned(),
                "one\n".to_owned(),
                "three\n".to_owned()
            )]
        );
    }

    #[tokio::test]
    async fn a_batch_that_fails_changes_nothing_in_any_file() {
        let p = project(&[("main.tex", "keep\n"), ("b.tex", "b\n")]);
        let scratch = tempfile::tempdir().unwrap();
        let pad = ScratchProject::create(scratch.path(), "s3", p.path()).unwrap();

        let err = pad
            .apply(&[
                replace("main.tex", "keep", "changed"),
                replace("b.tex", "absent", "x"),
            ])
            .await
            .unwrap_err();
        assert!(matches!(err, ScratchpadError::Apply(_)), "{err:?}");
        assert_eq!(
            pad.read("main.tex").await.unwrap(),
            "keep\n",
            "all-or-nothing across files"
        );
        assert_eq!(pad.changes().await.unwrap().len(), 0);
    }

    #[tokio::test]
    async fn edits_cannot_reach_outside_the_mirror_or_non_source_files() {
        let p = project(&[
            ("main.tex", "x\n"),
            (".env", "SECRET\n"),
            ("fig.png", "png"),
        ]);
        let scratch = tempfile::tempdir().unwrap();
        let pad = ScratchProject::create(scratch.path(), "s4", p.path()).unwrap();

        for bad in [
            "../escape.tex",
            "/etc/passwd",
            ".env",
            "fig.png",
            "",
            "sub/../../x.tex",
            "missing.tex",
        ] {
            let err = pad.apply(&[replace(bad, "x", "y")]).await.unwrap_err();
            assert!(
                matches!(err, ScratchpadError::BadPath(_)),
                "{bad:?}: {err:?}"
            );
        }
        assert!(!scratch.path().join("escape.tex").exists());
        assert!(
            !pad.root().join(".env").exists(),
            "secrets are not mirrored"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn editing_a_linked_looking_file_never_writes_through_to_the_original() {
        // A source file is copied, so replacing it cannot touch the original;
        // and even a symlinked one would be replaced, not written through.
        let p = project(&[("main.tex", "orig\n")]);
        let scratch = tempfile::tempdir().unwrap();
        let pad = ScratchProject::create(scratch.path(), "s5", p.path()).unwrap();
        std::fs::remove_file(pad.root().join("main.tex")).unwrap();
        std::os::unix::fs::symlink(p.path().join("main.tex"), pad.root().join("main.tex")).unwrap();

        pad.apply(&[replace("main.tex", "orig", "edited")])
            .await
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(p.path().join("main.tex")).unwrap(),
            "orig\n"
        );
        assert_eq!(pad.read("main.tex").await.unwrap(), "edited\n");
    }

    #[test]
    fn the_mirror_is_removed_on_drop_and_cleanup_is_idempotent() {
        let p = project(&[("main.tex", "x\n")]);
        let scratch = tempfile::tempdir().unwrap();
        let root;
        {
            let pad = ScratchProject::create(scratch.path(), "s6", p.path()).unwrap();
            root = pad.root().to_owned();
            assert!(root.exists());
            pad.cleanup();
            pad.cleanup();
            assert!(!root.exists());
        }
        let pad = ScratchProject::create(scratch.path(), "s7", p.path()).unwrap();
        let root = pad.root().to_owned();
        drop(pad);
        assert!(
            !root.exists(),
            "drop must clean up (cancel/panic paths rely on it)"
        );
    }

    #[tokio::test]
    async fn lru_cap_evicts_oldest_sessions() {
        let p = project(&[("main.tex", "x\n")]);
        let scratch = tempfile::tempdir().unwrap();
        // Keep each alive: dropping would delete them before the cap matters.
        let mut keep = Vec::new();
        for i in 0..(MAX_SESSIONS + 3) {
            keep.push(
                ScratchProject::create(scratch.path(), &format!("s{i:02}"), p.path()).unwrap(),
            );
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
                .exists()
        );
    }
}
