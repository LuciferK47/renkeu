# rust.md — Rust Coding Guidelines + Codebase Port Spec

> **Status: BINDING**. Part I is permanent (re-read every session like `CONTEXT.md`). Part II is disposable once M5 lands.
> **Audience**: AI coding agents and humans editing the LaTeX-AI Rust codebase. AI assistants in particular are bad at idiomatic Rust; treat everything in Part I as a hard rule, not a suggestion.
> **Companion docs**: `CONTEXT.md` (four pillars — the law), `idea.md` (pitch + library lock), `masterplan.md` (engineering execution).

---

# Part I — Rust Coding Guidelines (BINDING)

This part is the Rust-native equivalent of the four pillars in `CONTEXT.md`. Every commit must satisfy every rule.

## I.1 Ownership, Borrowing, Lifetimes

### Borrow checker as a friend, not an enemy

When the borrow checker blocks a write, fix the *architecture* before sprinkling `.clone()`. A `.clone()` is sometimes correct (immutable share) but is usually a sign you mis-stated data ownership. Audit each `.clone()` in a PR with: "Did I really need another copy here, or did I just dodge a lifetime annotation?"

**Permitted `.clone()`**: cheap (mostly `Arc::clone`, `String` reads, large reads via `Cow`).
**Forbidden `.clone()`**: whole `Vec<u8>` PDF bytes, whole `Vec<AgentEdit>` across thread boundaries — thread those via `Arc<[u8]>` and `Arc<Vec<AgentEdit>>` instead (refcount bump = no copy).

### Lifetime hygiene

- If you write a `'a` annotation, label *why* (typically: "tied to the buffer in `AppState` for the lifetime of the request").
- Prefer `Cow<'_, str>` over `&'a str` where you might mutate.
- Public API functions must not require the caller to pass values outliving the call (no `fn print(&'static str)` boundaries). Exceptions: literals for `serde` tags or const lookups.
- `&'static str` is fine for `match` arms and static config keys; **never** for paths coming from user input (`PathBuf` is correct there).

### `Arc<[T]>` over `Vec<T>` for shared hot buffers

When a function returns buffers consumed by multiple subscribers (PDF bytes from Tectonic, AI-edit lists, PNG crop), return `Arc<[T]>`, not `Vec<T>`:

```rust
// good — refcount bump on .clone()
pub fn render_crop(/*...*/) -> Result<Arc<[u8]>, VisualError>;

// bad — every subscriber copies the whole thing
pub fn render_crop(/*...*/) -> Result<Vec<u8>, VisualError>;
```

This rule exists because the agent-healed loop calls `render_crop` and then feeds the PNG bytes simultaneously to the VLM and to disk write.

## I.2 Error Handling — `thiserror` for libraries, `anyhow` for app

Pattern (2026 production consensus; enforce):

```rust
// crates/compiler/src/lib.rs (a library)
#[derive(thiserror::Error, Debug)]
pub enum CompileError {
    #[error("tectonic engine error: {0}")]
    Tectonic(#[from] tectonic::Error),
    #[error("scratchpad write failed: {0}")]
    Scratchpad(#[source] std::io::Error),
    #[error("compilation cancelled by client")]
    Cancelled,
    #[error("compilation exceeded {timeout_ms}ms budget")]
    Timeout { timeout_ms: u64 },
}

pub type Result<T, E = CompileError> = std::result::Result<T, E>;
```

```rust
// crates/latex_ai_server/src/main.rs (application)
use anyhow::{Context, Result};

#[tokio::main]
async fn main() -> Result<()> {
    let cfg = config::load().await.context("loading config from ~/.latex-ai/config.toml")?;
    let state = init_state(&cfg).await.context("initialising AppState")?;
    serve(state).await.context("serving axum")?;
    Ok(())
}
```

### `.context()` at every trust boundary

Every `?` that crosses an I/O, network, or sub-process boundary takes `.context()`/`.with_context()`:

```rust
let latex = tokio::fs::read_to_string(&scratchpad.main_file)
    .await
    .with_context(|| format!("reading scratch main: {}", scratchpad.main_file.display()))?;
```

Why: traceback at 3am is useless if it just says "No such file or directory" with no path. `.context()` is the difference between "fix obvious" and "reproduce from logs."

### `#[must_use]` on every public `Result`

```rust
#[must_use = "this Result should be handled, never ignored"]
pub fn validate_path(/*...*/) -> Result<PathBuf, SecurityError>;
```

The compiler will warn on every ignored return; treat all such warnings as errors via `#![deny(unused_must_use)]` at every crate root.

### Forbidden in non-test code

- `.unwrap()` — add `#![deny(clippy::unwrap_used)]` at every crate root.
- `.expect()` — same. The only allowed `expect` is in `fn main()` for startup-critical initialisation, and even then, write `#[track_caller]`-safe messages that contain full context.
- `.clone()` on `Vec<u8>` of size >1KB; use `Arc<[u8]>`.
- `panic!()` in library code. In app code only allowed at startup (`main()`).
- `Box<dyn Error>` as a return type. Always specify a concrete error or use `anyhow::Error`.
- Silent swallowing:
  ```rust
  let _ = something_that_might_fail().await; // ❌
  if let Err(e) = something_that_might_fail().await {
      tracing::warn!(error = ?e, "non-fatal thing failed");
  } // ✅
  ```

## I.3 Async Cancellation Discipline

### `CancellationToken` is the only mechanism

We use `tokio_util::sync::CancellationToken`. SSE disconnect cancels the token. Every spawned long task takes a `CancellationToken` parameter and `tokio::select!`s against it:

```rust
async fn long_op(cancel: CancellationToken) -> Result<()> {
    loop {
        tokio::select! {
            _ = cancel.cancelled() => return Err(CompileError::Cancelled),
            r = do_one_step() => r?,
        }
    }
}
```

### Drop cleanup is forbidden as the primary cancel path

Neither `Drop` nor `tokio::task::JoinHandle::abort()` is the primary cancel mechanism; they're *backstops*. The primary mechanism is the explicit `CancellationToken::cancelled()` check inside the long-running future. The backstop is for "client crashed without sending FIN" cases.

### Tectonic runs in a worker process, not a task

**Amended (M10, 2026-10).** A blocking Tectonic call cannot be interrupted, and it holds a process-wide lock for the whole compile, so `spawn_blocking` plus `select!` only abandoned the *wait*: the engine, and every later compile, stayed stuck behind it. Compiles therefore run in a short-lived child process (this same binary, re-executed as `__compile-worker`). The parent selects over three things and kills and reaps the child on the first two:

```rust
tokio::select! {
    _ = cancel.cancelled() => { child.start_kill()?; child.wait().await?; Err(CompileError::Cancelled) }
    _ = tokio::time::sleep(timeout) => { child.start_kill()?; child.wait().await?; Err(CompileError::Timeout) }
    status = child.wait() => finish(status),
}
```

`kill_on_drop` is only the backstop. Tectonic's cache writes are write-then-rename, so killing a worker cannot corrupt the bundle cache. There is no `TECTONIC_MUTEX` and no compile queue: two compiles may run at once, one process each.

## I.4 Concurrency Primitive Decision Tree

Force yourself through this tree BEFORE choosing a primitive; explain in the PR description.

```
1. Is the data passed between tasks一口only one task writes at a time?
   → tokio::sync::Mutex<T>  (Mutex — fine for serialised access)

2. Do many tasks read but rarely write?
   → Arc<RwLock<T>>  (RwLock — check reader predication; if all reads are <10us, just use Mutex)

3. Is the value read hundreds of times and updated rarely (config-like)?
   → arc_swap::ArcSwap<T>  (lock-free reads, atomic writes via Arc::clone)

4. Is this a "queue between producers and consumer" with one producer only?
   → tokio::sync::oneshot::channel<T>

5. Is this a "queue between producers and consumer" with multiple producers?
   → tokio::sync::mpsc::channel<T> with bounded capacity matching burst tolerance

6. Is the data only ever mutated by one task?
   → own the data inside the task; no Arc at all.

7. Are reads on a map shared between high-frequency callers and updates rare?
   → moka::future::Cache<K, V>  (lock-free-ish reads via TinyLFU)

8. Does anyone need to BLOCK on insert (backpressure)?
   → bounded mpsc; if backpressure is the point, do not use moka.
```

Forbidden: `std::sync::Mutex` in async contexts (it blocks the runtime thread). `std::sync::RwLock` same.

## I.5 `cargo` Discipline (CI gates)

Every PR must pass:

```sh
cargo fmt --check                                  # formatting
cargo clippy --workspace --all-targets -- \
    -D warnings -D clippy::pedantic \
    -D clippy::unwrap_used -D clippy::expect_used \
    -D clippy::dbg_macro -D clippy::todo \
    -D clippy::print_stdout -D clippy::print_stderr
cargo nextest run --workspace                      # tests
cargo tarpaulin --workspace --fail-under-80         # coverage gate
cargo deny check                                   # advisories + licenses + bans
cargo audit                                        # RUSTSEC advisories
cargo machete --workspace                           # unused-dep detector
cargo sqlx prepare --check --workspace              # query! macro compiles vs offline data
```

### `cargo deny` policies (`.cargo-deny.toml`)

- `[advisories]` vendored = false; daily-checked via dependabot for crates.
- `[licenses] allow = [MIT, Apache-2.0, BSD-3-Clause, MPL-2.0, Zlib]`. Other licenses flagged for review.
- `[bans]` triple-licenses (dual-license) are fine; `GPL` and `AGPL` are rejected unless we license compatibly — we are MIT OR Apache-2.0.

### Workspace single lockfile

One `Cargo.lock` at the workspace root, shared across all members. Never check out a sub-crate standalone — the workspace closure is the unit of release.

## I.6 `unsafe`

- No `unsafe` in any non-test Rust source file without a SAFETY.md entry next to it explaining the invariant the unsafe block relies on.
- The crate root has `#![forbid(unsafe_code)]` unless the crate is explicitly `crates/safety/` (we don't open one).
- FFI crates (`pdfium-render`, `tectonic`) already encapsulate `unsafe`; we do not invent more.

## I.7 Type and Trait Discipline

### Builder pattern when 3+ construction args

```rust
pub struct CompileRequest {
    pub main_file: PathBuf,
    pub engine: Engine,
    pub max_iterations: u32,
    pub timeout_ms: u64,
    pub shell_escape: bool,
}

impl CompileRequest {
    pub fn builder(main_file: impl Into<PathBuf>) -> CompileRequestBuilder {
        CompileRequestBuilder::new(main_file)
    }
}
```

### `#[non_exhaustive]` on every public enum that may grow

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentOp {
    RewriteFile, ReplaceRange, Insert, ApplyUnifiedDiff,
}
```

Adding a variant won't be a breaking API change for downstream.

### Newtypes over primitive strings

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct HunkId(String);
impl HunkId {
    pub fn from_search(s: &str) -> Self {
        Self(blake3::hash(s.as_bytes()).to_hex().as_str()[..16].to_string())
    }
}
```

This sticks the invariant "hunk ids are blake3-truncated hex" in the type system; impossible to confuse with `session_id` or `etag`.

### Serde strict mode

```rust
#[derive(serde::Deserialize, garde::Validate)]
#[serde(deny_unknown_fields)]
pub struct ApplySelectedRequest { /* ... */ }
```

Every payload struct has `deny_unknown_fields`. No exceptions. This prevents silent client drift.

## I.8 `tracing` not `println!`

- Never `println!()` or `eprintln!()` in non-test code. `#![deny(clippy::print_stdout, clippy::print_stderr)]` enforces.
- Use `tracing::{info,warn,error,debug,trace}` macros with structured fields:

```rust
tracing::info!(
    request_id = %req.id,
    file = %req.file_path.display(),
    op = ?req.op,
    "applying agent edit"
);
```

- `#[instrument(skip(_sensitive_field))]` on every public async fn that's part of a request path. Skip large buffers and secrets.
- Configure `EnvFilter` from `RUST_LOG=latex_ai=debug` env; default `info`.
- Prod Subscriber: `tracing_subscriber::fmt().json().finish()` for log ingest; dev: pretty console renderer.

## I.9 Lifetime of Spawned Tasks

A `tokio::task::spawn` returns a `JoinHandle`. **Assert in tests that every spawned task finishes within the test timeout.** This catches leaks:

```rust
#[tokio::test]
async fn agent_loop_completes_on_disconnect() {
    let cancel = CancellationToken::new();
    let task = tokio::spawn(async move { agent_loop(cancel).await });
    cancel.cancel();
    tokio::time::timeout(Duration::from_millis(200), task).await
        .expect("task did not cancel within 200ms")
        .expect("task panicked")
        .expect("task returned Err unexpectedly");
}
```

## I.10 `clippy::pedantic` exceptions (audited)

We deny over `clippy::pedantic` but allow these explicitly only where the overhead is unjust:

```toml
# in Cargo.toml per-crate section
[lints.clippy]
pedantic = { level = "deny", priority = -1 }
must_use_candidate = "allow"           # noise
missing_errors_doc = "allow"           # we don't write rustdoc for every fn
module_name_repetitions = "allow"      # false positives on `crate_name::type_name`
needless_pass_by_value = "allow"       # axum extractors require this
cast_precision_loss = "allow"          # assertions/tests tolerate this
```

Every other `pedantic` lint fires as an error.

## I.11 Async Streams

- Inside SSE handlers, prefer `tokio_stream::wrappers::ReceiverStream<mpsc::Receiver>` consumed by `Sse::new`.
- For pull-based generators, implement `futures::Stream` (do NOT deadloop polling patterns).
- Avoid `async-stream` macros except in tests (the macros hide `unsafe` lifetime state in their expansion).

## I.12 Configuration

- Configuration is layered at startup: defaults < `<home>/config.toml` < environment variables (env wins). Unknown keys are a startup error (`deny_unknown_fields`). Secrets (API keys) live in a separate `0600` file or an env var, never in the readable config file, and their types redact `Debug`.
- **Amended (M8, 2026-10):** the process config is hot-swappable (`ArcSwap<Snapshot>`), because a desktop app must let the user set an API key without restarting. The rule that matters is kept: **every request takes one snapshot at its start and uses only that snapshot**, so a settings change never lands mid-request. A write persists to disk first and swaps second; a failed write changes nothing.
- A golden `config.example.toml` is checked into the repo and a test asserts it parses to exactly the defaults.
- Fields pinned by an environment variable are reported to the UI and refuse changes (409), so a UI edit is never silently overridden on the next start.

## I.13 Filesystem Atomic Writes

```rust
pub async fn atomic_write(path: &Path, content: &[u8]) -> Result<()> {
    let tmp = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    tokio::fs::write(&tmp, content).await.context("write tmp")?;
    tokio::fs::rename(&tmp, path).await.context("rename to final")?;
    Ok(())
}
```

- Use `tokio::fs::rename` (atomic on both POSIX and Windows when same filesystem).
- Never `write+delete+rename` — write+rename is atomic on most platforms.
- The history row and the file must never disagree. **Amended (M7):** the row is inserted inside a transaction, then the file is written atomically, then the transaction commits; a failed write rolls the row back and a failed commit puts the old bytes back. (The earlier order, rename first, returned a 500 for a file that had in fact changed.)

## I.14 Testing Discipline

- Unit tests live next to the code (`#[cfg(test)] mod tests` in same file) for pure logic.
- Integration tests live in `tests/` directory per workspace crate for cross-module flows.
- LLM HTTP mocked via `wiremock-rs` (mock `generativelanguage.googleapis.com`).
- Temp DB used in integration tests via `tempfile::NamedTempFile::new()` + sqlx in-memory `:memory:`.
- `cargo tarpaulin --workspace --fail-under-80` is a hard CI gate.
- Test body naming: `async fn <condition>_<verb>_<outcome>()`:
  - `async fn missing_end_environment_triggers_three_iterations()`.
  - `async fn disconnect_mid_compile_aborts_within_200ms()`.

## I.15 The "Don't Invent" rule (CONTEXT.md §5 — Intentionality concretised for Rust)

Don't open a new workspace crate, new module, new trait, new type, new abstraction layer, new builder, new strategy pattern until a current concrete use-case demands it (idea.md or a tracked issue). AI assistants are unusually prone to "future-proofing" — Turn down the temptation. Today's requirements only.

Before opening a new file/function/type, answer the four Intentionality questions (CONTEXT.md §5) *in the PR description*:

1. Why? Which `idea.md` line or which issue?
2. What calls it / what does it call / what does it read-write?
3. What breaks if removed?
4. Simplest variant that satisfies the requirement?

If you cannot answer all four concretely, the code is unintentional — do not add it.

---

*Part II (the Python-to-Rust port spec) was removed at the close of M5, as it
always said it would be: it was a migration roadmap, and the migration is done.
Part I above and `masterplan.md` remain binding.*
