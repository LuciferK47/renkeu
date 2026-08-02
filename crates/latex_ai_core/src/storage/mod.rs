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

/// Result of a successful save.
#[derive(Debug)]
pub struct SaveOutcome {
    pub new_etag: String,
    pub history_id: i64,
    /// Unified diff old -> new, as stored in history.
    pub diff: String,
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

    /// Overwrite `path` with `content` if `expected_etag` still matches,
    /// recording a history row. `expected_etag == None` skips the check
    /// (first save / caller opted out).
    ///
    /// # Errors
    /// [`StorageError::EtagConflict`] when the file changed under the caller;
    /// otherwise IO/DB errors.
    pub async fn save_with_etag_history(
        &self,
        path: &Path,
        content: &str,
        expected_etag: Option<&str>,
        command: &str,
        edit_type: &str,
    ) -> Result<SaveOutcome, StorageError> {
        let _guard = self.write_lock.lock().await;
        let full_path = self.workspace_root.join(path);

        let old_content = if full_path.exists() {
            tokio::fs::read_to_string(&full_path).await?
        } else {
            String::new()
        };

        if full_path.exists() {
            if let Some(expected) = expected_etag {
                let actual = etag_of(&old_content);
                if expected != actual {
                    return Err(StorageError::EtagConflict {
                        expected: expected.to_owned(),
                        actual,
                    });
                }
            }
        }

        self.write_and_record(path, &full_path, &old_content, content, command, edit_type)
            .await
    }

    /// Apply the pre-selected `edits` to `path` atomically (all-or-nothing),
    /// with a single `ETag` check up front and a single history row tagged
    /// `history_tag` regardless of edit count (masterplan §12 M4).
    ///
    /// # Errors
    /// [`StorageError::EtagConflict`] if any edit carries a stale
    /// `expected_etag`; [`StorageError::Apply`] if an edit does not apply —
    /// nothing is written in either case.
    pub async fn atomic_apply_selected(
        &self,
        path: &Path,
        edits: &[&AgentEdit],
        history_tag: &str,
    ) -> Result<SaveOutcome, StorageError> {
        let _guard = self.write_lock.lock().await;
        let full_path = self.workspace_root.join(path);

        let old_content = if full_path.exists() {
            tokio::fs::read_to_string(&full_path).await?
        } else {
            String::new()
        };
        let current_etag = etag_of(&old_content);

        // Every etag is checked BEFORE anything is applied, so a stale
        // selection cannot half-write (masterplan §12 M4 audit: security).
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
            "apply-selected",
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
               AND (?2 = 0 OR edit_type = 'agent-apply')
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

        let tmp = full_path.with_extension("tmp");
        tokio::fs::write(&tmp, new_content).await?;
        tokio::fs::rename(&tmp, &full_path).await?;
        let new_etag = etag_of(new_content);

        let path_str = rel_path.to_string_lossy();
        let history_id: i64 = sqlx::query_scalar!(
            "INSERT INTO history (file_path, command, new_text, diff, edit_type) VALUES (?, ?, ?, ?, ?) RETURNING id",
            path_str,
            command,
            new_content,
            diff,
            edit_type
        )
        .fetch_one(&self.pool)
        .await?;

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
            .save_with_etag_history(Path::new("a.tex"), "new\n", None, "save", "manual-save")
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
                Some("not-the-etag"),
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
            .atomic_apply_selected(Path::new("a.tex"), &[&good, &bad], "apply-selected")
            .await
            .unwrap_err();
        assert!(matches!(err, StorageError::Apply(_)));
        assert_eq!(
            std::fs::read_to_string(dir.path().join("a.tex")).unwrap(),
            "alpha beta\n",
            "failed batch must not write anything"
        );

        let outcome = store
            .atomic_apply_selected(Path::new("a.tex"), &[&good], "apply-selected")
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
        let err = store
            .atomic_apply_selected(Path::new("a.tex"), &[&edit], "apply-selected")
            .await
            .unwrap_err();
        assert!(matches!(err, StorageError::EtagConflict { .. }));

        edit.expected_etag = Some(etag_of("content\n"));
        store
            .atomic_apply_selected(Path::new("a.tex"), &[&edit], "apply-selected")
            .await
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.path().join("a.tex")).unwrap(),
            "replaced\n"
        );
    }
}
