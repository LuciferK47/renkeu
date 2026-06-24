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

### Tectonic calls need `spawn_blocking` + select

```rust
let pdf_bytes = tokio::select! {
    r = tokio::task::spawn_blocking(move || tectonic::latex_to_pdf(&latex)) => r
        .context("tectonic join")?
        .map_err(CompileError::Tectonic)?,
    _ = cancel.cancelled() => return Err(CompileError::Cancelled),
};
```

### Long mutex holdings

The `TECTONIC_MUTEX` is held for the whole compile (10s±). Holding a `tokio::sync::Mutex` across `.await` on a hot path is fine here because the queue only allows one compile at a time anyway. Document it once; don't re-debate.

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

- Configuration is loaded via `figment::Provider` chain at startup, producing a single immutable `Arc<Config>` cloned into `AppState`.
- **No config reload** during a request lifecycle (would require `ArcSwap` round trip per hot path — out of scope).
- Tunables catalogued in `.latex-ai.toml.example` generated via `figment::test::-doc` at build time, checked into the repo after every change.
- Env vars win over TOML win over defaults, in that order.

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
- History entry write happens AFTER successful rename, not before (CRITICAL — context.md §4.2: state machine integrity).

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

# Part II — Spec for Converting the Existing Python Codebase to Rust (DISPOSABLE)

> This entire Part II is removed at the close of M5 once fully utilised. It is a migration roadmap, not permanent architecture. Part I and `masterplan.md` remain.

## II.1 Python → Rust Module Mapping

| Python file (`src/latex_ai/`) | Rust crate / path | Responsible owner | M-target |
|---|---|---|---|
| `__init__.py` | (gone; use `lib.rs`) | — | M1 |
| `server.py` | `crates/latex_ai_server/src/lib.rs` (`build_router`, `init`) + `src/main.rs` | `latex_ai_server` | M1 |
| `cli.py` | `crates/latex_ai_server/src/cli.rs` (clap derive; subcommands `serve`, `compile`, `desktop`) | `latex_ai_server` | M1 |
| `engine.py` | `crates/latex_ai_core/src/document/engine.rs` (regex + registry dispatch) | `latex_ai_core` | M2 |
| `document.py` | `crates/latex_ai_core/src/document/mod.rs` (semantic LaTex ops, no `pylatexenc` — use `latex-ai-ast-lite` equivalent, or fetch `parser` crate if it matures; V1: regex AST) | `latex_ai_core` | M2 |
| `parser.py` | `crates/latex_ai_core/src/document/parser.rs` (regex AST; substitute for `pylatexenc`); later M6+ may use a real LaTeX parser crate | `latex_ai_core` | M2 |
| `types.py` | `crates/latex_ai_core/src/types.rs` (shared `EditResult`, `ConversationMessage`, `PlanRequest`) | `latex_ai_core` | M2 |
| `commands/__init__.py` | `crates/latex_ai_core/src/commands/mod.rs` (`Command`, `CommandRegistry`, `CommandMetadata`) | `latex_ai_core` | M2 |
| `commands/builtin.py` | `crates/latex_ai_core/src/commands/builtin.rs` (SetMargins etc) | `latex_ai_core` | M2 |
| `document_bundle.py` | `crates/latex_ai_core/src/bundle.rs` (`DocumentBundle`, walkdir + cycle detection + workspace containment) | `latex_ai_core` | M2 |
| `synctex_parser.py` | `crates/synctex/src/lib.rs` (NEW OSS crate) | `synctex` | M3 |
| `pdf_compiler.py` | `crates/compiler/src/lib.rs` (Tectonic in-process + global mutex + cache) | `compiler` | M1/M2 |
| `compilation_queue.py` | `crates/compiler/src/queue.rs` (moka + tokio Semaphore + debounce) | `compiler` | M2 |
| `ai_planner.py` | `crates/latex_ai_core/src/ai/mod.rs` (`EditPlanner` trait) + `genai.rs` (`GeminiPlanner` impl) | `latex_ai_core` | M2 |
| `ai_prompts.py` | `crates/latex_ai_core/src/ai/prompts.rs` | `latex_ai_core` | M2 |
| `ai_streaming.py` | folded into `GeminiPlanner::edits_stream` (`tokio_stream`) | `latex_ai_core` | M2 |
| `ai_cache.py` | gone; use `sqlx` `ai_cache` table directly | — | M2 |
| `history_db.py` | `crates/latex_ai_core/src/storage/history.rs` (sqlx connection pool, async funcs) | `latex_ai_core` | M2 |
| `cache.py` | gone; replaced by moka directly inside services | — | M2 |
| `rate_limiter.py` | gone; `tower-governor` middleware in `latex_ai_server/src/main.rs` | — | M2 |
| `metrics.py` | `crates/latex_ai_core/src/metrics.rs` (atomic counters) | `latex_ai_core` | M2/M5 |
| `streaming.py` + `streaming_helpers.py` | gone; `axum::response::sse::Sse` handlessse directly | — | M2 |
| `structured_logging.py` | gone; `tracing`+`tracing-subscriber` setup in `latex_ai_server/src/telemetry.rs` | — | M1 |
| `security.py` | `crates/latex_ai_core/src/security.rs` (`validate_path`, `validate_workspace_path`, `safe_resolve_path`, `check_symlink_safety`, `sanitize_user_instruction`) | `latex_ai_core` | M2 |
| `state.py` | gone; `AppState` (`#[derive(Clone)] struct AppState { ... Arc<X> ... }`) handed to every handler via `State<AppState>` | — | M1 |
| `config.py` | `crates/latex_ai_core/src/config.rs` (`Config` struct + `figment` loader); delete split `AppSettings` vs `ConfigManager` split-brain | `latex_ai_core` | M1 |
| `models_registry.py` | `crates/latex_ai_core/src/models.rs` (`AVAILABLE_MODELS: &[ModelInfo]` static) | `latex_ai_core` | M2 |
| `resource_management.py` | gone (broken, unreferenced) | — | M1 |
| `plugin_loader.py` | gone (speculative) | — | M1 |
| `lsp_client.py` | gone; replaced by `crates/lsp/src/lib.rs` (in-binary `tower-lsp-server`) | `lsp` | M5 |
| `api/__init__.py` | (gone; use `routes/mod.rs`) | — | M1 |
| `api/dependencies.py` | `crates/latex_ai_core/src/security.rs` (`validate_path`, `get_workspace_root`) | — | M2 |
| `api/schemas.py` | `crates/latex_ai_server/src/schemas/*.rs` (Pydantic → serde strict types) | `latex_ai_server` | M2 |
| `api/files.py` | `crates/latex_ai_server/src/routes/files.rs` | `latex_ai_server` | M2 |
| `api/agent.py` | `crates/latex_ai_server/src/routes/agent.rs` | `latex_ai_server` | M2 |
| `api/compile.py` | `crates/latex_ai_server/src/routes/compile.rs` | `latex_ai_server` | M2 |
| `api/history.py` | `crates/latex_ai_server/src/routes/history.rs` | `latex_ai_server` | M2 |
| `api/health.py` | `crates/latex_ai_server/src/routes/health.rs` | `latex_ai_server` | M1 |
| `api/lsp.py` | `crates/latex_ai_server/src/routes/lsp.rs` (WS route forwarding into `lsp` crate) | `latex_ai_server` | M2 |
| `api/metrics.py` | `crates/latex_ai_server/src/routes/metrics.rs` | `latex_ai_server` | M5 |
| `services/__init__.py` | (gone) | — | — |
| `services/files.py` | `crates/latex_ai_core/src/storage/mod.rs` (atomic write + etag + history + backup) | `latex_ai_core` | M2 |
| `services/ai.py` | `crates/latex_ai_core/src/ai/edits.rs` (apply_agent_edits, compute_proposal_diffs, capture_etags) | `latex_ai_core` | M2 |
| `services/compile.py` | folded into `crates/compiler/src/lib.rs` | — | M2 |
| `services/patch_utils.py` | `crates/latex_ai_core/src/diff.rs` (using `similar` crate; drop dmp dual-path) | `latex_ai_core` | M2 |

## II.2 Filesystem Layout After M1

```
Latex-AI/
├── Cargo.toml                          # workspace
├── Cargo.lock
├── .cargo-deny.toml
├── .latex-ai.config.example.toml       # generated via figment doc macro
├── crates/
│   ├── compiler/                        # tectonic + queue + scratchpad
│   │   ├── Cargo.toml
│   │   ├── src/lib.rs
│   │   ├── src/queue.rs
│   │   └── src/scratchpad.rs
│   ├── synctex/                         # NEW — published as latex-ai-synctex
│   │   ├── Cargo.toml
│   │   └── src/lib.rs
│   ├── visual/                          # pdfium-render crop
│   │   ├── Cargo.toml
│   │   └── src/lib.rs
│   ├── lsp/                             # tower-lsp-server in-binary
│   │   ├── Cargo.toml
│   │   └── src/lib.rs
│   ├── latex_ai_core/                   # use-case agnostic services
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── config.rs
│   │       ├── security.rs
│   │       ├── storage/ { mod.rs, history.rs }
│   │       ├── ai/ { mod.rs, genai.rs, prompts.rs, edits.rs }
│   │       ├── document/ { mod.rs, engine.rs, parser.rs }
│   │       ├── bundle.rs
│   │       ├── commands/ { mod.rs, builtin.rs }
│   │       ├── metrics.rs
│   │       ├── models.rs
│   │       ├── prompt_safety.rs
│   │       ├── types.rs
│   │       └── diff.rs
│   ├── latex_ai_server/                 # axum assembly
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── main.rs                  # bin entry: clap CLI parsing
│   │       ├── lib.rs                   # build_router, AppState, init
│   │       ├── cli.rs
│   │       ├── telemetry.rs
│   │       ├── routes/
│   │       │   ├── mod.rs
│   │       │   ├── files.rs
│   │       │   ├── agent.rs
│   │       │   ├── compile.rs
│   │       │   ├── visual.rs            # M3 NEW
│   │       │   ├── history.rs
│   │       │   ├── health.rs
│   │       │   ├── lsp.rs
│   │       │   └── metrics.rs
│   │       └── schemas/
│   │           ├── mod.rs
│   │           ├── agent.rs             # AgentEdit, AgentRequest, AgentOp, etc.
│   │           ├── files.rs
│   │           ├── compile.rs
│   │           ├── visual.rs
│   │           └── errors.rs            # ApiError envelope
│   └── latex_ai_tauri/                  # Tauri shell (M5)
│       ├── Cargo.toml
│       ├── tauri.conf.json
│       ├── icons/
│       └── src/main.rs
├── migrations/                          # sqlx migrations (sqlx-cli)
│   ├── 0001_baseline.up.sql
│   ├── 0001_baseline.down.sql
│   ├── 0002_compile_queue.up.sql
│   ├── 0002_compile_queue.down.sql
│   ├── 0003_compile_errors.up.sql
│   └── 0003_compile_errors.down.sql
├── .sqlx/                               # offline query metadata for sqlx::query!
│   └── query-xyz.json
├── tests/                               # integration tests shared
└── frontend/                            # unchanged JS — see II.10
    └── ...
```

## II.3 Tectonic Integration Tactics

### Single global mutex

```rust
// crates/compiler/src/lib.rs
use std::sync::OnceLock;
use tokio::sync::Mutex;

static TECTONIC_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();

pub struct Engine {
    scratchpad_root: PathBuf,
    cache: moka::future::Cache<String, Arc<CompileResult>>,
}

impl Engine {
    pub async fn compile_in_scratchpad(
        &self,
        req: CompileRequest,
        cancel: CancellationToken,
    ) -> Result<CompileResult, CompileError> {
        let scratch = self.scratchpad_for(&req.session_id);
        scratch.apply_edits(&req.edits).await?;
        let bundle_hash = self.compute_bundle_hash(&scratch.main_file).await?;
        if let Some(cached) = self.cache.get(&bundle_hash).await {
            return Ok((*cached).clone());  // CompileResult is Clone
        }

        let latex = tokio::fs::read_to_string(&scratch.main_file)
            .await.context("read scratch main")?;

        // Hold the global Tectonic mutex for the duration of the compile
        let _guard = TECTONIC_MUTEX.get_or_init(|| Mutex::new(())).lock().await;

        let latex_clone = latex.clone();
        let pdf_bytes = tokio::select! {
            r = tokio::task::spawn_blocking(move || tectonic::latex_to_pdf(&latex_clone)) => {
                r.context("tectonic join")?
                    .map_err(|e| CompileError::Tectonic(e.into()))?
            }
            _ = cancel.cancelled() => return Err(CompileError::Cancelled),
        };

        tokio::fs::write(&scratch.pdf_path(), &pdf_bytes).await?;
        let result = CompileResult {
            success: !pdf_bytes.is_empty(),
            pdf_bytes: Arc::from(pdf_bytes),
            synctex_path: scratch.synctex_path(),
            pdf_path: scratch.pdf_path(),
            content_hash: bundle_hash,
            errors: vec![],
            warnings: vec![],
        };

        if result.success {
            self.cache.insert(bundle_hash, Arc::new(result.clone())).await;
        }

        Ok(result)
    }
}
```

### Queue sizing

```rust
// crates/compiler/src/queue.rs
use tokio::sync::{Semaphore, Mutex as AsyncMutex};
use std::sync::Arc;

pub struct CompilationQueue {
    inner: AsyncMutex<QueueState>,
    semaphore: Semaphore,  // size 1 — matches Tectonic global mutex reality
    debounce: tokio::time::Duration,
    cache: moka::future::Cache<String, Arc<CompileResult>>,
}

impl CompilationQueue {
    pub async fn submit(&self, req: CompileRequest, cancel: CancellationToken)
        -> Result<Arc<CompileResult>, CompileError>
    {
        // Phase 1: compute bundle hash OUTSIDE the lock.
        let bundle_hash = compute_bundle_hash(&req.main_file).await?;

        let key = (req.main_file.clone(), bundle_hash.clone());
        let permit = self.semaphore.acquire().await
            .map_err(|_| CompileError::EngineShutdown)?;

        // Phase 2: check cache or queue:
        if let Some(cached) = self.cache.get(&bundle_hash).await {
            return Ok(cached);
        }

        // Phase 3: execute (Tectonic's own mutex holds the actual compile here)
        let result = self.engine.compile_in_scratchpad(req, cancel).await?;
        drop(permit);
        Ok(Arc::new(result))
    }
}
```

The `Semaphore::new(1)` matches Tectonic's global mutex — debounced callers coalesce; simultaneous callers wait without consuming a slot.

## II.4 `latex-ai-synctex` crate spec

### Goals

- Parse `.synctex.gz` files via `flate2` (GzDecoder) + `nom` parser combinators.
- Expose `parse(path) -> Result<SyncTeXData, SyncTeXError>`, `backward_search(&data, page, x, y) -> Result<SourceHit, SyncTeXError>`, `forward_search(&data, file, line) -> Result<PdfHit, SyncTeXError>`.
- Zero domain knowledge outside "this is a synctex file". No LaTeX types.
- License: MIT OR Apache-2.0 dual.
- Crate name: `latex-ai-synctex` on crates.io.

### Source reference

[jlaurens/synctex](https://github.com/jlaurens/synctex/blob/main/synctex_parser_readme.md) is the canonical C parser. Port its line-prefix state-machine (`<` for page-open, `>` for page-close, `[` for vertical box, `(` for horizontal box, `h`/`v` for boxes within a page, `I` for input).

Threat model: untrusted `.synctex.gz` files. We reject on first malformed line.

### Skeleton

```rust
// crates/synctex/src/lib.rs
use std::path::{Path, PathBuf};
use std::collections::BTreeMap;
use flate2::read::GzDecoder;
use std::io::{BufRead, BufReader};

#[derive(Debug, thiserror::Error)]
pub enum SyncTeXError {
    #[error("io: {0}")] Io(#[from] std::io::Error),
    #[error("malformed synctex line {n}: {line}")] Malformed { n: usize, line: String },
    #[error("page {0} not found in synctex data")] PageNotFound(u32),
    #[error("file tag {0} not in inputs map")] UnknownTag(u32),
    #[error("no hit on page {page} at ({x}, {y})")] Miss { page: u32, x: f64, y: f64 },
}

#[derive(Debug, Clone)]
pub struct SyncTeXData {
    pub inputs: BTreeMap<u32, PathBuf>,
    pub pages: BTreeMap<u32, Vec<HBox>>,
    pub magnification: u32,
    pub unit: u32,
    pub x_offset: i32,
    pub y_offset: i32,
}

#[derive(Debug, Clone)]
pub struct HBox {
    pub file_tag: u32,
    pub line: u32,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

#[derive(Debug, Clone)]
pub struct SourceHit { pub file: PathBuf, pub line: u32, pub column: u32 }
#[derive(Debug, Clone)]
pub struct PdfHit { pub page: u32, pub x: f64, pub y: f64 }

pub fn parse(path: &Path) -> Result<SyncTeXData, SyncTeXError> {
    let f = std::fs::File::open(path)?;
    let gz = GzDecoder::new(f);
    let reader = BufReader::new(gz);
    let mut data = SyncTeXData { inputs: BTreeMap::new(), pages: BTreeMap::new(), magnification: 1000, unit: 1, x_offset: 0, y_offset: 0 };
    for (n, line_res) in reader.lines().enumerate() {
        let line = line_res?;
        parse_line(&line, &mut data).map_err(|_| SyncTeXError::Malformed { n, line: line.clone() })?;
    }
    Ok(data)
}

fn parse_line(line: &str, data: &mut SyncTeXData) -> Result<(), SyncTeXError> {
    // Single-letter prefix switch ('I' for Input, 'h'/'v' for boxes within page, etc.)
    // Use nom combinators for the remainder, fall back to manual parse for the leading tag.
    unimplemented!("port from C synctex_parser.c lines 539-694")
}

pub fn backward_search(data: &SyncTeXData, page: u32, x: f64, y: f64) -> Result<SourceHit, SyncTeXError> {
    // nearest hbox on `page` to (x, y) in PDF coords
    // convert via unit/magnification; resolve file_tag via data.inputs map
    unimplemented!("M3")
}

pub fn forward_search(data: &SyncTeXData, file: &str, line: u32) -> Result<PdfHit, SyncTeXError> {
    // first hbox whose file_tag.path ends_with(file) && line matches
    unimplemented!("M3")
}
```

## II.5 `genai-rs` Session Wiring

The key win: **Gemini server owns chat history**. We send an initial interaction, receive `interaction_id`, send next turn with `with_previous_interaction(interaction_id)`.

```rust
// crates/latex_ai_core/src/ai/genai.rs
use genai_rs::{Client, Interaction, StreamChunk, ResponseFormat};
use std::sync::Arc;
use tokio::sync::RwLock;
use std::collections::HashMap;

pub struct GeminiPlanner {
    client: Client,
    model: String,
    // session_id → previous_interaction_id (last known)
    sessions: RwLock<HashMap<String, String>>,
}

impl GeminiPlanner {
    pub fn new(api_key: String, model: String) -> Arc<Self> {
        Arc::new(Self {
            client: Client::new(api_key),
            model,
            sessions: RwLock::new(HashMap::new()),
        })
    }

    pub async fn edits_stream(
        &self,
        session_id: String,
        snapshot_text: String,
        intent: String,
        cancel: CancellationToken,
    ) -> Result<Vec<AgentEdit>, anyhow::Error> {
        let prev_id = self.sessions.read().await.get(&session_id).cloned();

        let mut interaction = self.client.interaction()
            .with_model(&self.model)
            .with_text(format!("{}\n\nUser request: {}", snapshot_text, intent));

        if let Some(id) = prev_id {
            interaction = interaction.with_previous_interaction(&id);
        } else {
            interaction = interaction.with_store_enabled();  // gives us a content-addressed ID for next turn
        }

        let response = tokio::select! {
            r = interaction.create() => r?,
            _ = cancel.cancelled() => return Err(anyhow!("cancelled")),
        };

        if let Some(id) = response.id {
            self.sessions.write().await.insert(session_id.clone(), id);
        }

        let text = response.text().ok_or_else(|| anyhow!("empty response"))?;
        let parsed: SerializedResponse = serde_json::from_str(&text)
            .with_context(|| format!("parsing LLM response as JSON: {}", &text[..text.len().min(200)]))?;
        Ok(parsed.edits)
    }

    pub async fn visual_fix(
        &self,
        session_id: String,
        snippet: String,
        crop_png: Arc<[u8]>,
        intent: String,
        cancel: CancellationToken,
    ) -> Result<Vec<AgentEdit>, anyhow::Error> {
        let prev_id = self.sessions.read().await.get(&session_id).cloned();

        let mut interaction = self.client.interaction()
            .with_model(&self.model)
            .with_text(format!("{}\n\nIntent: {}", snippet, intent))
            .with_image_bytes(crop_png.to_vec(), "image/png".to_string())
            .with_response_format(ResponseFormat::Json);  // enforced

        if let Some(id) = prev_id {
            interaction = interaction.with_previous_interaction(&id);
        } else {
            interaction = interaction.with_store_enabled();
        }

        let response = tokio::select! {
            r = interaction.create() => r?,
            _ = cancel.cancelled() => return Err(anyhow!("cancelled")),
        };

        if let Some(id) = response.id {
            self.sessions.write().await.insert(session_id, id);
        }

        let text = response.text().ok_or_else(|| anyhow!("empty VLM response"))?;
        let parsed: SerializedResponse = serde_json::from_str(&text)
            .with_context(|| format!("VLM JSON parse failed: {}", &text[..text.len().min(200)]))?;
        Ok(parsed.edits)
    }
}

#[derive(serde::Deserialize)]
struct SerializedResponse { edits: Vec<AgentEdit> }
```

Note: genai-rs exact API surface (whether `Interaction` exposes `.with_image_bytes`, `.client.interaction().with_model(...)` or `client.interaction(model=...)`) must be verified at the M3 pinning time. Treat this skeleton as illustrative until genai-rs 0.7 docs are finalised in our `Cargo.lock`.

## II.6 `tower-lsp` Server Boot

```rust
// crates/lsp/src/lib.rs
use tower_lsp::{LanguageServer, LspService, Server};
use tower_lsp::lsp_types::{InitializeParams, ServerCapabilities, CompletionOptions};
use std::sync::Arc;

#[derive(Debug)]
struct LatexBackend {
    state: Arc<LspState>,
}

#[tower_lsp::async_trait]
impl LanguageServer for LatexBackend {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult, _> {
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncKind::INCREMENTAL.into()),
                completion_provider: Some(CompletionOptions { trigger_characters: Some(vec!["\\".into()]), ..Default::default() }),
                hover_provider: Some(OneOf::Left(true)),
                definition_provider: Some(OneOf::Left(true)),
                document_symbol_provider: Some(OneOf::Left(true)),
                ..Default::default()
            },
            ..Default::default()
        })
    }
    // did_open, did_change, completion, hover, definition, formatting — see tower-lsp docs
}

pub async fn serve_on_stdio() -> Result<(), anyhow::Error> {
    let (service, socket) = LspService::build(|socket| LatexBackend { state: LspState::new(socket) }).finish();
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    Server::new(stdin, stdout, socket).serve(service).await;
    Ok(())
}
```

Note: the in-binary LSP participates in `routes/lsp.rs` via axum WebSocket bridge that decodes the LSP JSON-RPC message from the browser, dispatches via tower-lsp's internal protocol, and bridges the response back.

## II.7 Tauri Shell Embed

```rust
// crates/latex_ai_tauri/src/main.rs
use tauri::Manager;

fn main() {
    let app_state = latex_ai_server::init().expect("init");
    let router = latex_ai_server::build_router(app_state);

    tauri::Builder::default()
        .plugin(tauri_plugin_axum::init(router))
        .setup(|app| {
            // SSE routes need TCP loopback because Tauri custom protocol doesn't stream
            let webview = app.get_webview_window("main").unwrap();
            let host = "127.0.0.1:18981";
            // spawn axum server bound to host (just our SSE routes subset)
            // ... docs.rs/tauri-plugin-axum examples show the streaming workaround
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

Frontend uses `import.meta.env.VITE_API_BASE` (`axum://localhost` for non-streaming, `http://127.0.0.1:18981` for SSE) — set per-env.

## II.8 sqlx Migrations Seed

```sh
# On first workspace setup, M1:
cargo install sqlx-cli --no-default-features --features sqlite,rustls
cargo sqlx database create --database-url sqlite://$HOME/.latex-ai/history.db
cargo sqlx migrate add baseline
cargo sqlx migrate add compile_queue
cargo sqlx migrate add compile_errors
# Fill in _up/_down .sql files (see masterplan.md §7.1)
cargo sqlx migrate run --database-url sqlite://$HOME/.latex-ai/history.db

# For sqlx::query! macro offline metadata:
cargo sqlx prepare --workspace --check
```

## II.9 Test Scaffolding

### `wiremock-rs` for Gemini HTTP

```rust
// crates/latex_ai_core/tests/ai_streams.rs
use wiremock::{Mock, MockServer, ResponseTemplate};
use wiremock::matchers::{method, path};

#[tokio::test]
async fn stream_emits_thinking_then_proposal_then_done() {
    let server = MockServer::start().await;
    let base_url = server.uri();

    Mock::given(method("POST"))
        .and(path("v1beta/interactions"))
        .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"response":{"text":"{\"edits\":[{\"file_path\":\"main.tex\",\"op\":\"insert\",\"line\":10,\"insert_text\":\"\\\\section{Test}\"}]}","#}))
        .mount(&server).await;

    // Construct GeminiPlanner pointed at base_url via env override; call .edits_stream(..., cancel)
    // Assert SSE stream emits {thinking, proposal, done} in order.
}
```

### `tempfile` for scratchpad

```rust
// crates/compiler/tests/scratchpad_lifecycle.rs
use tempfile::TempDir;
use crates/compiler::ScratchPad;

#[tokio::test]
async fn scratchpad_cleans_on_drop() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();

    {
        let scratch = ScratchPad::create_for("session-1", root.clone()).await.unwrap();
        assert!(scratch.root().exists());
        // dropping scratch should remove its subdir
    }

    assert!(!root.join("session-1").exists(), "scratch not cleaned");
}
```

### `cargo tarpaulin` coverage gate

```sh
cargo tarpaulin --workspace --fail-under-80 --out Xml --output-dir target/coverage
```

CI fails if line coverage < 80%.

## II.10 Frontend Side (Minimal Edits)

Frontend stays Svelte; only the API base URL + malformed client.js need touching.

> **Note (LOCKED 2026-06-25, masterplan §10.1):** `client.js` is now a native
> `fetch` wrapper, **not** axios — ignore the `import axios` in the example below;
> drop it for `fetch`. fuse.js → backend `nucleo` (`GET /api/search`); PDF.js →
> backend `pdfium-render` (`GET /api/pdf/page`). No Rust-UI rewrite (Dioxus/egui
> rejected): Rust UI is same-perf-more-complexity here. See masterplan §10.1.

### Replacements to ship as part of M1/M2:

- **`frontend/src/lib/api/client.js`** — CREATE this file (was missing in audit). Existing services import `{ api, APIClient, APIError }` from it. Single axios-backed wrapper:

```javascript
import axios from 'axios'

export class APIError extends Error {
  constructor(message, status, payload) {
    super(message)
    this.name = 'APIError'
    this.status = status
    this.payload = payload
  }
}

const BASE_URL = import.meta.env.VITE_API_BASE || '/api'

const instance = axios.create({ baseURL: BASE_URL, timeout: 60000 })

export class APIClient {
  async request(method, path, { body, headers, params, signal } = {}) {
    try {
      const res = await instance.request({ method, url: path, data: body, params, headers, signal })
      return res.data
    } catch (err) {
      if (err.response) {
        throw new APIError(err.response.data?.error || err.message, err.response.status, err.response.data)
      }
      throw new APIError(err.message, 0, null)
    }
  }
  getFile(path) { return this.request('GET', '/file', { params: { path } }) }
  saveFile(path, content, etag) {
    return this.request('POST', '/file', { body: { path, content }, headers: etag ? { 'If-Match': etag } : {} })
  }
  compile(filePath, engine = 'pdflatex', signal) {
    return this.request('POST', '/compile', { body: { file_path: filePath, engine }, signal })
  }
  getCompileStatus(jobId, signal) { return this.request('GET', `/compile/status/${jobId}`, { signal }) }
  checkCompileCache(filePath, outputDir = null, signal) {
    return this.request('POST', '/compile/check-cache', { body: { file_path: filePath, output_dir: outputDir }, signal })
  }
  syncTex(path, page, h, v) { return this.request('POST', '/synctex', { body: { file_path: path, page, h, v } }) }
  getPdfUrl(jobId) { return `${BASE_URL}/compile/stream/${jobId}` }
  getConfig() { return this.request('GET', '/config') }
  setConfig(config) { return this.request('POST', '/config', { body: config }) }
  repairVisualCrop(filePath, page, x, y, w, h, intent) {
    return this.request('POST', '/visual-fix', { body: { file_path: filePath, page, x, y, w, h, intent } })
  }
  buildUrl(endpoint) { return `${BASE_URL}${endpoint}` }
}

export const api = new APIClient()
```

- **`frontend/src/services/_stream.js`** — CREATE; SSE parser (single canonical):
```javascript
export async function* streamSSE(response) {
  const reader = response.body.getReader()
  const decoder = new TextDecoder()
  let buffer = ''
  let event = 'message'
  while (true) {
    const { done, value } = await reader.read()
    if (done) break
    buffer += decoder.decode(value, { stream: true })
    const lines = buffer.split('\n')
    buffer = lines.pop()
    for (const line of lines) {
      if (line.startsWith('event: ')) event = line.slice(7).trim()
      else if (line.startsWith('data: ')) {
        const payload = line.slice(6)
        if (!payload.trim()) continue
        yield { event, data: JSON.parse(payload) }
        event = 'message'
      }
    }
  }
}
```

- **`frontend/src/services/aiService.js` / `compileService.js` / `fileService.js` / `configService.js`** — refactor each to use the consolidated client and `_stream.js`; drop the duplicated inline parsers.

- **`frontend/vite.config.js`** — bump Vite to 8; add `define: { 'import.meta.env.VITE_API_BASE': JSON.stringify(process.env.VITE_API_BASE || '/api') }` and proxy both `/api` and `/ws` to `http://localhost:3000`.

- **NEW `frontend/src/components/VisualFixOverlay.svelte`** (M3) — PDF.js + canvas drag-select bounding box; emits on submit.

- **`frontend/src/components/DiffView.svelte`** (M4) — promote from inline `App.svelte` scaffolding; hunk-level accept/reject per stable hunk_id.

## II.11 Migration Sequence by Milestone

### M1 deliverable file checklist (delete Python; stand up Rust skeleton):

```
DEL:
  src/latex_ai/                            # entire Python package
  Makefile                                # replaced by `cargo` commands
  pyproject.toml                           # gone
  pytest.ini                              # gone
  docker-compose.yml                      # rewrite for Rust (see masterplan §9)
  Dockerfile                               # rewrite (see masterplan §9)
  frontend/src/apiClient.js                # gone; replaced by lib/api/client.js
  frontend/src/api.js                      # gone; consolidate into services
  frontend/src/services/*.js               # rewritten to use new client.js
  resource_management.py references        # gone
  plugin_loader references                 # gone

ADD:
  Cargo.toml                              # workspace root
  Cargo.lock                              # generated by cargo build
  .cargo-deny.toml
  crates/compiler/                        # see II.1
  crates/synctex/                         # NEW; published at M3
  crates/visual/                          # see II.1
  crates/lsp/                             # see II.1
  crates/latex_ai_core/                   # see II.1
  crates/latex_ai_server/                 # see II.1
  crates/latex_ai_tauri/                  # stub for M5
  migrations/                              # sqlx migrations
  .sqlx/                                  # offline query metadata
  README.md                                # rewrite from scratch
  .github/workflows/ci.yml                # rewrite for Cargo gates
  .latex-ai.config.example.toml           # generated via figment doc macro
```

### M2 deliverable file checklist (full server functionality minus VLM):

- Implement `crates/compiler/src/queue.rs`, `crates/compiler/src/scratchpad.rs`.
- Implement `crates/latex_ai_core/src/ai/mod.rs` + `genai.rs` + `edits.rs`.
- Implement `crates/latex_ai_server/src/routes/{agent.rs, files.rs, compile.rs, history.rs, health.rs, lsp.rs, metrics.rs}`.
- Implement `crates/latex_ai_server/src/schemas/errors.rs` (ApiError).
- Implement `crates/latex_ai_server/src/middleware/cancel_on_disconnect.rs` (Tower layer; Drops the SSE receiver when client disconnects → emits `CancellationToken` cancel).
- Wire `tower-governor` middleware on agent routes.
- Delete `frontend/src/services/*.js` duplicates, write `frontend/src/lib/api/client.js` and `frontend/src/lib/api/_stream.js` per II.10.

### M3 deliverable file checklist:

- Implement `crates/synctex/src/lib.rs` (~600 LOC nom parser).
- Implement `crates/visual/src/lib.rs` (`pdfium_render` crop render).
- Implement `crates/latex_ai_server/src/routes/visual.rs` (`POST /api/visual-fix`).
- Publish `latex-ai-synctex` to crates.io as v0.1.0.
- Add frontend `VisualFixOverlay.svelte` + wire to `services/visualService.js` (new file using `client.repairVisualCrop`).

### M4 deliverable file checklist:

- Implement `crates/latex_ai_core/src/diff.rs` (`similar` crate unified-diff formatter).
- Implement `crates/latex_ai_server/src/routes/agent.rs::apply_selected` (`POST /api/agent/apply-selected`).
- Implement `HunkId` newtype in `crates/latex_ai_core/src/types.rs`.
- Promote `frontend/src/components/DiffView.svelte` from inline scaffolding in `App.svelte` into a real component; add per-hunk accept/reject UI.

### M5 deliverable file checklist:

- Implement `crates/latex_ai_tauri/src/main.rs` (Tauri shell + axum router embed).
- Implement `tauri.conf.json` with bundlers per-OS.
- Implement `crates/latex_ai_server/src/telemetry.rs` (`tracing-subscriber` JSON layer for prod, fmt for dev).
- Wire `core::metrics::record_*` into every router handler.
- Add `migrations/0003_compile_errors.{up,down}.sql`.
- Persist `CompileQueue::active_jobs` to `compile_queue` table.
- **REMOVE THIS Part II from rust.md once all milestones green.**

## II.12 Cargo.toml Skeleton Files

### `Cargo.toml` (workspace root)

```toml
[workspace]
resolver = "2"
members = [
    "crates/compiler",
    "crates/synctex",
    "crates/visual",
    "crates/lsp",
    "crates/latex_ai_core",
    "crates/latex_ai_server",
    "crates/latex_ai_tauri",
]

[workspace.package]
edition = "2024"
rust-version = "1.85"
license = "MIT OR Apache-2.0"
repository = "https://github.com/sahid/Latex-AI"

[workspace.dependencies]
tokio = { version = "1", features = ["rt-multi-thread", "macros", "fs", "signal", "sync", "time", "io-util"] }
tokio-util = { version = "0.7", features = ["rt"] }
tokio-stream = "0.1"
futures-util = "0.3"
axum = { version = "0.8", features = ["macros", "ws", "tokio"] }
tower = "0.5"
tower-http = { version = "0.6", features = ["cors", "trace", "compression-gzip", "limit"] }
tower-governor = "0.5"
tectonic = { version = "0.16", default-features = false }
genai-rs = "0.7"
sqlx = { version = "0.8", default-features = false, features = ["runtime-tokio-rustls", "sqlite", "macros", "migrate", "chrono"] }
moka = { version = "0.12", features = ["future", "sync"] }
governor = "0.7"
blake3 = "1.5"
thiserror = "2"
anyhow = "1"
garde = { version = "0.20", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.8"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt", "json"] }
tower-lsp-server = "0.21"
lsp-types = "0.97"
similar = { version = "2.6", features = ["text"] }
pdfium-render = { version = "0.8", features = ["thread_safe"] }
flate2 = "1.0"
nom = "7"
tauri = "2"
tauri-plugin-axum = "2"
clap = { version = "4.5", features = ["derive", "env"] }
figment = { version = "0.10", features = ["toml", "env"] }

[workspace.dev-dependencies]
wiremock = "0.6"
pretty_assertions = "1.4"
tempfile = "3"
tokio-test = "0.4"

[profile.release]
opt-level = "z"
lto = "fat"
codegen-units = 1
strip = true
panic = "abort"
```

### `crates/compiler/Cargo.toml`

```toml
[package]
name = "latex-ai-compiler"
version = "0.1.0"
edition.workspace = true
license.workspace = true

[dependencies]
tokio = { workspace = true }
tokio-util = { workspace = true }
tectonic = { workspace = true }
moka = { workspace = true }
blake3 = { workspace = true }
thiserror = { workspace = true }
anyhow = { workspace = true }
tracing = { workspace = true }
serde = { workspace = true }
parking_lot = "0.12"

[dev-dependencies]
tempfile = { workspace = true }
tokio-test = { workspace = true }

[lints]
workspace = true
```

### Crate root `#![deny(...)]` lint baseline

In every workspace crate:

```rust
// crates/<crate>/src/lib.rs
#![deny(
    rust_2024_compatibility,
    missing_debug_implementations,
    unreachable_pub,
    unused_must_use,
    rust_2018_idioms,
    elided_lifetimes_in_paths,  // explicit > implicit
)]
#![deny(clippy::all)]
#![deny(clippy::pedantic)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::dbg_macro)]
#![deny(clippy::todo)]
#![deny(clippy::print_stdout)]
#![deny(clippy::print_stderr)]
#![allow(clippy::must_use_candidate)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::needless_pass_by_value)]
#![allow(clippy::cast_precision_loss)]
#![forbid(unsafe_code)]
```

## II.13 Deletion Candidates (CONTEXT.md §5 — Intentionality audits run on Python residue)

These Python-side constructs are deleted without replacement in the Rust edition. Re-evaluation at M6+ with confirmed need:

| Construct | Deletion rationale |
|---|---|
| `plugin_loader.py` | No callers; speculative extension architecture with no known user. |
| `resource_management.py` | Bare-bracket tokens (`[TIMEOUT_MANAGEMENT]` etc.) made it unimportable; no caller in repo. |
| `services/ai.py::_circuit_breaker_state` state machine | No tests; no demonstrated flapping on Gemini API. Re-introduce only with a test that proves flapping and a fix that demonstrates the breaker tripping opens + auto-closes. |
| `cache.py::ResponseCache` PDF-level cache | Conflict with `crates/compiler` moka bundle-hash cache; one cache per concern chose the bundle-hash-keyed cache. |
| `commands/` plugin discovery at runtime | Manual `register_all(&mut CommandRegistry)` at boot is enough. No need for `inventory` or runtime plugin scan. |
| `api/agent.py` `/api/agent/patch` deprecated endpoint | Rust rewrite drops deprecated routes; the structured `POST /api/agent` supersedes. |
| `streaming.py` and `streaming_helpers.py` dual SSE helpers | Replaced by axum-native `Sse<Event>` and a single frontend `_stream.js`. |
| `structured_logging.py` custom decorator wrappers | Replaced by `tracing::instrument` macro — no custom shape needed. |
| `models_registry.py` single-file linear list | Becomes `crates/latex_ai_core/src/models.rs` as a static slice; pattern unchanged, file format changed. |

---

# Closing

This file (rust.md) is binding. Part I is permanent; treated like CONTEXT.md for Rust-specific clauses. Part II is a migration spec and is removed from the repo at M5 close. Until then, every implementation session re-reads Part I before any Rust edit.

*Document version: 1.0*
*Date: 2026-06-24*
*Companion docs: `CONTEXT.md`, `idea.md`, `masterplan.md`, `opencode.json`*