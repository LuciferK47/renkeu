//! Compilation queue (masterplan.md §12 M2): deduplicates identical requests,
//! debounces rapid-fire submissions, and admits one compile at a time — the
//! `Semaphore(1)` mirrors Tectonic's process-global mutex.

use crate::{CompileError, CompileResult, Engine};
use moka::future::Cache;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

/// How long recent results are replayed for identical source (dedup window).
const DEDUP_TTL: Duration = Duration::from_secs(60);

#[derive(Clone)]
pub struct CompilationQueue {
    engine: Arc<Engine>,
    semaphore: Arc<Semaphore>,
    /// Replays results for identical source within [`DEDUP_TTL`]; key is the
    /// blake3 content hash.
    cache: Cache<String, CompileResult>,
    /// Monotonic submission counter backing [`Self::submit_debounced`]: a
    /// sleeper only proceeds if no newer submission arrived meanwhile.
    generation: Arc<AtomicU64>,
}

impl CompilationQueue {
    #[must_use]
    pub fn new(engine: Arc<Engine>) -> Self {
        Self {
            engine,
            semaphore: Arc::new(Semaphore::new(1)),
            cache: Cache::builder()
                .time_to_live(DEDUP_TTL)
                .max_capacity(128)
                .build(),
            generation: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Compile `latex`, deduplicating identical concurrent or recent jobs.
    ///
    /// # Errors
    /// [`CompileError::Cancelled`] if `cancel` fires while queued or
    /// compiling; engine errors otherwise.
    pub async fn submit(
        &self,
        latex: &str,
        cancel: &CancellationToken,
    ) -> Result<CompileResult, CompileError> {
        let hash = blake3::hash(latex.as_bytes()).to_hex().to_string();

        if let Some(cached) = self.cache.get(&hash).await {
            return Ok(cached);
        }

        let _permit = tokio::select! {
            res = self.semaphore.acquire() => res.map_err(|_| CompileError::SemaphoreClosed)?,
            () = cancel.cancelled() => return Err(CompileError::Cancelled),
        };

        // Re-check after winning the permit: the previous holder may have
        // compiled exactly this source while we waited.
        if let Some(cached) = self.cache.get(&hash).await {
            return Ok(cached);
        }

        let result = self.engine.compile(latex, cancel).await?;

        if result.success {
            self.cache.insert(hash, result.clone()).await;
        }

        Ok(result)
    }

    /// [`Self::submit`] that also produces the `SyncTeX` source map (M3).
    ///
    /// `tex_name` participates in the dedup key because the artefact names
    /// derive from it.
    ///
    /// # Errors
    /// As [`Self::submit`].
    pub async fn submit_with_synctex(
        &self,
        latex: &str,
        tex_name: &str,
        cancel: &CancellationToken,
    ) -> Result<CompileResult, CompileError> {
        let hash = blake3::hash(latex.as_bytes()).to_hex().to_string();
        let key = format!("{hash}:synctex:{tex_name}");

        if let Some(cached) = self.cache.get(&key).await {
            return Ok(cached);
        }

        let _permit = tokio::select! {
            res = self.semaphore.acquire() => res.map_err(|_| CompileError::SemaphoreClosed)?,
            () = cancel.cancelled() => return Err(CompileError::Cancelled),
        };

        if let Some(cached) = self.cache.get(&key).await {
            return Ok(cached);
        }

        let result = self
            .engine
            .compile_with_synctex(latex, tex_name, cancel)
            .await?;

        if result.success {
            self.cache.insert(key, result.clone()).await;
        }

        Ok(result)
    }

    /// Debounced [`Self::submit`] for auto-compile-on-keystroke callers: waits
    /// `debounce`, and if a newer debounced submission arrived meanwhile,
    /// yields [`CompileError::Superseded`] instead of compiling stale source.
    ///
    /// # Errors
    /// [`CompileError::Superseded`] when a newer submission wins;
    /// [`CompileError::Cancelled`] if cancelled during the debounce window;
    /// otherwise as [`Self::submit`].
    pub async fn submit_debounced(
        &self,
        latex: &str,
        debounce: Duration,
        cancel: &CancellationToken,
    ) -> Result<CompileResult, CompileError> {
        let my_generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;

        tokio::select! {
            () = tokio::time::sleep(debounce) => {}
            () = cancel.cancelled() => return Err(CompileError::Cancelled),
        }

        if self.generation.load(Ordering::SeqCst) != my_generation {
            return Err(CompileError::Superseded);
        }

        self.submit(latex, cancel).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC_A: &str = r"\documentclass{article}\begin{document}queue a\end{document}";
    const DOC_B: &str = r"\documentclass{article}\begin{document}queue b\end{document}";

    #[tokio::test]
    async fn identical_submissions_hit_the_dedup_cache() {
        let queue = CompilationQueue::new(Arc::new(Engine::default()));
        let cancel = CancellationToken::new();

        let first = queue.submit(DOC_A, &cancel).await.expect("first compile");
        let second = queue.submit(DOC_A, &cancel).await.expect("cached compile");
        assert!(first.success && second.success);
        assert_eq!(first.content_hash, second.content_hash);
    }

    #[tokio::test]
    async fn pre_cancelled_submission_never_compiles() {
        let queue = CompilationQueue::new(Arc::new(Engine::default()));
        let cancel = CancellationToken::new();
        cancel.cancel();
        let err = queue.submit(DOC_B, &cancel).await.expect_err("must cancel");
        assert!(matches!(err, CompileError::Cancelled));
    }

    #[tokio::test]
    async fn older_debounced_submission_is_superseded_by_newer() {
        let queue = CompilationQueue::new(Arc::new(Engine::default()));
        let cancel = CancellationToken::new();

        let slow = {
            let queue = queue.clone();
            let cancel = cancel.clone();
            tokio::spawn(async move {
                queue
                    .submit_debounced(DOC_A, Duration::from_millis(80), &cancel)
                    .await
            })
        };
        // Give the first submission time to claim its generation slot.
        tokio::time::sleep(Duration::from_millis(20)).await;
        let fast = {
            let queue = queue.clone();
            let cancel = cancel.clone();
            tokio::spawn(async move {
                queue
                    .submit_debounced(DOC_A, Duration::from_millis(30), &cancel)
                    .await
            })
        };

        let older = slow.await.expect("join");
        assert!(
            matches!(older, Err(CompileError::Superseded)),
            "older submission should be superseded, got: {older:?}"
        );
        // The newer one compiles for real (slow: first Tectonic run in tests).
        let newer = fast
            .await
            .expect("join")
            .expect("newer submission compiles");
        assert!(newer.success);
    }
}
