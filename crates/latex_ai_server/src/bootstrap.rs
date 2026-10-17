//! Application assembly shared by the daemon (`latex-ai serve`) and the
//! desktop shell, so the two cannot drift apart (they did: only one of them
//! once set `PRAGMA foreign_keys`).

use crate::{AppState, jobs::JobRegistry};
use anyhow::Context;
use latex_ai_core::config::{ConfiguredPlanner, EnvOverrides, Settings};
use latex_ai_core::project::ProjectData;
use latex_ai_core::storage::Store;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use std::path::PathBuf;
use std::sync::Arc;

/// What [`build_state`] needs to know about the environment it runs in.
pub struct BootOptions {
    /// Directory every API path is sandboxed to. Canonicalised here.
    pub workspace_root: PathBuf,
    /// App data directory: config, secrets, per-project history.
    pub home: PathBuf,
    pub env: EnvOverrides,
}

impl BootOptions {
    /// Workspace = current directory, home and overrides from the process
    /// environment.
    ///
    /// # Errors
    /// When the current directory cannot be determined.
    pub fn from_process() -> anyhow::Result<Self> {
        Ok(Self {
            workspace_root: std::env::current_dir().context("reading the current directory")?,
            home: latex_ai_core::config::default_home(),
            env: EnvOverrides::from_process(),
        })
    }
}

/// Load settings, open and migrate the project database, and wire the state.
///
/// # Errors
/// Any startup failure, with context. An invalid config file is fatal on
/// purpose: running on a guessed configuration is worse than not starting.
pub async fn build_state(opts: BootOptions) -> anyhow::Result<AppState> {
    let workspace_root = std::fs::canonicalize(&opts.workspace_root)
        .with_context(|| format!("canonicalizing {}", opts.workspace_root.display()))?;

    let settings = Arc::new(
        Settings::open(opts.home.clone(), opts.env).context("loading configuration")?,
    );

    let project = ProjectData::locate(&opts.home, &workspace_root);
    project
        .ensure_dir()
        .await
        .context("creating the project data directory")?;
    if let Err(e) = project.import_legacy(&workspace_root).await {
        // Not fatal: history is a convenience. Say so and start fresh.
        tracing::warn!(error = %e, "could not import the legacy history database");
    }

    let pool = open_database(&project).await?;
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .context("running database migrations")?;

    // A job still marked running belongs to a process that died. Close those
    // out so the queue does not show phantom work forever.
    match latex_ai_core::storage::jobs::recover_abandoned(&pool).await {
        Ok(0) => {}
        Ok(n) => tracing::warn!(count = n, "recovered abandoned compile jobs from a previous run"),
        Err(e) => tracing::warn!(error = %e, "compile-job recovery failed"),
    }

    let store = Arc::new(Store::new(pool, workspace_root.clone()));
    let engine = Arc::new(latex_ai_compiler::Engine::new(10));
    let queue = Arc::new(latex_ai_compiler::queue::CompilationQueue::new(engine));

    Ok(AppState {
        planner: Arc::new(ConfiguredPlanner::new(Arc::clone(&settings))),
        queue,
        store,
        jobs: Arc::new(JobRegistry::default()),
        workspace_root,
        settings,
        metrics: Arc::new(latex_ai_core::metrics::Metrics::new()),
    })
}

/// `foreign_keys` is a per-connection pragma, so it must be part of the
/// connect options: a one-off `PRAGMA` after the pool exists only reaches
/// whichever connection happened to run it, and `compile_errors` relies on
/// its cascade (masterplan §12 M5 audit: data integrity).
async fn open_database(project: &ProjectData) -> anyhow::Result<sqlx::SqlitePool> {
    let options = SqliteConnectOptions::new()
        .filename(&project.db_path)
        .create_if_missing(true)
        .foreign_keys(true);
    SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await
        .with_context(|| format!("opening the history database {}", project.db_path.display()))
}
