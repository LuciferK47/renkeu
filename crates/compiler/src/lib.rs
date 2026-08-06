//! Tectonic in-process LaTeX engine wrapper.
//!
//! Tectonic is not thread-safe, so a single global mutex serializes every
//! compile (masterplan.md §4.1). Compiles a LaTeX string to PDF bytes —
//! optionally with the `SyncTeX` source map (M3) — cancellable, with a
//! blake3-keyed in-memory result cache.
#![forbid(unsafe_code)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

mod engine;
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
    /// gzipped `SyncTeX` map, present only when the compile requested it
    /// ([`Engine::compile_with_synctex`]).
    pub synctex_gz: Option<Arc<[u8]>>,
    /// blake3 hex of the source — also the cache key.
    pub content_hash: String,
}

/// Default name given to the primary input. It determines the artefact names
/// (`<stem>.pdf`, `<stem>.synctex.gz`). Note that `SyncTeX` itself records the
/// primary input as `texput` regardless, so reverse search maps the primary
/// tag onto the document the caller compiled.
pub const DEFAULT_TEX_NAME: &str = "texput.tex";

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

    /// Compile `latex` to PDF bytes in-process (no `SyncTeX`).
    ///
    /// # Errors
    /// Returns [`CompileError`] on Tectonic failure, task-join failure, or
    /// cancellation.
    pub async fn compile(
        &self,
        latex: &str,
        cancel: &CancellationToken,
    ) -> Result<CompileResult, CompileError> {
        self.run(latex, DEFAULT_TEX_NAME, false, cancel).await
    }

    /// Compile `latex` and also return the `SyncTeX` source map (M3).
    ///
    /// `tex_name` should be the document's real basename: `SyncTeX` records it
    /// as the source file name, and reverse search maps hits back through it.
    ///
    /// # Errors
    /// As [`Engine::compile`].
    pub async fn compile_with_synctex(
        &self,
        latex: &str,
        tex_name: &str,
        cancel: &CancellationToken,
    ) -> Result<CompileResult, CompileError> {
        self.run(latex, tex_name, true, cancel).await
    }

    /// Shared body. Identical source *and* `SyncTeX`-ness is served from cache;
    /// the key includes both because a cached PDF-only result cannot satisfy a
    /// caller that needs the source map.
    ///
    /// The active compile runs on the blocking pool and is raced against
    /// `cancel`; on cancel the future returns [`CompileError::Cancelled`] (the
    /// detached Tectonic thread is a backstop, per rust.md I.3).
    async fn run(
        &self,
        latex: &str,
        tex_name: &str,
        want_synctex: bool,
        cancel: &CancellationToken,
    ) -> Result<CompileResult, CompileError> {
        let content_hash = blake3::hash(latex.as_bytes()).to_hex().to_string();
        let cache_key = if want_synctex {
            format!("{content_hash}:synctex:{tex_name}")
        } else {
            content_hash.clone()
        };

        if let Some(cached) = self.cache.get(&cache_key).await {
            tracing::debug!(content_hash = %content_hash, "compile cache hit");
            return Ok(cached);
        }

        let owned = latex.to_owned();
        let owned_name = tex_name.to_owned();
        let guard = TECTONIC_MUTEX.get_or_init(|| Mutex::new(()));
        let _lock = guard.lock().await;

        let raw = tokio::select! {
            joined = tokio::task::spawn_blocking(move || {
                engine::run(&owned, &owned_name, want_synctex)
            }) => {
                joined.map_err(|e| CompileError::Join(e.to_string()))??
            }
            () = cancel.cancelled() => return Err(CompileError::Cancelled),
        };

        let result = CompileResult {
            success: !raw.pdf.is_empty(),
            pdf_bytes: Arc::from(raw.pdf),
            synctex_gz: raw.synctex_gz.map(Arc::from),
            content_hash,
        };

        if result.success {
            self.cache.insert(cache_key, result.clone()).await;
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
    async fn emits_a_parseable_synctex_map() {
        use std::io::Read as _;

        let engine = Engine::default();
        let result = engine
            .compile_with_synctex(
                "\\documentclass{article}\n\\begin{document}\nsynctex probe\n\\end{document}\n",
                "probe.tex",
                &CancellationToken::new(),
            )
            .await
            .expect("compile should succeed");

        assert!(result.success);
        let gz = result
            .synctex_gz
            .expect("synctex(true) must yield a source map");
        assert!(!gz.is_empty());

        let mut text = String::new();
        flate2::read::GzDecoder::new(gz.as_ref())
            .read_to_string(&mut text)
            .expect("synctex output should be gzip");
        assert!(
            text.starts_with("SyncTeX Version:"),
            "unexpected synctex header: {:?}",
            &text[..text.len().min(60)]
        );
        // Tectonic names the primary input "texput" internally regardless of
        // `tex_input_name`, so the map records that rather than the caller's
        // basename. Single-document compiles map tag 1 to the compiled file.
        assert!(
            text.contains("Input:1:texput"),
            "expected the primary input record"
        );
        assert!(text.contains("Magnification:"), "expected the preamble");
    }

    #[tokio::test]
    async fn plain_compile_requests_no_synctex() {
        let engine = Engine::default();
        let result = engine
            .compile(
                r"\documentclass{article}\begin{document}x\end{document}",
                &CancellationToken::new(),
            )
            .await
            .expect("compile should succeed");
        assert!(result.synctex_gz.is_none());
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
