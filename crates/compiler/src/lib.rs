//! LaTeX compilation: Tectonic in a killable worker process, multi-file
//! projects, structured diagnostics, and a content-addressed build store.
//!
//! The pieces:
//! - [`worker`]: the child process that runs the engine (self-exec);
//! - [`project`]: main-file detection, fingerprinting, log-name resolution;
//! - [`diagnostics`]: `.log` and `.blg` parsed into problems with a file and line;
//! - [`builds`]: finished compiles, kept outside the project;
//! - [`Compiler`]: ties them together, with timeout and cancellation.
#![forbid(unsafe_code)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod builds;
/// Structured diagnostics now live in the `texlog` crate; re-exported here so
/// existing paths keep working.
pub use texlog as diagnostics;
mod engine;
pub mod project;
pub mod scratchpad;
pub mod worker;

use builds::{Build, Builds, Manifest};
use diagnostics::{Diagnostic, DiagnosticKind, Severity};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;
use worker::{BLG, LOG, PDF, RESULT, WORKER_ARG, WorkerRequest, WorkerResult};

/// Why a compile produced no verdict. A document that simply has TeX errors
/// is *not* one of these: that is a normal [`Build`] with `success == false`.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CompileError {
    /// The caller cancelled; the worker was killed.
    #[error("compilation cancelled")]
    Cancelled,
    /// The document ran past its budget (an infinite macro loop, say); the
    /// worker was killed and the next compile is unaffected.
    #[error("compilation timed out after {secs}s")]
    Timeout { secs: u64 },
    /// The worker died without reporting (killed, out of memory, crashed).
    #[error("the compile worker crashed ({status}){}", if .stderr.is_empty() { String::new() } else { format!(": {}", .stderr) })]
    WorkerCrashed { status: String, stderr: String },
    #[error("starting the compile worker `{}`: {source}", .exe.display())]
    Spawn {
        exe: PathBuf,
        source: std::io::Error,
    },
    #[error("compile I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("the compile worker returned an unreadable result: {0}")]
    Protocol(String),
    #[error("the compile queue is closed")]
    QueueClosed,
}

pub struct CompilerConfig {
    /// Binary to run in worker mode (see [`worker::run_if_requested`]).
    pub worker_exe: PathBuf,
    /// Where finished builds are kept.
    pub builds_dir: PathBuf,
    /// Compiles allowed at once. Each is its own process, so they do not
    /// share Tectonic's lock; two keeps a second tab responsive without
    /// letting a burst of saves swamp the machine.
    pub max_parallel: usize,
    /// Builds retained per store.
    pub keep_builds: usize,
}

impl CompilerConfig {
    #[must_use]
    pub fn new(worker_exe: PathBuf, builds_dir: PathBuf) -> Self {
        Self {
            worker_exe,
            builds_dir,
            max_parallel: 2,
            keep_builds: 12,
        }
    }
}

/// One compile request.
#[derive(Debug, Clone)]
pub struct CompileJob {
    /// Canonical project root.
    pub root: PathBuf,
    /// Main file, project-relative.
    pub main: String,
    pub synctex: bool,
    /// Where to load the TeX bundle from (a URL or a local path); `None` = Tectonic's default.
    pub bundle: Option<String>,
    pub timeout: Duration,
}

/// What an agent iteration needs from a throw-away compile.
#[derive(Debug)]
pub struct ScratchOutput {
    pub success: bool,
    pub pdf: Vec<u8>,
    pub diagnostics: Vec<Diagnostic>,
}

pub struct Compiler {
    worker_exe: PathBuf,
    builds: Builds,
    permits: Arc<Semaphore>,
}

impl Compiler {
    #[must_use]
    pub fn new(config: CompilerConfig) -> Self {
        Self {
            worker_exe: config.worker_exe,
            builds: Builds::new(config.builds_dir, config.keep_builds),
            permits: Arc::new(Semaphore::new(config.max_parallel.max(1))),
        }
    }

    #[must_use]
    pub fn builds(&self) -> &Builds {
        &self.builds
    }

    /// Compile `job` and keep the result in the build store.
    ///
    /// Identical inputs (same [`project::fingerprint`]) return the existing
    /// build without running the engine; that includes a build that failed,
    /// since the same broken input fails the same way. Timeouts, cancels and
    /// crashes are never stored.
    ///
    /// # Errors
    /// [`CompileError`] when no verdict was reached.
    pub async fn compile(
        &self,
        job: &CompileJob,
        cancel: &CancellationToken,
    ) -> Result<Build, CompileError> {
        let id = {
            let (root, main, synctex) = (job.root.clone(), job.main.clone(), job.synctex);
            blocking(move || project::fingerprint(&root, &main, synctex)).await??
        };

        if let Some(hit) = self.builds.get(&id).await {
            tracing::debug!(build = %id, "compile cache hit");
            return Ok(hit);
        }

        let _permit = self.acquire(cancel).await?;
        // The compile ahead of us may have been this very project.
        if let Some(hit) = self.builds.get(&id).await {
            return Ok(hit);
        }

        let scratch = self.builds.new_scratch_dir().await?;
        let executed = match self.execute(job, &scratch, cancel).await {
            Ok(e) => e,
            Err(e) => {
                self.builds.discard(&scratch).await;
                return Err(e);
            }
        };

        let manifest = Manifest {
            build_id: id,
            main: job.main.clone(),
            success: executed.success,
            has_pdf: executed.result.has_pdf,
            has_synctex: executed.result.has_synctex,
            created: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs()),
            diagnostics: executed.diagnostics,
        };
        match self.builds.publish(&scratch, manifest).await {
            Ok(build) => Ok(build),
            Err(e) => {
                self.builds.discard(&scratch).await;
                Err(e.into())
            }
        }
    }

    /// Compile without storing anything: the agent loop's verification step.
    /// The PDF comes back in memory.
    ///
    /// # Errors
    /// [`CompileError`] when no verdict was reached.
    pub async fn compile_scratch(
        &self,
        job: &CompileJob,
        cancel: &CancellationToken,
    ) -> Result<ScratchOutput, CompileError> {
        let _permit = self.acquire(cancel).await?;
        let scratch = self.builds.new_scratch_dir().await?;
        let outcome = async {
            let executed = self.execute(job, &scratch, cancel).await?;
            let pdf = if executed.result.has_pdf {
                tokio::fs::read(scratch.join(PDF)).await?
            } else {
                Vec::new()
            };
            Ok(ScratchOutput {
                success: executed.success,
                pdf,
                diagnostics: executed.diagnostics,
            })
        }
        .await;
        self.builds.discard(&scratch).await;
        outcome
    }

    async fn acquire(
        &self,
        cancel: &CancellationToken,
    ) -> Result<tokio::sync::OwnedSemaphorePermit, CompileError> {
        tokio::select! {
            permit = Arc::clone(&self.permits).acquire_owned() => {
                permit.map_err(|_| CompileError::QueueClosed)
            }
            () = cancel.cancelled() => Err(CompileError::Cancelled),
        }
    }

    /// Run the worker into `scratch` and turn its files into a verdict.
    async fn execute(
        &self,
        job: &CompileJob,
        scratch: &Path,
        cancel: &CancellationToken,
    ) -> Result<Executed, CompileError> {
        let hidden = {
            let root = job.root.clone();
            blocking(move || project::denied_files(&root)).await??
        };
        let request = WorkerRequest {
            root: job.root.clone(),
            main: job.main.clone(),
            synctex: job.synctex,
            bundle: job.bundle.clone(),
            out_dir: scratch.to_owned(),
            hidden,
        };
        let result = self.run_worker(&request, job.timeout, cancel).await?;
        let diagnostics = self.diagnose(job, scratch, &result).await;
        let success = result.tex_ok && result.has_pdf;
        Ok(Executed {
            success,
            result,
            diagnostics,
        })
    }

    async fn diagnose(
        &self,
        job: &CompileJob,
        scratch: &Path,
        result: &WorkerResult,
    ) -> Vec<Diagnostic> {
        let main_dir = Path::new(&job.main)
            .parent()
            .map_or_else(|| job.root.clone(), |p| job.root.join(p));
        let resolve = |token: &str| project::resolve_log_name(&job.root, &main_dir, token);

        let read = |name: &'static str| async move {
            tokio::fs::read(scratch.join(name))
                .await
                .map(|b| String::from_utf8_lossy(&b).into_owned())
                .unwrap_or_default()
        };
        let mut diagnostics = if result.has_log {
            diagnostics::parse_tex_log(&read(LOG).await, &resolve)
        } else {
            Vec::new()
        };
        if result.has_blg {
            let mut bib = diagnostics::parse_bibtex_log(&read(BLG).await);
            for d in &mut bib {
                // `.bib` names in the .blg are relative to the main file.
                if let Some(f) = d.file.as_deref().and_then(resolve) {
                    d.file = Some(f);
                }
            }
            diagnostics.extend(bib);
        }

        // An error TeX raised after the last file closed (a runaway argument
        // at end of input) has no file; the document it happened in is the
        // one we compiled.
        for d in &mut diagnostics {
            if d.severity == Severity::Error && d.file.is_none() {
                d.file = Some(job.main.clone());
            }
        }

        let failed = !(result.tex_ok && result.has_pdf);
        if failed && !diagnostics.iter().any(|d| d.severity == Severity::Error) {
            // No parsable error (the engine could not even start, or died
            // before logging): say what we do know rather than nothing.
            diagnostics.insert(
                0,
                Diagnostic {
                    severity: Severity::Error,
                    kind: DiagnosticKind::Fatal,
                    file: Some(job.main.clone()),
                    line: None,
                    message: result
                        .engine_error
                        .clone()
                        .unwrap_or_else(|| "the engine produced no PDF".to_owned()),
                    context: None,
                },
            );
        }
        diagnostics
    }

    async fn run_worker(
        &self,
        request: &WorkerRequest,
        timeout: Duration,
        cancel: &CancellationToken,
    ) -> Result<WorkerResult, CompileError> {
        let payload =
            serde_json::to_vec(request).map_err(|e| CompileError::Protocol(e.to_string()))?;

        let mut command = tokio::process::Command::new(&self.worker_exe);
        command
            .arg(WORKER_ARG)
            .env_clear()
            .envs(worker_env())
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            // A backstop only: the paths below kill and reap explicitly.
            .kill_on_drop(true);
        let mut child = command.spawn().map_err(|source| CompileError::Spawn {
            exe: self.worker_exe.clone(),
            source,
        })?;

        if let Some(mut stdin) = child.stdin.take() {
            // A worker that dies before reading shows up as a crash below.
            if let Err(e) = stdin.write_all(&payload).await {
                tracing::debug!(error = %e, "worker closed stdin early");
            }
        }
        let stderr = child.stderr.take();
        let stderr_tail = tokio::spawn(async move {
            use tokio::io::AsyncReadExt;
            let mut buf = Vec::new();
            if let Some(s) = stderr {
                let _ = s.take(8 * 1024).read_to_end(&mut buf).await;
            }
            String::from_utf8_lossy(&buf).trim().to_owned()
        });

        let finished = tokio::select! {
            status = child.wait() => Ok(status?),
            () = cancel.cancelled() => Err(CompileError::Cancelled),
            () = tokio::time::sleep(timeout) => Err(CompileError::Timeout { secs: timeout.as_secs() }),
        };
        let status = match finished {
            Ok(s) => s,
            Err(e) => {
                // Kill, then reap so no zombie is left behind.
                if let Err(kill) = child.start_kill() {
                    tracing::warn!(error = %kill, "could not kill the compile worker");
                }
                let _ = child.wait().await;
                stderr_tail.abort();
                return Err(e);
            }
        };

        let tail = stderr_tail.await.unwrap_or_default();
        let text = tokio::fs::read_to_string(request.out_dir.join(RESULT)).await;
        match text {
            Ok(text) => {
                serde_json::from_str(&text).map_err(|e| CompileError::Protocol(e.to_string()))
            }
            Err(_) => Err(CompileError::WorkerCrashed {
                status: status.to_string(),
                stderr: tail,
            }),
        }
    }
}

struct Executed {
    success: bool,
    result: WorkerResult,
    diagnostics: Vec<Diagnostic>,
}

/// Run blocking filesystem work off the async threads.
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> T + Send + 'static,
) -> Result<T, CompileError> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| CompileError::Protocol(format!("blocking task failed: {e}")))
}

/// The only environment a worker gets. Secrets (API keys, tokens) stay out,
/// and `PATH` is empty so the engine cannot exec `biber` or anything else.
/// What remains is what Tectonic needs to find its cache and, on first use,
/// download its bundle.
fn worker_env() -> Vec<(String, String)> {
    const PASS: &[&str] = &[
        "HOME",
        "XDG_CACHE_HOME",
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "LATEX_AI_HOME",
        "TMPDIR",
        "LANG",
        "LC_ALL",
        "LD_LIBRARY_PATH",
        "SSL_CERT_FILE",
        "SSL_CERT_DIR",
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "NO_PROXY",
        "http_proxy",
        "https_proxy",
        "no_proxy",
    ];
    let mut env: Vec<(String, String)> = PASS
        .iter()
        .filter_map(|k| std::env::var(k).ok().map(|v| ((*k).to_owned(), v)))
        .collect();
    env.push(("PATH".to_owned(), String::new()));
    env
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_env_carries_no_secrets_and_an_empty_path() {
        let env = worker_env();
        assert!(env.contains(&("PATH".to_owned(), String::new())));
        for (key, _) in &env {
            let upper = key.to_ascii_uppercase();
            let secret = (upper.contains("KEY") && upper != "SSL_CERT_FILE")
                || upper.contains("TOKEN")
                || upper.contains("SECRET");
            assert!(!secret, "{key} must not reach the worker");
        }
    }
}
