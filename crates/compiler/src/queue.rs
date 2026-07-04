use std::sync::Arc;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;
use moka::future::Cache;
use std::time::Duration;
use crate::{CompileResult, CompileError, Engine};

/// Queue that deduplicates identical compile requests and ensures only one compile runs at a time.
#[derive(Clone)]
pub struct CompilationQueue {
    engine: Arc<Engine>,
    semaphore: Arc<Semaphore>,
    // Deduplication cache to prevent identical bursts.
    // Key: hash of the file content.
    cache: Cache<String, CompileResult>,
}

impl CompilationQueue {
    pub fn new(engine: Arc<Engine>) -> Self {
        Self {
            engine,
            semaphore: Arc::new(Semaphore::new(1)),
            cache: Cache::builder()
                .time_to_live(Duration::from_secs(60))
                .max_capacity(128)
                .build(),
        }
    }

    /// Submit a compile job. Deduplicates identical concurrent jobs.
    pub async fn submit(
        &self,
        latex: &str,
        cancel: &CancellationToken,
    ) -> Result<CompileResult, CompileError> {
        let hash = blake3::hash(latex.as_bytes()).to_hex().to_string();

        if let Some(cached) = self.cache.get(&hash).await {
            return Ok(cached);
        }

        // Wait for turn
        let _permit = tokio::select! {
            res = self.semaphore.acquire() => res.map_err(|_| CompileError::SemaphoreClosed)?,
            _ = cancel.cancelled() => return Err(CompileError::Cancelled),
        };

        // Check cache again after acquiring lock (another task might have compiled it while we waited)
        if let Some(cached) = self.cache.get(&hash).await {
            return Ok(cached);
        }

        // Compile
        let result = self.engine.compile(latex, cancel).await?;
        
        if result.success {
            self.cache.insert(hash, result.clone()).await;
        }
        
        Ok(result)
    }
}
