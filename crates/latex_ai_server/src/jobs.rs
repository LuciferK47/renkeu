//! In-memory compile-job registry: `POST /api/compile` creates a job, the
//! job's progress is broadcast on a `watch` channel, and
//! `GET /api/compile/stream/{id}` replays it as SSE. Persistence into the
//! `compile_queue` table is M5 scope (masterplan.md §12 M5) — this registry is
//! deliberately process-local.

use latex_ai_compiler::diagnostics::Diagnostic;
use serde::Serialize;
use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

/// Cap on remembered jobs; oldest are evicted (finished jobs stay queryable
/// until then so late subscribers still see the terminal state).
const MAX_JOBS: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum JobPhase {
    Queued,
    Compiling,
    Done,
    Failed,
}

impl JobPhase {
    #[must_use]
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Done | Self::Failed)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct JobState {
    pub phase: JobPhase,
    pub message: String,
    /// 0.0..=1.0 for the progress bar.
    pub progress: f32,
    pub page_count: Option<u16>,
    pub error_summary: Option<String>,
    /// The stored build this job produced (or tried to), once there is one.
    pub build_id: Option<String>,
    /// The file that was compiled, project-relative.
    pub main_file: Option<String>,
    pub diagnostics: Vec<Diagnostic>,
}

impl JobState {
    #[must_use]
    pub fn queued() -> Self {
        Self {
            phase: JobPhase::Queued,
            message: "queued".to_owned(),
            progress: 0.0,
            page_count: None,
            error_summary: None,
            build_id: None,
            main_file: None,
            diagnostics: Vec::new(),
        }
    }
}

#[derive(Default)]
pub struct JobRegistry {
    inner: Mutex<Registry>,
}

#[derive(Default)]
struct Registry {
    jobs: HashMap<String, (watch::Receiver<JobState>, CancellationToken)>,
    order: VecDeque<String>,
}

impl JobRegistry {
    /// Register a new job; the returned sender drives its state. The mutex is
    /// held only for map bookkeeping (no await, no user code); a poisoned
    /// lock is recovered with `into_inner` since the map stays consistent.
    pub fn create(&self, job_id: &str) -> (watch::Sender<JobState>, CancellationToken) {
        let (tx, rx) = watch::channel(JobState::queued());
        let cancel = CancellationToken::new();
        let mut inner = match self.inner.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        if inner.order.len() >= MAX_JOBS {
            // Evict the oldest FINISHED job. Evicting a running one made its
            // stream endpoint 404 mid-compile; better to exceed the cap
            // briefly than to lose a live job.
            let evictable = inner.order.iter().position(|id| {
                inner
                    .jobs
                    .get(id)
                    .is_none_or(|(rx, _)| rx.borrow().phase.is_terminal())
            });
            if let Some(index) = evictable {
                if let Some(oldest) = inner.order.remove(index) {
                    inner.jobs.remove(&oldest);
                }
            }
        }
        inner.jobs.insert(job_id.to_owned(), (rx, cancel.clone()));
        inner.order.push_back(job_id.to_owned());
        (tx, cancel)
    }

    /// Ask a job to stop. Returns `false` if the job is unknown. Cancelling a
    /// job that already finished is harmless and still returns `true`.
    #[must_use]
    pub fn cancel(&self, job_id: &str) -> bool {
        let inner = match self.inner.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        match inner.jobs.get(job_id) {
            Some((_, token)) => {
                token.cancel();
                true
            }
            None => false,
        }
    }

    /// Watch an existing job, if it is still remembered.
    #[must_use]
    pub fn subscribe(&self, job_id: &str) -> Option<watch::Receiver<JobState>> {
        let inner = match self.inner.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        inner.jobs.get(job_id).map(|(rx, _)| rx.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subscribe_sees_latest_state_and_unknown_ids_are_none() {
        let registry = JobRegistry::default();
        let (tx, _) = registry.create("job-1");
        tx.send_replace(JobState {
            phase: JobPhase::Done,
            message: "ok".into(),
            progress: 1.0,
            page_count: Some(2),
            ..JobState::queued()
        });

        let rx = registry.subscribe("job-1").expect("job exists");
        assert_eq!(rx.borrow().phase, JobPhase::Done);
        assert!(registry.subscribe("nope").is_none());
    }

    #[test]
    fn eviction_never_drops_a_running_job() {
        let registry = JobRegistry::default();
        let (running, _) = registry.create("running");
        let mut finished = Vec::new();
        for i in 0..MAX_JOBS {
            let (tx, _) = registry.create(&format!("done-{i}"));
            tx.send_replace(JobState {
                phase: JobPhase::Done,
                ..JobState::queued()
            });
            finished.push(tx);
        }
        assert!(
            registry.subscribe("running").is_some(),
            "a running job was evicted"
        );
        assert!(
            registry.subscribe("done-0").is_none(),
            "oldest finished evicted"
        );
        drop(running);
    }

    #[test]
    fn eviction_keeps_at_most_max_jobs() {
        let registry = JobRegistry::default();
        let mut senders = Vec::new();
        for i in 0..(MAX_JOBS + 5) {
            let (tx, _) = registry.create(&format!("job-{i}"));
            tx.send_replace(JobState {
                phase: JobPhase::Done,
                ..JobState::queued()
            });
            senders.push(tx);
        }
        assert!(registry.subscribe("job-0").is_none(), "oldest evicted");
        assert!(
            registry
                .subscribe(&format!("job-{}", MAX_JOBS + 4))
                .is_some(),
            "newest kept"
        );
    }

    #[test]
    fn cancel_trips_the_jobs_token_and_unknown_ids_are_reported() {
        let registry = JobRegistry::default();
        let (_tx, token) = registry.create("j");
        assert!(!token.is_cancelled());
        assert!(registry.cancel("j"));
        assert!(token.is_cancelled());
        assert!(!registry.cancel("nope"));
    }
}
