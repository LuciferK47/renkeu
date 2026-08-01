//! Tectonic in-process LaTeX engine wrapper.
//!
//! Tectonic is not thread-safe, so a single global mutex serializes every
//! compile (masterplan.md §4.1). M1 surface: compile a LaTeX string to PDF
//! bytes, cancellable, with a blake3-keyed in-memory result cache.
#![forbid(unsafe_code)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod queue;
pub mod scratchpad;

use std::sync::{Arc, OnceLock};

use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

/// Errors from an in-process Tectonic compile.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CompileError {
    /// Tectonic engine rejected the document or failed internally.
    #[error("tectonic engine error: {0}")]
    Tectonic(String),
    /// The blocking compile task panicked or was dropped by the runtime.
    #[error("tectonic task join failed: {0}")]
    Join(String),
    /// The caller cancelled before the compile finished.
    #[error("compilation cancelled by client")]
    Cancelled,
    /// A newer debounced submission replaced this one (see
    /// [`queue::CompilationQueue::submit_debounced`]).
    #[error("compilation superseded by a newer submission")]
    Superseded,
    /// The concurrency semaphore was unexpectedly closed.
    #[error("compile queue semaphore closed")]
    SemaphoreClosed,
}

/// Outcome of a successful (or empty) compile.
#[derive(Debug, Clone)]
pub struct CompileResult {
    /// `true` when Tectonic produced non-empty PDF bytes.
    pub success: bool,
    /// PDF bytes, shared so multiple subscribers refcount instead of copy
    /// (rust.md I.1).
    pub pdf_bytes: Arc<[u8]>,
    /// blake3 hex of the source — also the cache key.
    pub content_hash: String,
}

/// Tectonic only allows one engine invocation at a time; this mutex enforces it
/// process-wide.
static TECTONIC_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();

/// In-process LaTeX compiler. Cheap to clone (`Arc` the cache).
#[derive(Clone)]
pub struct Engine {
    cache: moka::future::Cache<String, CompileResult>,
}

impl std::fmt::Debug for Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Engine")
            .field("cache_entries", &self.cache.entry_count())
            .finish()
    }
}

impl Default for Engine {
    fn default() -> Self {
        Self::new(64)
    }
}

impl Engine {
    /// Build an engine whose result cache holds up to `max_capacity` entries.
    #[must_use]
    pub fn new(max_capacity: u64) -> Self {
        Self {
            cache: moka::future::Cache::new(max_capacity),
        }
    }

    /// Compile `latex` to PDF bytes in-process.
    ///
    /// Identical source is served from cache. The active compile runs on the
    /// blocking pool and is raced against `cancel`; on cancel the future
    /// returns [`CompileError::Cancelled`] (the detached Tectonic thread is a
    /// backstop, per rust.md I.3).
    ///
    /// # Errors
    /// Returns [`CompileError`] on Tectonic failure, task-join failure, or
    /// cancellation.
    pub async fn compile(
        &self,
        latex: &str,
        cancel: &CancellationToken,
    ) -> Result<CompileResult, CompileError> {
        let hash = blake3::hash(latex.as_bytes()).to_hex().to_string();

        if let Some(cached) = self.cache.get(&hash).await {
            tracing::debug!(content_hash = %hash, "compile cache hit");
            return Ok(cached);
        }

        let owned = latex.to_owned();
        let guard = TECTONIC_MUTEX.get_or_init(|| Mutex::new(()));
        let _lock = guard.lock().await;

        let pdf_bytes: Vec<u8> = tokio::select! {
            joined = tokio::task::spawn_blocking(move || tectonic::latex_to_pdf(&owned)) => {
                joined
                    .map_err(|e| CompileError::Join(e.to_string()))?
                    .map_err(|e| CompileError::Tectonic(e.to_string()))?
            }
            () = cancel.cancelled() => return Err(CompileError::Cancelled),
        };

        let result = CompileResult {
            success: !pdf_bytes.is_empty(),
            pdf_bytes: Arc::from(pdf_bytes),
            content_hash: hash.clone(),
        };

        if result.success {
            self.cache.insert(hash, result.clone()).await;
        }

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn compiles_minimal_document_to_pdf() {
        let engine = Engine::default();
        let cancel = CancellationToken::new();
        let result = engine
            .compile(
                r"\documentclass{article}\begin{document}hello\end{document}",
                &cancel,
            )
            .await
            .expect("compile should succeed");
        assert!(result.success);
        assert!(!result.pdf_bytes.is_empty());
        assert_eq!(result.pdf_bytes.len(), result.pdf_bytes.as_ref().len());
    }

    #[tokio::test]
    async fn pre_cancelled_token_aborts() {
        let engine = Engine::default();
        let cancel = CancellationToken::new();
        cancel.cancel();
        let err = engine
            .compile(
                r"\documentclass{article}\begin{document}x\end{document}",
                &cancel,
            )
            .await
            .expect_err("pre-cancelled compile must error");
        assert!(matches!(err, CompileError::Cancelled));
    }
}
