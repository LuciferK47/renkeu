//! Process-lifetime counters (masterplan.md §4.5, §12 M5).
//!
//! Deliberately atomics rather than a metrics framework: this is a
//! single-process, single-user daemon, and the counters exist to answer
//! "is it doing anything?" — a registry, exporters and label cardinality
//! would be complexity nobody asked for (CONTEXT.md §5).
//!
//! Counters are monotonic and reset only on restart.

use std::fmt::Write as _;
use std::sync::atomic::{AtomicU64, Ordering};

/// All counters. One instance lives in the server's shared state.
#[derive(Debug, Default)]
pub struct Metrics {
    requests_total: AtomicU64,
    responses_failed: AtomicU64,
    compiles_started: AtomicU64,
    compiles_succeeded: AtomicU64,
    compiles_failed: AtomicU64,
    agent_runs: AtomicU64,
    agent_iterations: AtomicU64,
    llm_calls: AtomicU64,
    edits_applied: AtomicU64,
}

/// A point-in-time copy, for serialising.
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct MetricsSnapshot {
    pub requests_total: u64,
    pub responses_failed: u64,
    pub compiles_started: u64,
    pub compiles_succeeded: u64,
    pub compiles_failed: u64,
    pub agent_runs: u64,
    pub agent_iterations: u64,
    pub llm_calls: u64,
    pub edits_applied: u64,
}

/// `Relaxed` is right here: counters are independent, never used to order
/// other memory, and only ever read for display.
const ORDER: Ordering = Ordering::Relaxed;

impl Metrics {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// One per routed HTTP request.
    pub fn record_request(&self) {
        self.requests_total.fetch_add(1, ORDER);
    }

    /// A response that carried a 4xx/5xx status.
    pub fn record_failed_response(&self) {
        self.responses_failed.fetch_add(1, ORDER);
    }

    pub fn record_compile_started(&self) {
        self.compiles_started.fetch_add(1, ORDER);
    }

    pub fn record_compile_finished(&self, success: bool) {
        if success {
            self.compiles_succeeded.fetch_add(1, ORDER);
        } else {
            self.compiles_failed.fetch_add(1, ORDER);
        }
    }

    /// One per agent-loop stream started.
    pub fn record_agent_run(&self) {
        self.agent_runs.fetch_add(1, ORDER);
    }

    /// One per iteration within a loop.
    pub fn record_agent_iteration(&self) {
        self.agent_iterations.fetch_add(1, ORDER);
    }

    /// Counted BEFORE the request is issued, so an in-flight or failed call
    /// is still visible (masterplan §12 M5 audit: integration).
    pub fn record_llm_call(&self) {
        self.llm_calls.fetch_add(1, ORDER);
    }

    /// Number of hunks written by apply-selected.
    pub fn record_edits_applied(&self, count: u64) {
        self.edits_applied.fetch_add(count, ORDER);
    }

    #[must_use]
    pub fn snapshot(&self) -> MetricsSnapshot {
        MetricsSnapshot {
            requests_total: self.requests_total.load(ORDER),
            responses_failed: self.responses_failed.load(ORDER),
            compiles_started: self.compiles_started.load(ORDER),
            compiles_succeeded: self.compiles_succeeded.load(ORDER),
            compiles_failed: self.compiles_failed.load(ORDER),
            agent_runs: self.agent_runs.load(ORDER),
            agent_iterations: self.agent_iterations.load(ORDER),
            llm_calls: self.llm_calls.load(ORDER),
            edits_applied: self.edits_applied.load(ORDER),
        }
    }
}

impl MetricsSnapshot {
    /// Prometheus text exposition format, so `curl` or a scraper both work.
    #[must_use]
    pub fn to_prometheus(&self) -> String {
        let rows: [(&str, &str, u64); 9] = [
            (
                "latex_ai_requests_total",
                "HTTP requests routed",
                self.requests_total,
            ),
            (
                "latex_ai_responses_failed_total",
                "Responses with a 4xx or 5xx status",
                self.responses_failed,
            ),
            (
                "latex_ai_compiles_started_total",
                "Compile jobs started",
                self.compiles_started,
            ),
            (
                "latex_ai_compiles_succeeded_total",
                "Compile jobs that produced a PDF",
                self.compiles_succeeded,
            ),
            (
                "latex_ai_compiles_failed_total",
                "Compile jobs that did not produce a PDF",
                self.compiles_failed,
            ),
            (
                "latex_ai_agent_runs_total",
                "Agent loops started",
                self.agent_runs,
            ),
            (
                "latex_ai_agent_iterations_total",
                "Agent loop iterations across all runs",
                self.agent_iterations,
            ),
            (
                "latex_ai_llm_calls_total",
                "Requests issued to the LLM provider",
                self.llm_calls,
            ),
            (
                "latex_ai_edits_applied_total",
                "Hunks written to workspace files",
                self.edits_applied,
            ),
        ];

        let mut out = String::with_capacity(rows.len() * 96);
        for (name, help, value) in rows {
            // `write!` to a String is infallible; the Result is only there to
            // satisfy the trait.
            let _ = writeln!(out, "# HELP {name} {help}");
            let _ = writeln!(out, "# TYPE {name} counter");
            let _ = writeln!(out, "{name} {value}");
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counters_start_at_zero_and_accumulate() {
        let m = Metrics::new();
        assert_eq!(m.snapshot().requests_total, 0);

        m.record_request();
        m.record_request();
        m.record_failed_response();
        m.record_compile_started();
        m.record_compile_finished(true);
        m.record_compile_finished(false);
        m.record_agent_run();
        m.record_agent_iteration();
        m.record_agent_iteration();
        m.record_llm_call();
        m.record_edits_applied(3);

        let s = m.snapshot();
        assert_eq!(s.requests_total, 2);
        assert_eq!(s.responses_failed, 1);
        assert_eq!(s.compiles_started, 1);
        assert_eq!(s.compiles_succeeded, 1);
        assert_eq!(s.compiles_failed, 1);
        assert_eq!(s.agent_runs, 1);
        assert_eq!(s.agent_iterations, 2);
        assert_eq!(s.llm_calls, 1);
        assert_eq!(s.edits_applied, 3);
    }

    #[test]
    fn counting_is_safe_from_many_threads() {
        let m = std::sync::Arc::new(Metrics::new());
        let mut handles = Vec::new();
        for _ in 0..8 {
            let m = m.clone();
            handles.push(std::thread::spawn(move || {
                for _ in 0..1000 {
                    m.record_request();
                }
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(m.snapshot().requests_total, 8000);
    }

    #[test]
    fn prometheus_output_is_well_formed() {
        let m = Metrics::new();
        m.record_request();
        let text = m.snapshot().to_prometheus();

        assert!(text.contains("# TYPE latex_ai_requests_total counter"));
        assert!(text.contains("\nlatex_ai_requests_total 1\n"));
        // Every metric needs HELP and TYPE before its sample.
        let helps = text.matches("# HELP ").count();
        let types = text.matches("# TYPE ").count();
        assert_eq!(helps, types);
        assert_eq!(helps, 9, "one HELP per counter");
        assert!(text.ends_with('\n'));
    }
}
