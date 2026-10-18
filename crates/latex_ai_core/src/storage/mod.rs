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
    #[error("edit rejected: {0}")]
    Policy(#[from] crate::policy::PolicyViolation),
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
    /// Whether the text this entry replaced was recorded, i.e. whether the
    /// change can be undone from the history.
    pub has_before: bool,
}

/// One history entry with the text it recorded, for restoring a version.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct HistoryDetail {
    pub id: i64,
    pub file_path: String,
    pub edit_type: String,
    pub created_at: Option<String>,
    /// The file's content right after this entry.
    pub text_after: String,
    /// The content it replaced, when that was recorded (a delete, or an
    /// autosave burst merged into one row); otherwise `None`.
    pub text_before: Option<String>,
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

/// `edit_type` of a save the editor made on its own timer.
pub const AUTOSAVE: &str = "autosave";

/// An autosave within this many seconds of the previous autosave of the same
/// file, with nothing else written in between, is folded into that history
/// row instead of adding a new one. Typing a paragraph would otherwise leave
/// dozens of near-identical rows and bury the saves that matter.
pub const AUTOSAVE_MERGE_WINDOW_SECS: i64 = 300;

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

/// [`etag_of`] for raw bytes, so binary files get the same kind of tag.
#[must_use]
pub fn etag_of_bytes(content: &[u8]) -> String {
    hex::encode(blake3::hash(content).as_bytes())
}

/// One file's part of a multi-file change.
#[derive(Debug, Clone)]
pub struct FileChange<'a> {
    pub path: &'a Path,
    /// What the file must still contain for the change to be valid.
    pub expected_etag: &'a str,
    pub new_content: &'a str,
}

/// What [`Store::apply_file_set`] did to one file.
#[derive(Debug, Clone)]
pub struct FileOutcome {
    pub path: PathBuf,
    pub history_id: i64,
    pub new_etag: String,
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

        // The edits come back from the client, so they are as untrusted as the
        // model's output: the lint runs here, at the one place that writes.
        crate::policy::lint_edits(edits.iter().copied(), &old_content)?;

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

    /// Write several files as one all-or-nothing change.
    ///
    /// Every file's `ETag` is checked before anything is written. Then, in
    /// one database transaction, a history row per file is inserted and the
    /// files are replaced; if any step fails, the files already written are
    /// put back and the transaction rolls back, so the project is either
    /// fully changed or exactly as it was. (A multi-file agent change that
    /// half-applied would leave a project that does not compile.)
    ///
    /// Files whose new content equals the old are skipped, not recorded.
    ///
    /// # Errors
    /// [`StorageError::EtagConflict`] naming the first stale file;
    /// [`StorageError::NotFound`] if a file is missing; IO/DB errors, after
    /// restoring what was written.
    pub async fn apply_file_set(
        &self,
        changes: &[FileChange<'_>],
        command: &str,
        edit_type: &str,
    ) -> Result<Vec<FileOutcome>, StorageError> {
        let _guard = self.write_lock.lock().await;

        // Phase 1: read and check everything; nothing is written yet.
        let mut staged = Vec::with_capacity(changes.len());
        for change in changes {
            let full_path = self.workspace_root.join(change.path);
            let old = read_existing(&full_path).await?;
            check_precondition(Precondition::Etag(change.expected_etag), old.as_deref())?;
            let old = old.unwrap_or_default();
            if old != change.new_content {
                staged.push((change, full_path, old));
            }
        }

        // Phase 2: rows and files together, restoring on any failure.
        let mut tx = self.pool.begin().await?;
        let mut written: Vec<(PathBuf, String)> = Vec::new();
        let mut outcomes = Vec::with_capacity(staged.len());

        let result: Result<(), StorageError> = async {
            for (change, full_path, old) in &staged {
                let header = change.path.to_string_lossy();
                let diff = similar::TextDiff::from_lines(old.as_str(), change.new_content)
                    .unified_diff()
                    .header(&header, &header)
                    .to_string();
                let history_id: i64 = sqlx::query_scalar!(
                    "INSERT INTO history (file_path, command, new_text, diff, edit_type) VALUES (?, ?, ?, ?, ?) RETURNING id",
                    header,
                    command,
                    change.new_content,
                    diff,
                    edit_type
                )
                .fetch_one(&mut *tx)
                .await?;
                record_before(&mut tx, history_id, old).await?;

                crate::fsutil::atomic_write(full_path, change.new_content.as_bytes()).await?;
                written.push((full_path.clone(), old.clone()));
                outcomes.push(FileOutcome {
                    path: change.path.to_path_buf(),
                    history_id,
                    new_etag: etag_of(change.new_content),
                });
            }
            Ok(())
        }
        .await;

        let result = match result {
            Ok(()) => tx.commit().await.map_err(StorageError::from),
            Err(e) => {
                // Dropping `tx` rolls the rows back; the files are on us.
                drop(tx);
                Err(e)
            }
        };
        if let Err(e) = result {
            for (path, old) in written.iter().rev() {
                if let Err(restore) = crate::fsutil::atomic_write(path, old.as_bytes()).await {
                    tracing::error!(error = %restore, path = %path.display(), "could not restore a file after a failed multi-file change");
                }
            }
            return Err(e);
        }
        Ok(outcomes)
    }

    /// Hold the write lock while the caller moves or creates files, so a save
    /// cannot land between its check and its change.
    pub async fn lock_writes(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.write_lock.lock().await
    }

    /// Delete a file if it still has the content the caller saw, keeping the
    /// removed text in the history row (`base_text`) so it can be restored.
    ///
    /// The `ETag` is the hash of the file's bytes, so images and other
    /// non-text files can be deleted under the same rule as sources.
    ///
    /// # Errors
    /// [`StorageError::EtagConflict`] when the file changed, [`StorageError::NotFound`]
    /// when it is gone; nothing is removed in either case.
    pub async fn delete_with_etag_history(
        &self,
        path: &Path,
        expected_etag: &str,
        command: &str,
    ) -> Result<i64, StorageError> {
        let _guard = self.write_lock.lock().await;
        let full_path = self.workspace_root.join(path);
        let bytes = match tokio::fs::read(&full_path).await {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(StorageError::NotFound);
            }
            Err(e) => return Err(e.into()),
        };
        let actual = hex::encode(blake3::hash(&bytes).as_bytes());
        if actual != expected_etag {
            return Err(StorageError::EtagConflict {
                expected: expected_etag.to_owned(),
                actual,
            });
        }

        let header = path.to_string_lossy();
        let text = String::from_utf8(bytes.clone()).ok();
        let diff = match &text {
            Some(t) => similar::TextDiff::from_lines(t.as_str(), "")
                .unified_diff()
                .header(&header, &header)
                .to_string(),
            None => format!("(binary file {header} removed)\n"),
        };

        let mut tx = self.pool.begin().await?;
        let history_id: i64 = sqlx::query_scalar(
            "INSERT INTO history (file_path, command, new_text, diff, edit_type, base_text)
             VALUES (?, ?, '', ?, 'delete', ?) RETURNING id",
        )
        .bind(&*header)
        .bind(command)
        .bind(&diff)
        .bind(text.as_deref())
        .fetch_one(&mut *tx)
        .await?;

        tokio::fs::remove_file(&full_path).await?;
        if let Err(e) = tx.commit().await {
            if let Err(restore) = crate::fsutil::atomic_write(&full_path, &bytes).await {
                tracing::error!(error = %restore, path = %full_path.display(), "could not restore a file after a failed delete record");
            }
            return Err(e.into());
        }
        Ok(history_id)
    }

    /// Make history follow a renamed file (or every file under a renamed
    /// directory) so its past stays reachable under its new name.
    ///
    /// # Errors
    /// Database errors.
    pub async fn rename_history(&self, from: &str, to: &str) -> Result<(), StorageError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("UPDATE history SET file_path = ? WHERE file_path = ?")
            .bind(to)
            .bind(from)
            .execute(&mut *tx)
            .await?;
        // A directory: rewrite the prefix. `substr` rather than LIKE, so the
        // `%` and `_` in a name are not wildcards.
        let prefix = format!("{from}/");
        sqlx::query(
            "UPDATE history SET file_path = ? || substr(file_path, ?) WHERE substr(file_path, 1, ?) = ?",
        )
        .bind(format!("{to}/"))
        .bind(i64::try_from(prefix.chars().count()).unwrap_or(i64::MAX) + 1)
        .bind(i64::try_from(prefix.chars().count()).unwrap_or(i64::MAX))
        .bind(&prefix)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
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
                    CAST(created_at AS TEXT) AS created_at,
                    base_text IS NOT NULL AS has_before
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

    /// One entry with its recorded text, or `None` if there is no such row.
    ///
    /// # Errors
    /// Database errors.
    pub async fn history_detail(&self, id: i64) -> Result<Option<HistoryDetail>, StorageError> {
        Ok(sqlx::query_as::<_, HistoryDetail>(
            "SELECT id, file_path, edit_type, CAST(created_at AS TEXT) AS created_at,
                    new_text AS text_after, base_text AS text_before
             FROM history WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?)
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
        let new_etag = etag_of(new_content);
        let path_str = rel_path.to_string_lossy();

        // History row first, inside a transaction, then the file, then
        // commit. A failed write rolls the row back, and a failed commit
        // restores the old bytes, so the row and the file never disagree
        // (previously the rename happened first: a DB error returned 500
        // for a file that had in fact changed, and the next save 409'd).
        let mut tx = self.pool.begin().await?;

        // A burst of autosaves is one history entry whose diff runs from
        // where the burst started to where it is now.
        let merge = if edit_type == AUTOSAVE {
            mergeable_autosave(&mut tx, &path_str, old_content).await?
        } else {
            None
        };
        let base = merge.as_ref().map_or(old_content, |m| m.base.as_str());
        let diff = similar::TextDiff::from_lines(base, new_content)
            .unified_diff()
            .header(&header, &header)
            .to_string();

        let history_id: i64 = if let Some(m) = &merge {
            sqlx::query(
                "UPDATE history SET new_text = ?, diff = ?, command = ?, created_at = CURRENT_TIMESTAMP WHERE id = ?",
            )
            .bind(new_content)
            .bind(&diff)
            .bind(command)
            .bind(m.id)
            .execute(&mut *tx)
            .await?;
            m.id
        } else {
            let id: i64 = sqlx::query_scalar!(
                "INSERT INTO history (file_path, command, new_text, diff, edit_type) VALUES (?, ?, ?, ?, ?) RETURNING id",
                path_str,
                command,
                new_content,
                diff,
                edit_type
            )
            .fetch_one(&mut *tx)
            .await?;
            // What this row replaced: it is what "undo this change" restores,
            // and for an autosave it also marks where a burst began, which the
            // next autosave needs in order to join it.
            record_before(&mut tx, id, old_content).await?;
            id
        };

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

/// Keep the text a history row replaced.
async fn record_before(
    tx: &mut sqlx::SqliteConnection,
    id: i64,
    before: &str,
) -> Result<(), StorageError> {
    sqlx::query("UPDATE history SET base_text = ? WHERE id = ?")
        .bind(before)
        .bind(id)
        .execute(tx)
        .await?;
    Ok(())
}

/// The previous autosave row of this file, if a new autosave may join it.
struct Mergeable {
    id: i64,
    base: String,
}

/// A row is joinable when it is an autosave of this file that is recent, that
/// holds exactly what is on disk now (so nothing else touched the file since),
/// and that remembers where its burst began.
async fn mergeable_autosave(
    tx: &mut sqlx::SqliteConnection,
    path: &str,
    current_content: &str,
) -> Result<Option<Mergeable>, StorageError> {
    let last: Option<(i64, String, String, Option<String>, i64)> = sqlx::query_as(
        "SELECT id, edit_type, new_text, base_text,
                CAST(strftime('%s','now') AS INTEGER) - CAST(strftime('%s', created_at) AS INTEGER)
         FROM history WHERE file_path = ? ORDER BY id DESC LIMIT 1",
    )
    .bind(path)
    .fetch_optional(&mut *tx)
    .await?;

    Ok(last.and_then(|(id, kind, new_text, base, age)| {
        let joinable =
            kind == AUTOSAVE && new_text == current_content && age <= AUTOSAVE_MERGE_WINDOW_SECS;
        base.filter(|_| joinable).map(|base| Mergeable { id, base })
    }))
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
                base_text TEXT,
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
            .atomic_apply_selected(
                Path::new("a.tex"),
                &[&edit],
                &current,
                "cmd",
                "apply-selected",
            )
            .await
            .unwrap_err();
        assert!(matches!(err, StorageError::EtagConflict { .. }));

        edit.expected_etag = Some(current.clone());
        store
            .atomic_apply_selected(
                Path::new("a.tex"),
                &[&edit],
                &current,
                "cmd",
                "apply-selected",
            )
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
        assert!(
            rows.is_empty(),
            "history row survived a failed write: {rows:?}"
        );
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

    #[tokio::test]
    async fn file_set_writes_every_file_with_its_own_history_row() {
        let (store, dir) = test_store().await;
        std::fs::write(dir.path().join("a.tex"), "a1\n").unwrap();
        std::fs::write(dir.path().join("b.tex"), "b1\n").unwrap();

        let out = store
            .apply_file_set(
                &[
                    FileChange {
                        path: Path::new("a.tex"),
                        expected_etag: &etag_of("a1\n"),
                        new_content: "a2\n",
                    },
                    FileChange {
                        path: Path::new("b.tex"),
                        expected_etag: &etag_of("b1\n"),
                        new_content: "b2\n",
                    },
                ],
                "fix both",
                "agent-apply",
            )
            .await
            .unwrap();

        assert_eq!(out.len(), 2);
        assert_ne!(out[0].history_id, out[1].history_id);
        assert_eq!(out[1].new_etag, etag_of("b2\n"));
        assert_eq!(
            std::fs::read_to_string(dir.path().join("a.tex")).unwrap(),
            "a2\n"
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("b.tex")).unwrap(),
            "b2\n"
        );
        let rows = store.recent_history(None, true, 10).await.unwrap();
        assert_eq!(rows.len(), 2);
    }

    #[tokio::test]
    async fn one_stale_file_stops_the_whole_set_before_any_write() {
        let (store, dir) = test_store().await;
        std::fs::write(dir.path().join("a.tex"), "a1\n").unwrap();
        std::fs::write(dir.path().join("b.tex"), "b-changed-meanwhile\n").unwrap();

        let err = store
            .apply_file_set(
                &[
                    FileChange {
                        path: Path::new("a.tex"),
                        expected_etag: &etag_of("a1\n"),
                        new_content: "a2\n",
                    },
                    FileChange {
                        path: Path::new("b.tex"),
                        expected_etag: &etag_of("b1\n"),
                        new_content: "b2\n",
                    },
                ],
                "x",
                "agent-apply",
            )
            .await
            .unwrap_err();

        assert!(matches!(err, StorageError::EtagConflict { .. }), "{err:?}");
        assert_eq!(
            std::fs::read_to_string(dir.path().join("a.tex")).unwrap(),
            "a1\n",
            "a.tex must not change"
        );
        assert!(
            store
                .recent_history(None, false, 10)
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn a_failure_midway_restores_the_files_already_written_and_the_rows() {
        let (store, dir) = test_store().await;
        std::fs::write(dir.path().join("a.tex"), "a1\n").unwrap();
        // b.tex is a directory when the write is attempted, so replacing it
        // fails after a.tex has already been replaced.
        std::fs::create_dir(dir.path().join("b.tex")).unwrap();

        let err = store
            .apply_file_set(
                &[
                    FileChange {
                        path: Path::new("a.tex"),
                        expected_etag: &etag_of("a1\n"),
                        new_content: "a2\n",
                    },
                    FileChange {
                        path: Path::new("b.tex"),
                        expected_etag: &etag_of(""),
                        new_content: "b2\n",
                    },
                ],
                "x",
                "agent-apply",
            )
            .await
            .unwrap_err();

        assert!(
            matches!(err, StorageError::Io(_) | StorageError::NotFound),
            "{err:?}"
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("a.tex")).unwrap(),
            "a1\n",
            "a.tex must be put back"
        );
        assert!(
            store
                .recent_history(None, false, 10)
                .await
                .unwrap()
                .is_empty(),
            "rows must roll back"
        );
    }

    #[tokio::test]
    async fn unchanged_files_are_skipped_and_leave_no_history() {
        let (store, dir) = test_store().await;
        std::fs::write(dir.path().join("a.tex"), "same\n").unwrap();
        let out = store
            .apply_file_set(
                &[FileChange {
                    path: Path::new("a.tex"),
                    expected_etag: &etag_of("same\n"),
                    new_content: "same\n",
                }],
                "x",
                "agent-apply",
            )
            .await
            .unwrap();
        assert!(out.is_empty());
        assert!(
            store
                .recent_history(None, false, 10)
                .await
                .unwrap()
                .is_empty()
        );
    }

    // ---- autosave merging ------------------------------------------------

    async fn autosave_store() -> (Store, tempfile::TempDir) {
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
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                base_text TEXT
            )",
        )
        .execute(&pool)
        .await
        .unwrap();
        let dir = tempfile::tempdir().unwrap();
        (Store::new(pool, dir.path().to_path_buf()), dir)
    }

    async fn save(store: &Store, old: &str, new: &str, kind: &str) -> i64 {
        store
            .save_with_etag_history(
                Path::new("a.tex"),
                new,
                Precondition::Etag(&etag_of(old)),
                "save",
                kind,
            )
            .await
            .unwrap()
            .history_id
    }

    #[tokio::test]
    async fn a_burst_of_autosaves_is_one_row_whose_diff_spans_the_burst() {
        let (store, dir) = autosave_store().await;
        std::fs::write(dir.path().join("a.tex"), "v0\n").unwrap();

        let first = save(&store, "v0\n", "v1\n", AUTOSAVE).await;
        let second = save(&store, "v1\n", "v2\n", AUTOSAVE).await;
        let third = save(&store, "v2\n", "v3\n", AUTOSAVE).await;
        assert_eq!((first, second), (first, first));
        assert_eq!(third, first, "one row for the whole burst");

        let rows = store.recent_history(None, false, 10).await.unwrap();
        assert_eq!(rows.len(), 1);
        // From where the burst began (v0) to where it ended (v3), not v2 -> v3.
        assert!(
            rows[0].diff.contains("-v0") && rows[0].diff.contains("+v3"),
            "{}",
            rows[0].diff
        );
        assert!(!rows[0].diff.contains("v2"), "{}", rows[0].diff);
    }

    #[tokio::test]
    async fn a_manual_save_or_someone_elses_write_ends_the_burst() {
        let (store, dir) = autosave_store().await;
        std::fs::write(dir.path().join("a.tex"), "v0\n").unwrap();

        let a = save(&store, "v0\n", "v1\n", AUTOSAVE).await;
        let manual = save(&store, "v1\n", "v2\n", "manual-save").await;
        let b = save(&store, "v2\n", "v3\n", AUTOSAVE).await;
        assert!(
            a != manual && manual != b,
            "a manual save is its own row, and the next autosave starts afresh"
        );

        // The file changed behind the autosave's back (another process).
        std::fs::write(dir.path().join("a.tex"), "elsewhere\n").unwrap();
        let c = save(&store, "elsewhere\n", "v4\n", AUTOSAVE).await;
        assert_ne!(b, c, "must not merge over a change it did not make");
        assert_eq!(
            store.recent_history(None, false, 10).await.unwrap().len(),
            4
        );
    }

    #[tokio::test]
    async fn a_stale_burst_is_not_merged_into() {
        let (store, dir) = autosave_store().await;
        std::fs::write(dir.path().join("a.tex"), "v0\n").unwrap();
        let a = save(&store, "v0\n", "v1\n", AUTOSAVE).await;
        sqlx::query("UPDATE history SET created_at = datetime('now', '-1 hour') WHERE id = ?")
            .bind(a)
            .execute(store.pool())
            .await
            .unwrap();

        let b = save(&store, "v1\n", "v2\n", AUTOSAVE).await;
        assert_ne!(a, b, "an hour-old burst is closed");
    }

    #[tokio::test]
    async fn delete_needs_the_current_etag_and_keeps_the_text_in_history() {
        let (store, dir) = autosave_store().await;
        std::fs::write(dir.path().join("a.tex"), "precious\n").unwrap();

        let stale = store
            .delete_with_etag_history(Path::new("a.tex"), "nope", "delete")
            .await;
        assert!(
            matches!(stale, Err(StorageError::EtagConflict { .. })),
            "{stale:?}"
        );
        assert!(
            dir.path().join("a.tex").exists(),
            "a refused delete removes nothing"
        );
        assert!(
            store
                .recent_history(None, false, 10)
                .await
                .unwrap()
                .is_empty()
        );

        let id = store
            .delete_with_etag_history(Path::new("a.tex"), &etag_of("precious\n"), "delete a.tex")
            .await
            .unwrap();
        assert!(!dir.path().join("a.tex").exists());
        let base: Option<String> = sqlx::query_scalar("SELECT base_text FROM history WHERE id = ?")
            .bind(id)
            .fetch_one(store.pool())
            .await
            .unwrap();
        assert_eq!(
            base.as_deref(),
            Some("precious\n"),
            "the removed text must be recoverable"
        );
        let rows = store.recent_history(None, false, 10).await.unwrap();
        assert_eq!(rows[0].edit_type, "delete");
        assert!(rows[0].diff.contains("-precious"));
    }

    #[tokio::test]
    async fn deleting_a_binary_file_records_that_without_its_content() {
        let (store, dir) = autosave_store().await;
        std::fs::write(dir.path().join("fig.png"), [0xff, 0xfe, 0x00, 0x80]).unwrap();
        let etag = hex::encode(blake3::hash(&[0xff, 0xfe, 0x00, 0x80]).as_bytes());

        store
            .delete_with_etag_history(Path::new("fig.png"), &etag, "delete")
            .await
            .unwrap();

        assert!(!dir.path().join("fig.png").exists());
        let rows = store.recent_history(None, false, 10).await.unwrap();
        assert!(rows[0].diff.contains("binary file"), "{}", rows[0].diff);
        assert!(matches!(
            store
                .delete_with_etag_history(Path::new("fig.png"), &etag, "again")
                .await,
            Err(StorageError::NotFound)
        ));
    }

    #[tokio::test]
    async fn history_follows_a_renamed_file_and_a_renamed_directory() {
        let (store, dir) = autosave_store().await;
        for (path, text) in [
            ("a.tex", "1"),
            ("ch/b.tex", "2"),
            ("ch2/c.tex", "3"),
            ("100%/d.tex", "4"),
        ] {
            sqlx::query("INSERT INTO history (file_path, command, new_text, diff, edit_type) VALUES (?, 'c', ?, 'd', 'manual-save')")
                .bind(path).bind(text).execute(store.pool()).await.unwrap();
        }
        let _ = &dir;

        store.rename_history("a.tex", "z.tex").await.unwrap();
        store.rename_history("ch", "chapters").await.unwrap();
        store.rename_history("100%", "pct").await.unwrap();

        let mut paths: Vec<String> = store
            .recent_history(None, false, 10)
            .await
            .unwrap()
            .into_iter()
            .map(|r| r.file_path)
            .collect();
        paths.sort();
        assert_eq!(
            paths,
            ["ch2/c.tex", "chapters/b.tex", "pct/d.tex", "z.tex"],
            "ch2 must not be caught by renaming ch"
        );
    }

    #[tokio::test]
    async fn history_detail_returns_the_recorded_text_and_none_for_a_missing_row() {
        let (store, dir) = autosave_store().await;
        std::fs::write(dir.path().join("a.tex"), "v1\n").unwrap();
        let saved = store
            .save_with_etag_history(
                Path::new("a.tex"),
                "v2\n",
                Precondition::Etag(&etag_of("v1\n")),
                "save",
                "manual-save",
            )
            .await
            .unwrap();
        let detail = store
            .history_detail(saved.history_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(detail.file_path, "a.tex");
        assert_eq!(detail.text_after, "v2\n");
        assert_eq!(
            detail.text_before.as_deref(),
            Some("v1\n"),
            "every save keeps what it replaced, so it can be undone"
        );
        let listed = store.recent_history(Some("a.tex"), false, 5).await.unwrap();
        assert!(listed[0].has_before);

        let removed = store
            .delete_with_etag_history(Path::new("a.tex"), &etag_of("v2\n"), "delete")
            .await
            .unwrap();
        let gone = store.history_detail(removed).await.unwrap().unwrap();
        assert_eq!(gone.edit_type, "delete");
        assert_eq!(gone.text_after, "");
        assert_eq!(
            gone.text_before.as_deref(),
            Some("v2\n"),
            "a delete keeps what was removed"
        );

        assert!(store.history_detail(9999).await.unwrap().is_none());
    }
}
