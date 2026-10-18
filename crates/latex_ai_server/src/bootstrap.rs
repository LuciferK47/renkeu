//! Application assembly shared by the daemon (`latex-ai serve`) and the
//! desktop shell, so the two cannot drift apart (they did: only one of them
//! once set `PRAGMA foreign_keys`).

use crate::{AppState, jobs::JobRegistry};
use anyhow::Context;
use latex_ai_core::config::{ConfiguredPlanner, EnvOverrides, Settings};
use latex_ai_core::project::ProjectData;
use latex_ai_core::storage::Store;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// What [`build_state`] needs to know about the environment it runs in.
pub struct BootOptions {
    /// Directory every API path is sandboxed to. Canonicalised here.
    pub workspace_root: PathBuf,
    /// App data directory: config, secrets, per-project history.
    pub home: PathBuf,
    pub env: EnvOverrides,
    /// Binary to run compile workers from. `None` means this very process
    /// (see `latex_ai_compiler::worker::run_if_requested`); tests, whose own
    /// binary is the test harness, name the real one.
    pub worker_exe: Option<PathBuf>,
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
            worker_exe: None,
        })
    }
}

/// File in the app data directory naming the project the desktop app opens.
const LAST_PROJECT_FILE: &str = "last_project";

/// The project the user last chose, if it is still a directory.
#[must_use]
pub fn remembered_project(home: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(home.join(LAST_PROJECT_FILE)).ok()?;
    let path = PathBuf::from(text.trim());
    (path.is_absolute() && path.is_dir()).then_some(path)
}

/// Remember `project` for the next start.
///
/// # Errors
/// When the app data directory cannot be written.
pub fn remember_project(home: &Path, project: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(home).context("creating the app data directory")?;
    let file = home.join(LAST_PROJECT_FILE);
    let tmp = home.join(format!("{LAST_PROJECT_FILE}.tmp"));
    std::fs::write(&tmp, project.to_string_lossy().as_bytes())
        .and_then(|()| std::fs::rename(&tmp, &file))
        .with_context(|| format!("writing {}", file.display()))
}

/// Where the desktop app opens when started: the remembered project, else the
/// directory it was launched from, except that a launcher starts apps in `/`
/// or the home directory and treating those as a project would index the
/// user's whole disk. They get a private starter folder instead.
#[must_use]
pub fn desktop_workspace(home: &Path, cwd: &Path, user_home: Option<&Path>) -> PathBuf {
    if let Some(p) = remembered_project(home) {
        return p;
    }
    let too_broad = cwd.parent().is_none() || user_home.is_some_and(|h| h == cwd);
    if too_broad {
        let starter = home.join("starter-project");
        // Best effort: build_state reports the failure with the path.
        let _ = std::fs::create_dir_all(&starter);
        return starter;
    }
    cwd.to_owned()
}

impl BootOptions {
    /// [`Self::from_process`] for the desktop shell, which picks its project
    /// with [`desktop_workspace`].
    ///
    /// # Errors
    /// When the current directory cannot be determined.
    pub fn for_desktop() -> anyhow::Result<Self> {
        let mut opts = Self::from_process()?;
        let cwd = std::env::current_dir().context("reading the current directory")?;
        let user_home = std::env::var_os("HOME").map(PathBuf::from);
        opts.workspace_root = desktop_workspace(&opts.home, &cwd, user_home.as_deref());
        Ok(opts)
    }
}

/// Tell the PDF renderer about an extra folder that may hold libpdfium (the
/// desktop bundle's resources). Call before the first request.
pub fn add_pdfium_search_dir(dir: impl Into<PathBuf>) {
    visual::render::add_search_dir(dir);
}

/// Bind pdfium now and report where it was found, for `latex-ai doctor`.
///
/// # Errors
/// When no usable library can be found.
pub fn check_pdfium() -> anyhow::Result<String> {
    visual::render::library_source().map_err(anyhow::Error::from)
}

/// Load settings, open and migrate the project database, and wire the state.
///
/// # Errors
/// Any startup failure, with context. An invalid config file is fatal on
/// purpose: running on a guessed configuration is worse than not starting.
pub async fn build_state(opts: BootOptions) -> anyhow::Result<AppState> {
    let workspace_root = std::fs::canonicalize(&opts.workspace_root)
        .with_context(|| format!("canonicalizing {}", opts.workspace_root.display()))?;

    let settings =
        Arc::new(Settings::open(opts.home.clone(), opts.env).context("loading configuration")?);

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
        Ok(n) => tracing::warn!(
            count = n,
            "recovered abandoned compile jobs from a previous run"
        ),
        Err(e) => tracing::warn!(error = %e, "compile-job recovery failed"),
    }

    let store = Arc::new(Store::new(pool, workspace_root.clone()));
    let worker_exe = match opts.worker_exe {
        Some(exe) => exe,
        None => std::env::current_exe().context("locating this executable for compile workers")?,
    };
    let compiler = Arc::new(latex_ai_compiler::Compiler::new(
        latex_ai_compiler::CompilerConfig::new(worker_exe, project.dir.join("builds")),
    ));

    Ok(AppState {
        planner: Arc::new(ConfiguredPlanner::new(Arc::clone(&settings))),
        compiler,
        store,
        jobs: Arc::new(JobRegistry::default()),
        proposals: Arc::new(crate::proposals::ProposalRegistry::default()),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_remembered_project_wins_but_only_while_it_exists() {
        let home = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let cwd = tempfile::tempdir().unwrap();
        assert_eq!(remembered_project(home.path()), None);

        remember_project(home.path(), project.path()).unwrap();
        assert_eq!(
            remembered_project(home.path()).as_deref(),
            Some(project.path())
        );
        assert_eq!(
            desktop_workspace(home.path(), cwd.path(), None),
            project.path()
        );

        let gone = project.path().to_owned();
        drop(project);
        assert_eq!(remembered_project(home.path()), None, "{gone:?} is deleted");
        assert_eq!(desktop_workspace(home.path(), cwd.path(), None), cwd.path());
    }

    #[test]
    fn a_launcher_start_in_root_or_home_gets_a_starter_folder_instead() {
        let home = tempfile::tempdir().unwrap();
        let user_home = tempfile::tempdir().unwrap();

        let from_home = desktop_workspace(home.path(), user_home.path(), Some(user_home.path()));
        assert_eq!(from_home, home.path().join("starter-project"));
        assert!(from_home.is_dir());

        let from_root = desktop_workspace(home.path(), Path::new("/"), None);
        assert_eq!(from_root, home.path().join("starter-project"));
    }

    #[test]
    fn a_relative_or_garbled_remembered_path_is_ignored() {
        let home = tempfile::tempdir().unwrap();
        std::fs::write(home.path().join("last_project"), "relative/dir").unwrap();
        assert_eq!(remembered_project(home.path()), None);
        std::fs::write(home.path().join("last_project"), [0xff, 0xfe]).unwrap();
        assert_eq!(remembered_project(home.path()), None);
    }
}
