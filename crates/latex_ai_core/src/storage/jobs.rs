//! Durable compile-job records (masterplan.md §12 M5).
//!
//! The in-memory registry the API serves from is process-local by design; this
//! is the audit trail beside it. Its one operational job is crash recovery: a
//! row still marked running when the process starts again was interrupted, and
//! is closed out as `abandoned` rather than left looking live forever.

use super::StorageError;
use sqlx::SqlitePool;

/// Terminal and non-terminal states a persisted job can be in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobStatus {
    Running,
    Succeeded,
    Failed,
    /// The process died mid-compile; set during startup recovery.
    Abandoned,
}

impl JobStatus {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Abandoned => "abandoned",
        }
    }
}

/// One persisted compile job.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct JobRow {
    pub job_id: String,
    pub file_path: String,
    pub status: String,
    pub error: Option<String>,
    pub created_at: Option<String>,
    pub completed_at: Option<String>,
}

/// Record a job as running.
///
/// # Errors
/// [`StorageError::Database`] on write failure.
pub async fn record_started(
    pool: &SqlitePool,
    job_id: &str,
    file_path: &str,
) -> Result<(), StorageError> {
    let status = JobStatus::Running.as_str();
    sqlx::query(
        "INSERT OR REPLACE INTO compile_queue (job_id, file_path, status) VALUES (?, ?, ?)",
    )
    .bind(job_id)
    .bind(file_path)
    .bind(status)
    .execute(pool)
    .await?;
    Ok(())
}

/// Close a job out with its outcome.
///
/// # Errors
/// [`StorageError::Database`] on write failure.
pub async fn record_finished(
    pool: &SqlitePool,
    job_id: &str,
    status: JobStatus,
    error: Option<&str>,
) -> Result<(), StorageError> {
    sqlx::query(
        "UPDATE compile_queue
         SET status = ?, error = ?, completed_at = CURRENT_TIMESTAMP
         WHERE job_id = ?",
    )
    .bind(status.as_str())
    .bind(error)
    .bind(job_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Close out jobs left running by a previous process. Returns how many were
/// recovered, so startup can log it.
///
/// # Errors
/// [`StorageError::Database`] on write failure.
pub async fn recover_abandoned(pool: &SqlitePool) -> Result<u64, StorageError> {
    let result = sqlx::query(
        "UPDATE compile_queue
         SET status = ?, error = 'abandoned', completed_at = CURRENT_TIMESTAMP
         WHERE status = ?",
    )
    .bind(JobStatus::Abandoned.as_str())
    .bind(JobStatus::Running.as_str())
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

/// Recent jobs, newest first.
///
/// # Errors
/// [`StorageError::Database`] on query failure.
pub async fn recent(pool: &SqlitePool, limit: i64) -> Result<Vec<JobRow>, StorageError> {
    let rows = sqlx::query_as::<_, JobRow>(
        "SELECT job_id, file_path, status, error,
                CAST(created_at AS TEXT) AS created_at,
                CAST(completed_at AS TEXT) AS completed_at
         FROM compile_queue ORDER BY rowid DESC LIMIT ?",
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// One compile diagnostic tied to the history row it belongs to
/// (masterplan §7.1; §12 M5 audit: data integrity requires the FK).
#[derive(Debug, Clone)]
pub struct CompileErrorRecord {
    pub iteration: u32,
    pub line: Option<u32>,
    pub file: Option<String>,
    pub message: String,
    pub error_type: String,
}

impl CompileErrorRecord {
    /// A whole-document failure with no parsed location — what Tectonic gives
    /// us today, since we do not yet parse its structured log.
    #[must_use]
    pub fn engine_failure(iteration: u32, message: impl Into<String>) -> Self {
        Self {
            iteration,
            line: None,
            file: None,
            message: message.into(),
            error_type: "engine".to_owned(),
        }
    }
}

/// Attach compile errors to a history entry.
///
/// The FK means a history row must already exist; the cascade means deleting
/// history cleans these up.
///
/// # Errors
/// [`StorageError::Database`] on write failure.
pub async fn record_compile_errors(
    pool: &SqlitePool,
    history_id: i64,
    errors: &[CompileErrorRecord],
) -> Result<(), StorageError> {
    for error in errors {
        sqlx::query(
            "INSERT INTO compile_errors (history_id, iteration, line, file, message, error_type)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(history_id)
        .bind(error.iteration)
        .bind(error.line)
        .bind(error.file.as_deref())
        .bind(&error.message)
        .bind(&error.error_type)
        .execute(pool)
        .await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn pool() -> SqlitePool {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .connect("sqlite::memory:")
            .await
            .unwrap();
        // Foreign keys are off by default in SQLite; the app enables them too.
        sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE history (
                id INTEGER PRIMARY KEY AUTOINCREMENT, file_path TEXT NOT NULL,
                command TEXT NOT NULL, new_text TEXT NOT NULL, diff TEXT NOT NULL,
                edit_type TEXT NOT NULL, base_text TEXT, created_at DATETIME DEFAULT CURRENT_TIMESTAMP)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "CREATE TABLE compile_queue (
                job_id TEXT PRIMARY KEY, file_path TEXT NOT NULL, status TEXT NOT NULL,
                error TEXT, created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                completed_at DATETIME)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "CREATE TABLE compile_errors (
                id INTEGER PRIMARY KEY AUTOINCREMENT, history_id INTEGER NOT NULL,
                iteration INTEGER NOT NULL, line INTEGER, file TEXT, message TEXT NOT NULL,
                error_type TEXT NOT NULL, created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                FOREIGN KEY (history_id) REFERENCES history(id) ON DELETE CASCADE)",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    async fn insert_history(pool: &SqlitePool) -> i64 {
        sqlx::query_scalar::<_, i64>(
            "INSERT INTO history (file_path, command, new_text, diff, edit_type)
             VALUES ('main.tex', 'c', 'n', 'd', 'agent-apply') RETURNING id",
        )
        .fetch_one(pool)
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn a_job_moves_from_running_to_a_terminal_state() {
        let pool = pool().await;
        record_started(&pool, "job-1", "main.tex").await.unwrap();
        assert_eq!(recent(&pool, 10).await.unwrap()[0].status, "running");

        record_finished(&pool, "job-1", JobStatus::Succeeded, None)
            .await
            .unwrap();
        let row = recent(&pool, 10).await.unwrap().remove(0);
        assert_eq!(row.status, "succeeded");
        assert!(row.completed_at.is_some());
        assert!(row.error.is_none());
    }

    #[tokio::test]
    async fn recovery_closes_out_jobs_left_running_by_a_dead_process() {
        let pool = pool().await;
        record_started(&pool, "interrupted", "main.tex")
            .await
            .unwrap();
        record_started(&pool, "finished", "other.tex")
            .await
            .unwrap();
        record_finished(&pool, "finished", JobStatus::Succeeded, None)
            .await
            .unwrap();

        let recovered = recover_abandoned(&pool).await.unwrap();
        assert_eq!(recovered, 1, "only the in-flight job is touched");

        let rows = recent(&pool, 10).await.unwrap();
        let interrupted = rows.iter().find(|r| r.job_id == "interrupted").unwrap();
        assert_eq!(interrupted.status, "abandoned");
        assert_eq!(interrupted.error.as_deref(), Some("abandoned"));

        let finished = rows.iter().find(|r| r.job_id == "finished").unwrap();
        assert_eq!(finished.status, "succeeded", "must not be reopened");

        // Running it again is a no-op — recovery is idempotent.
        assert_eq!(recover_abandoned(&pool).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn compile_errors_hang_off_a_real_history_row() {
        let pool = pool().await;
        let history_id = insert_history(&pool).await;

        record_compile_errors(
            &pool,
            history_id,
            &[
                CompileErrorRecord::engine_failure(1, "undefined control sequence"),
                CompileErrorRecord::engine_failure(2, "missing }"),
            ],
        )
        .await
        .unwrap();

        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM compile_errors WHERE history_id = ?")
                .bind(history_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(count, 2);
    }

    #[tokio::test]
    async fn an_error_cannot_reference_a_missing_history_row() {
        // masterplan §12 M5 audit (data integrity): every FK points at a row.
        let pool = pool().await;
        let err = record_compile_errors(
            &pool,
            9999,
            &[CompileErrorRecord::engine_failure(1, "orphan")],
        )
        .await;
        assert!(err.is_err(), "the foreign key must reject an orphan");
    }

    #[tokio::test]
    async fn deleting_history_cascades_to_its_errors() {
        let pool = pool().await;
        let history_id = insert_history(&pool).await;
        record_compile_errors(
            &pool,
            history_id,
            &[CompileErrorRecord::engine_failure(1, "boom")],
        )
        .await
        .unwrap();

        sqlx::query("DELETE FROM history WHERE id = ?")
            .bind(history_id)
            .execute(&pool)
            .await
            .unwrap();

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM compile_errors")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 0, "cascade should have removed the errors");
    }
}
