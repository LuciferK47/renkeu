//! Durable workspace writes: blake3 `ETag` optimistic concurrency, atomic
//! tmp-file + rename, and one `history` row per mutation (masterplan.md §4.5).

pub mod jobs;

use crate::apply;
use crate::compile::AgentEdit;
use sqlx::SqlitePool;
use std::path::{Path, PathBuf};
use thiserror::Error;
use tokio::sync::Mutex;

#[derive(Error, Debug)]
#[non_exhaustive]
pub enum StorageError {
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("etag conflict: expected {expected}, found {actual}")]
    EtagConflict { expected: String, actual: String },
    #[error("file already exists")]
    AlreadyExists,
    #[error("file not found")]
    NotFound,
    #[error("edit failed to apply: {0}")]
    Apply(#[from] apply::ApplyError),
}

/// One row of the `history` table.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct HistoryRow {
    pub id: i64,
    pub file_path: String,
    pub command: String,
    pub diff: String,
    pub edit_type: String,
    pub created_at: Option<String>,
}

/// What the caller knows about the file it is about to overwrite. There is
/// deliberately no "don't care" variant: a blind overwrite is how concurrent
/// edits were silently lost.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub enum Precondition<'a> {
    /// The file must exist and still hash to this `ETag`.
    Etag(&'a str),
    /// The file must not exist yet.
    CreateNew,
}

/// `edit_type` values written by AI-driven mutations; the history panel's
/// "agent only" filter matches exactly these.
pub const AGENT_EDIT_TYPES: &[&str] = &["agent-apply", "apply-selected"];

/// Result of a successful save.
#[derive(Debug)]
pub struct SaveOutcome {
    pub new_etag: String,
    pub history_id: i64,
    /// Unified diff old -> new, as stored in history.
    pub diff: String,
}

async fn read_existing(full_path: &Path) -> Result<Option<String>, StorageError> {
    match tokio::fs::read_to_string(full_path).await {
        Ok(content) => Ok(Some(content)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

fn check_precondition(
    precondition: Precondition<'_>,
    current: Option<&str>,
) -> Result<(), StorageError> {
    match (precondition, current) {
        (Precondition::CreateNew, None) => Ok(()),
        (Precondition::CreateNew, Some(_)) => Err(StorageError::AlreadyExists),
        (Precondition::Etag(_), None) => Err(StorageError::NotFound),
        (Precondition::Etag(expected), Some(content)) => {
            let actual = etag_of(content);
            if expected == actual {
                Ok(())
            } else {
                Err(StorageError::EtagConflict {
                    expected: expected.to_owned(),
                    actual,
                })
            }
        }
    }
}

/// blake3 content hash in hex — the `ETag` format used across the API.
#[must_use]
pub fn etag_of(content: &str) -> String {
    hex::encode(blake3::hash(content.as_bytes()).as_bytes())
}

pub struct Store {
    pool: SqlitePool,
    workspace_root: PathBuf,
    /// Serializes mutations. Single-user local server: one global write lock
    /// gives the per-file atomicity masterplan §12 M4 requires, with no lock
    /// table to maintain.
    write_lock: Mutex<()>,
}

impl Store {
    /// The underlying pool, for the durable-job helpers in [`jobs`].
    #[must_use]
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    #[must_use]
    pub fn new(pool: SqlitePool, workspace_root: PathBuf) -> Self {
        Self {
            pool,
            workspace_root,
            write_lock: Mutex::new(()),
        }
    }

    /// Read a workspace file and its current `ETag`.
    ///
    /// # Errors
    /// [`StorageError::Io`] if the file cannot be read.
    pub async fn read_with_etag(&self, path: &Path) -> Result<(String, String), StorageError> {
        let full_path = self.workspace_root.join(path);
        let content = tokio::fs::read_to_string(&full_path).await?;
        let etag = etag_of(&content);
        Ok((content, etag))
    }

    /// Overwrite `path` with `content` if `precondition` holds, recording a
    /// history row.
    ///
    /// # Errors
    /// [`StorageError::EtagConflict`] when the file changed under the caller,
    /// [`StorageError::AlreadyExists`] / [`StorageError::NotFound`] when the
    /// precondition names the wrong existence state; otherwise IO/DB errors.
    pub async fn save_with_etag_history(
        &self,
        path: &Path,
        content: &str,
        precondition: Precondition<'_>,
        command: &str,
        edit_type: &str,
    ) -> Result<SaveOutcome, StorageError> {
        let _guard = self.write_lock.lock().await;
        let full_path = self.workspace_root.join(path);
        let old_content = read_existing(&full_path).await?;
        check_precondition(precondition, old_content.as_deref())?;

        self.write_and_record(
            path,
            &full_path,
            old_content.as_deref().unwrap_or_default(),
            content,
            command,
            edit_type,
        )
        .await
    }

    /// Apply the pre-selected `edits` to `path` atomically (all-or-nothing),
    /// with a single `ETag` check up front and a single history row tagged
    /// `history_tag` regardless of edit count (masterplan §12 M4).
    ///
    /// `expected_etag` is the file version the edits were proposed against.
    /// It is required at request level: checking only per-edit etags let a
    /// `rewrite_file` without one clobber concurrent changes.
    ///
    /// # Errors
    /// [`StorageError::EtagConflict`] if the file (or any edit carrying its
    /// own `expected_etag`) is stale; [`StorageError::Apply`] if an edit does
    /// not apply — nothing is written in either case.
    pub async fn atomic_apply_selected(
        &self,
        path: &Path,
        edits: &[&AgentEdit],
        expected_etag: &str,
        command: &str,
        history_tag: &str,
    ) -> Result<SaveOutcome, StorageError> {
        let _guard = self.write_lock.lock().await;
        let full_path = self.workspace_root.join(path);
        let old_content = read_existing(&full_path).await?;
        check_precondition(Precondition::Etag(expected_etag), old_content.as_deref())?;
        let old_content = old_content.unwrap_or_default();

        // Every etag is checked BEFORE anything is applied, so a stale
        // selection cannot half-write (masterplan §12 M4 audit: security).
        let current_etag = etag_of(&old_content);
        for edit in edits {
            if let Some(expected) = &edit.expected_etag {
                if expected != &current_etag {
                    return Err(StorageError::EtagConflict {
                        expected: expected.clone(),
                        actual: current_etag,
                    });
                }
            }
        }

        // Identical hunks from two prompts collapse to one application.
        let unique = crate::compile::dedupe_hunks(edits);
        let new_content = apply::apply_edits(&old_content, &unique)?;

        self.write_and_record(
            path,
            &full_path,
            &old_content,
            &new_content,
            command,
            history_tag,
        )
        .await
    }

    /// Recent history rows, newest first, optionally scoped to one file
    /// and/or to agent-authored edits.
    ///
    /// # Errors
    /// [`StorageError::Database`] on query failure.
    pub async fn recent_history(
        &self,
        file_path: Option<&str>,
        agent_only: bool,
        limit: i64,
    ) -> Result<Vec<HistoryRow>, StorageError> {
        let rows = sqlx::query_as::<_, HistoryRow>(
            "SELECT id, file_path, command, diff, edit_type,
                    CAST(created_at AS TEXT) AS created_at
             FROM history
             WHERE (?1 IS NULL OR file_path = ?1)
               AND (?2 = 0 OR edit_type IN ('agent-apply', 'apply-selected'))
             ORDER BY id DESC
             LIMIT ?3",
        )
        .bind(file_path)
        .bind(i32::from(agent_only))
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// Shared tail: diff (BEFORE writing — the old code diffed after the
    /// overwrite and stored empty diffs forever), atomic write, history row.
    async fn write_and_record(
        &self,
        rel_path: &Path,
        full_path: &Path,
        old_content: &str,
        new_content: &str,
        command: &str,
        edit_type: &str,
    ) -> Result<SaveOutcome, StorageError> {
        let header = rel_path.to_string_lossy();
        let diff = similar::TextDiff::from_lines(old_content, new_content)
            .unified_diff()
            .header(&header, &header)
            .to_string();

        let new_etag = etag_of(new_content);
        let path_str = rel_path.to_string_lossy();

        // History row first, inside a transaction, then the file, then
        // commit. A failed write rolls the row back, and a failed commit
        // restores the old bytes, so the row and the file never disagree
        // (previously the rename happened first: a DB error returned 500
        // for a file that had in fact changed, and the next save 409'd).
        let mut tx = self.pool.begin().await?;
        let history_id: i64 = sqlx::query_scalar!(
            "INSERT INTO history (file_path, command, new_text, diff, edit_type) VALUES (?, ?, ?, ?, ?) RETURNING id",
            path_str,
            command,
            new_content,
            diff,
            edit_type
        )
        .fetch_one(&mut *tx)
        .await?;

        crate::fsutil::atomic_write(full_path, new_content.as_bytes()).await?;

        if let Err(e) = tx.commit().await {
            if let Err(restore) =
                crate::fsutil::atomic_write(full_path, old_content.as_bytes()).await
            {
                tracing::error!(error = %restore, path = %full_path.display(), "could not restore file after failed history commit");
            }
            return Err(e.into());
        }

        Ok(SaveOutcome {
            new_etag,
            history_id,
            diff,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compile::{AgentEdit, AgentOp};

    async fn test_store() -> (Store, tempfile::TempDir) {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                file_path TEXT NOT NULL,
                command TEXT NOT NULL,
                new_text TEXT NOT NULL,
                diff TEXT NOT NULL,
                edit_type TEXT NOT NULL,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP
            )",
        )
        .execute(&pool)
        .await
        .unwrap();

        let dir = tempfile::tempdir().unwrap();
        (Store::new(pool, dir.path().to_path_buf()), dir)
    }

    #[tokio::test]
    async fn save_records_nonempty_diff_and_new_etag() {
        let (store, dir) = test_store().await;
        std::fs::write(dir.path().join("a.tex"), "old\n").unwrap();

        let outcome = store
            .save_with_etag_history(
                Path::new("a.tex"),
                "new\n",
                Precondition::Etag(&etag_of("old\n")),
                "save",
                "manual-save",
            )
            .await
            .unwrap();

        assert_eq!(outcome.new_etag, etag_of("new\n"));
        assert!(
            outcome.diff.contains("-old"),
            "diff lost the old side: {}",
            outcome.diff
        );
        assert!(
            outcome.diff.contains("+new"),
            "diff lost the new side: {}",
            outcome.diff
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("a.tex")).unwrap(),
            "new\n"
        );
    }

    #[tokio::test]
    async fn stale_etag_is_rejected_and_file_untouched() {
        let (store, dir) = test_store().await;
        std::fs::write(dir.path().join("a.tex"), "current\n").unwrap();

        let err = store
            .save_with_etag_history(
                Path::new("a.tex"),
                "clobber\n",
                Precondition::Etag("not-the-etag"),
                "save",
                "manual-save",
            )
            .await
            .unwrap_err();

        assert!(matches!(err, StorageError::EtagConflict { .. }));
        assert_eq!(
            std::fs::read_to_string(dir.path().join("a.tex")).unwrap(),
            "current\n"
        );
    }

    #[tokio::test]
    async fn apply_selected_is_all_or_nothing() {
        let (store, dir) = test_store().await;
        std::fs::write(dir.path().join("a.tex"), "alpha beta\n").unwrap();

        let good = AgentEdit {
            file_path: "a.tex".into(),
            op: AgentOp::ReplaceRange,
            payload: None,
            text: Some("alpha".into()),
            start_line: None,
            end_line: None,
            line: None,
            insert_text: Some("ALPHA".into()),
            expected_etag: None,
            hunk_id: None,
        };
        let mut bad = good.clone();
        bad.text = Some("no-such-text".into());

        let err = store
            .atomic_apply_selected(
                Path::new("a.tex"),
                &[&good, &bad],
                &etag_of("alpha beta\n"),
                "cmd",
                "apply-selected",
            )
            .await
            .unwrap_err();
        assert!(matches!(err, StorageError::Apply(_)));
        assert_eq!(
            std::fs::read_to_string(dir.path().join("a.tex")).unwrap(),
            "alpha beta\n",
            "failed batch must not write anything"
        );

        let outcome = store
            .atomic_apply_selected(
                Path::new("a.tex"),
                &[&good],
                &etag_of("alpha beta\n"),
                "cmd",
                "apply-selected",
            )
            .await
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.path().join("a.tex")).unwrap(),
            "ALPHA beta\n"
        );
        assert!(outcome.history_id >= 1);
    }

    #[tokio::test]
    async fn apply_selected_rejects_stale_expected_etag() {
        let (store, dir) = test_store().await;
        std::fs::write(dir.path().join("a.tex"), "content\n").unwrap();

        let mut edit = AgentEdit {
            file_path: "a.tex".into(),
            op: AgentOp::RewriteFile,
            payload: Some("replaced\n".into()),
            text: None,
            start_line: None,
            end_line: None,
            line: None,
            insert_text: None,
            expected_etag: Some("stale".into()),
            hunk_id: None,
        };
        let current = etag_of("content\n");
        let err = store
            .atomic_apply_selected(Path::new("a.tex"), &[&edit], &current, "cmd", "apply-selected")
            .await
            .unwrap_err();
        assert!(matches!(err, StorageError::EtagConflict { .. }));

        edit.expected_etag = Some(current.clone());
        store
            .atomic_apply_selected(Path::new("a.tex"), &[&edit], &current, "cmd", "apply-selected")
            .await
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.path().join("a.tex")).unwrap(),
            "replaced\n"
        );
    }

    #[tokio::test]
    async fn request_level_etag_guards_a_rewrite_without_per_edit_etag() {
        let (store, dir) = test_store().await;
        std::fs::write(dir.path().join("a.tex"), "theirs\n").unwrap();
        let rewrite = AgentEdit {
            file_path: "a.tex".into(),
            op: AgentOp::RewriteFile,
            payload: Some("mine\n".into()),
            text: None,
            start_line: None,
            end_line: None,
            line: None,
            insert_text: None,
            expected_etag: None,
            hunk_id: None,
        };
        let err = store
            .atomic_apply_selected(
                Path::new("a.tex"),
                &[&rewrite],
                &etag_of("what-i-saw\n"),
                "cmd",
                "apply-selected",
            )
            .await
            .unwrap_err();
        assert!(matches!(err, StorageError::EtagConflict { .. }));
        assert_eq!(
            std::fs::read_to_string(dir.path().join("a.tex")).unwrap(),
            "theirs\n"
        );
    }

    #[tokio::test]
    async fn create_new_refuses_to_overwrite_and_etag_refuses_a_missing_file() {
        let (store, dir) = test_store().await;
        std::fs::write(dir.path().join("a.tex"), "x\n").unwrap();

        let exists = store
            .save_with_etag_history(
                Path::new("a.tex"),
                "y\n",
                Precondition::CreateNew,
                "save",
                "manual-save",
            )
            .await
            .unwrap_err();
        assert!(matches!(exists, StorageError::AlreadyExists));

        let missing = store
            .save_with_etag_history(
                Path::new("ghost.tex"),
                "y\n",
                Precondition::Etag("whatever"),
                "save",
                "manual-save",
            )
            .await
            .unwrap_err();
        assert!(matches!(missing, StorageError::NotFound));

        store
            .save_with_etag_history(
                Path::new("fresh.tex"),
                "y\n",
                Precondition::CreateNew,
                "save",
                "manual-save",
            )
            .await
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.path().join("fresh.tex")).unwrap(),
            "y\n"
        );
    }

    #[tokio::test]
    async fn failed_write_leaves_no_history_row() {
        let (store, dir) = test_store().await;
        std::fs::write(dir.path().join("a.tex"), "old\n").unwrap();
        // Make the target unreplaceable: a directory sits where the temp
        // rename must land after we swap the file for a directory.
        std::fs::remove_file(dir.path().join("a.tex")).unwrap();
        std::fs::create_dir(dir.path().join("a.tex")).unwrap();

        let result = store
            .save_with_etag_history(
                Path::new("a.tex"),
                "new\n",
                Precondition::CreateNew,
                "save",
                "manual-save",
            )
            .await;
        assert!(result.is_err());

        let rows = store.recent_history(None, false, 10).await.unwrap();
        assert!(rows.is_empty(), "history row survived a failed write: {rows:?}");
    }

    #[tokio::test]
    async fn agent_filter_matches_apply_selected_and_keeps_the_instruction() {
        let (store, dir) = test_store().await;
        std::fs::write(dir.path().join("a.tex"), "alpha\n").unwrap();
        let edit = AgentEdit {
            file_path: "a.tex".into(),
            op: AgentOp::ReplaceRange,
            payload: None,
            text: Some("alpha".into()),
            start_line: None,
            end_line: None,
            line: None,
            insert_text: Some("beta".into()),
            expected_etag: None,
            hunk_id: None,
        };
        store
            .atomic_apply_selected(
                Path::new("a.tex"),
                &[&edit],
                &etag_of("alpha\n"),
                "make it beta",
                "apply-selected",
            )
            .await
            .unwrap();
        store
            .save_with_etag_history(
                Path::new("a.tex"),
                "gamma\n",
                Precondition::Etag(&etag_of("beta\n")),
                "save",
                "manual-save",
            )
            .await
            .unwrap();

        let agent = store.recent_history(None, true, 10).await.unwrap();
        assert_eq!(agent.len(), 1);
        assert_eq!(agent[0].command, "make it beta");
    }
}
