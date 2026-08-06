//! In-memory compile-job registry: `POST /api/compile` creates a job, the
//! job's progress is broadcast on a `watch` channel, and
//! `GET /api/compile/stream/{id}` replays it as SSE. Persistence into the
//! `compile_queue` table is M5 scope (masterplan.md §12 M5) — this registry is
//! deliberately process-local.

use serde::Serialize;
use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use tokio::sync::watch;

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
        }
    }
}

#[derive(Default)]
pub struct JobRegistry {
    inner: Mutex<Registry>,
}

#[derive(Default)]
struct Registry {
    jobs: HashMap<String, watch::Receiver<JobState>>,
    order: VecDeque<String>,
}

impl JobRegistry {
    /// Register a new job; the returned sender drives its state. The mutex is
    /// held only for map bookkeeping (no await, no user code); a poisoned
    /// lock is recovered with `into_inner` since the map stays consistent.
    pub fn create(&self, job_id: &str) -> watch::Sender<JobState> {
        let (tx, rx) = watch::channel(JobState::queued());
        let mut inner = match self.inner.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        if inner.order.len() >= MAX_JOBS {
            if let Some(oldest) = inner.order.pop_front() {
                inner.jobs.remove(&oldest);
            }
        }
        inner.jobs.insert(job_id.to_owned(), rx);
        inner.order.push_back(job_id.to_owned());
        tx
    }

    /// Watch an existing job, if it is still remembered.
    #[must_use]
    pub fn subscribe(&self, job_id: &str) -> Option<watch::Receiver<JobState>> {
        let inner = match self.inner.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        inner.jobs.get(job_id).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subscribe_sees_latest_state_and_unknown_ids_are_none() {
        let registry = JobRegistry::default();
        let tx = registry.create("job-1");
        tx.send_replace(JobState {
            phase: JobPhase::Done,
            message: "ok".into(),
            progress: 1.0,
            page_count: Some(2),
            error_summary: None,
        });

        let rx = registry.subscribe("job-1").expect("job exists");
        assert_eq!(rx.borrow().phase, JobPhase::Done);
        assert!(registry.subscribe("nope").is_none());
    }

    #[test]
    fn eviction_keeps_at_most_max_jobs() {
        let registry = JobRegistry::default();
        for i in 0..(MAX_JOBS + 5) {
            let _tx = registry.create(&format!("job-{i}"));
        }
        assert!(registry.subscribe("job-0").is_none(), "oldest evicted");
        assert!(
            registry
                .subscribe(&format!("job-{}", MAX_JOBS + 4))
                .is_some(),
            "newest kept"
        );
    }
}
