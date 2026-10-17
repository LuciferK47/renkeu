//! Per-project data location (D7).
//!
//! History used to live in `<workspace>/.latex-ai.db`, which put a database
//! (and its WAL files) inside the user's project, where it showed up in
//! `git status` and could be zipped and shared by accident. It now lives under
//! the app home, keyed by the project's canonical path:
//! `<home>/projects/<id>/history.db`.

use std::path::{Path, PathBuf};

/// Where the pre-M8 database lived, relative to the workspace root.
pub const LEGACY_DB_NAME: &str = ".latex-ai.db";

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ProjectError {
    #[error("preparing project data directory {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("importing the legacy history database: {0}")]
    Import(#[from] sqlx::Error),
}

/// Stable id for a project: the first 16 hex chars of blake3 over its
/// canonical path. Different spellings of one directory share an id only if
/// the caller canonicalises first (bootstrap does).
#[must_use]
pub fn project_id(canonical_root: &Path) -> String {
    let hash = blake3::hash(canonical_root.as_os_str().as_encoded_bytes());
    hex::encode(&hash.as_bytes()[..8])
}

/// Locations of one project's app-side data.
#[derive(Debug, Clone)]
pub struct ProjectData {
    pub id: String,
    pub dir: PathBuf,
    pub db_path: PathBuf,
}

impl ProjectData {
    #[must_use]
    pub fn locate(home: &Path, canonical_root: &Path) -> Self {
        let id = project_id(canonical_root);
        let dir = home.join("projects").join(&id);
        let db_path = dir.join("history.db");
        Self { id, dir, db_path }
    }

    /// Create the directory.
    ///
    /// # Errors
    /// [`ProjectError::Io`].
    pub async fn ensure_dir(&self) -> Result<(), ProjectError> {
        tokio::fs::create_dir_all(&self.dir)
            .await
            .map_err(|source| ProjectError::Io {
                path: self.dir.clone(),
                source,
            })
    }

    /// Bring a pre-M8 in-workspace database across, once.
    ///
    /// Uses `VACUUM INTO` rather than copying the file: the old database ran
    /// in WAL mode, so a plain copy of `.latex-ai.db` would silently drop
    /// whatever was still in the `-wal` file. The original is left untouched
    /// (the user can delete it); nothing happens if the new database exists
    /// already. Returns whether an import took place.
    ///
    /// # Errors
    /// [`ProjectError`] when the legacy database cannot be read or the copy
    /// cannot be published; the new database is not created in that case.
    pub async fn import_legacy(&self, workspace_root: &Path) -> Result<bool, ProjectError> {
        let legacy = workspace_root.join(LEGACY_DB_NAME);
        if self.db_path.exists() || !legacy.is_file() {
            return Ok(false);
        }
        self.ensure_dir().await?;

        let staging = self.dir.join("history.db.importing");
        // A previous crashed import leaves this behind; VACUUM INTO refuses
        // to overwrite.
        let _ = tokio::fs::remove_file(&staging).await;

        let url = format!("sqlite://{}?mode=ro", legacy.display());
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await?;
        let copied = sqlx::query("VACUUM INTO ?")
            .bind(staging.to_string_lossy().into_owned())
            .execute(&pool)
            .await;
        pool.close().await;
        copied?;

        tokio::fs::rename(&staging, &self.db_path)
            .await
            .map_err(|source| ProjectError::Io {
                path: self.db_path.clone(),
                source,
            })?;
        tracing::info!(
            from = %legacy.display(),
            to = %self.db_path.display(),
            "imported the legacy history database; the original was left in place"
        );
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_is_stable_and_distinguishes_projects() {
        let a = project_id(Path::new("/home/u/thesis"));
        assert_eq!(a, project_id(Path::new("/home/u/thesis")));
        assert_ne!(a, project_id(Path::new("/home/u/paper")));
        assert_eq!(a.len(), 16);
    }

    #[test]
    fn data_lives_under_home_not_in_the_project() {
        let d = ProjectData::locate(Path::new("/h/.latex-ai"), Path::new("/w/proj"));
        assert!(d.db_path.starts_with("/h/.latex-ai/projects"));
        assert!(d.db_path.ends_with("history.db"));
    }

    async fn legacy_db(dir: &Path) {
        let url = format!("sqlite://{}?mode=rwc", dir.join(LEGACY_DB_NAME).display());
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .unwrap();
        sqlx::query("CREATE TABLE t (v TEXT)").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO t VALUES ('kept')").execute(&pool).await.unwrap();
        // Deliberately not closed first: the row may still be in the WAL.
        std::mem::forget(pool);
    }

    #[tokio::test]
    async fn legacy_database_is_imported_once_and_left_in_place() {
        let home = tempfile::tempdir().unwrap();
        let ws = tempfile::tempdir().unwrap();
        legacy_db(ws.path()).await;
        let data = ProjectData::locate(home.path(), ws.path());

        assert!(data.import_legacy(ws.path()).await.unwrap());
        assert!(ws.path().join(LEGACY_DB_NAME).exists(), "original must stay");

        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .connect(&format!("sqlite://{}?mode=ro", data.db_path.display()))
            .await
            .unwrap();
        let (v,): (String,) = sqlx::query_as("SELECT v FROM t").fetch_one(&pool).await.unwrap();
        assert_eq!(v, "kept");

        // Second call: new DB exists, so nothing is overwritten.
        assert!(!data.import_legacy(ws.path()).await.unwrap());
    }

    #[tokio::test]
    async fn no_legacy_database_means_no_import() {
        let home = tempfile::tempdir().unwrap();
        let ws = tempfile::tempdir().unwrap();
        let data = ProjectData::locate(home.path(), ws.path());
        assert!(!data.import_legacy(ws.path()).await.unwrap());
        assert!(!data.db_path.exists());
    }
}
