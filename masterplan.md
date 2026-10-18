# LaTeX-AI — Master Architecture Document (Rust Edition)

> **Purpose**: Implement the Autonomous Document Engine described in `idea.md` as a Rust-native codebase where Tectonic runs as an in-process LaTeX engine (no subprocess, no TeX Live install on host), axum serves both as standalone daemon and embedded inside a Tauri desktop shell (single binary, ≤25 MB), and `genai-rs` owns chat history via Gemini's Interactions API (we don't manage state).
> **Date**: 2026-06-24 (Rust pivot — supersedes the Python masterplan)
> **Status**: Master reference. Supersedes README/GUIDE/incomplete architectural comments in the existing repo when those conflict. `CONTEXT.md` is the binding law; `idea.md` is the product pitch; `rust.md` is the binding coding guideline (its Part II port spec was removed at the close of M5); this doc is the engineering execution plan and reads all three as inputs.
> **Reference sources**:
> - `CONTEXT.md` — four pillars (Modularity / Completeness / Auditability / Intentionality), workflow, red flags
> - `idea.md` — three-pillar pitch, M1–M5 milestones, locked Rust libraries, audit gates, 18 contracts
> - `rust.md` — Rust coding guidelines (Part I). Part II (the port spec) was removed at the close of M5, as it always said it would be.
> - State-of-practice scan (2026): `tectonic` Rust docs ([docs.rs/tectonic](https://docs.rs/tectonic)), `genai-rs` ([docs.rs/genai-rs](https://docs.rs/genai-rs)), `tauri-plugin-axum` ([docs.rs/tauri-plugin-axum](https://docs.rs/tauri-plugin-axum)), `ferroleaf` (peer Rust LaTeX editor using Iced + pdfium-render), `r/tauri` 2026-03 thread (114-route axum embedded in Tauri 2 at 18 MB), peer `TexGuardian` / `PaperFit` / `scholia` (Python LLM-LaTeX agents minus the in-process-engine advantage)

---

## 1. Architecture Philosophy

**Local-first, single-binary Autonomous Document Engine, written in Rust to earn every Rust advantage and not as a port.**

Three operating pillars (from `idea.md`):
1. **Self-healing compile loop** — agent writes an `AgentEdit[]` patch into a `~/.latex-ai/scratch/<session>/` scratchpad, calls `tectonic::latex_to_pdf` in-process (single global mutex serializes compilations), parses the structured log (Tectonic emits `ProcessingEvents` via channel — we never parse text again), and feeds errors back into `genai_rs::Interaction::with_previous_interaction(id)` as the next user message until success or max-iter.
2. **Visual grounding via reverse SyncTeX** — user draws a bbox on the PDF; `latex-ai-synctex::backward_search(page, x, y)` maps to source; `pdfium-render` crops the page region to PNG; `genai_rs::Interaction::with_response_format(JSON)` returns `{ edits: AgentEdit[] }`.
3. **Deterministic diff-driven control** — every AI change is an `AgentEdit` serde-strict type with 1-based inclusive lines + `expected_etag: Option<Hash>` (blake3); frontend `DiffView` accepts/rejects per hunk via `POST /api/agent/apply-selected`.

Cross-cutting principles from `CONTEXT.md` (binding):

| # | Principle | Masterplan enforcement |
|---|---|---|
| P1 | Modularity — Document Pipeline is platform; LaTeX is adapter | §3 layered view; §5 module boundary rules; `compiler` crate doesn't know about `lsp`; `lsp` doesn't know about `server`. |
| P2 | Completeness — no half-implementation (Stop Rule) | §12 phase gates with explicit acceptance + audit-gate blocks; no milestone merges without `cargo nextest run` green + clippy pedantic green + `cargo deny` clean. |
| P3 | Auditability — every milestone emits a 4-axis audit record | §12 per-phase Audit Gate blocks; §7 schema audit on every migration; §13 Cargo-decision rationale column. |
| P4 | Intentionality — every line answers Why/What/Breaks/Simplest | §11 deletion candidates (Python repo deleted wholesale in M1); §13 rationale per decision. |

**Existing checkout state**: the audit from sessions past identified several breaking bugs (`document_bundle.py` syntax error, `api/health.py` try-without-except, missing `frontend/src/lib/api/client.js`, etc.). Instead of patching these in Python, M1 abandons the Python code entirely (delete `src/latex_ai/`) and starts fresh in Rust.

---

## 2. Technology Stack & Justification

### 2.1 Core Stack (latest stable as of 2026-06)

| Layer | Crate / Tool | Latest | Why |
|---|---|---|---|
| **Language** | Rust | 1.85+ (2026 stable) | `std::backtrace` stable; `async fn` in traits stable; let-else; let-chains. Edition 2024. |
| **LaTeX engine** | `tectonic` ≥ 0.16 ([docs.rs/tectonic](https://docs.rs/tectonic)) | latest | **Killer USP.** `tectonic::latex_to_pdf(&str) -> Result<Vec<u8>>` returns PDF bytes in-process. XeTeX-based (Unicode + modern fonts default). No TeX Live install on host. Auto-fetches missing packages (content-address cache at `~/.cache/Tectonic/`). Single global mutex — our queue serializes anyway. SyncTeX output via `synctex: true` in `Tectonic.toml`. |
| **Web framework** | `axum` ≥ 0.8 + `tokio` ≥ 1.42 + `tower` ≥ 0.5 + `tower-http` ≥ 0.6 | latest | Standard Rust async web. Macro-free routing, typed extractors, integrated SSE via `axum::response::sse::{Event, Sse, KeepAlive}`. |
| **Desktop shell** | `tauri` ≥ 2.9 + `tauri-plugin-axum` | latest | Same Rust binary serves (a) standalone daemon binding TCP, or (b) Tauri webview via `axum://localhost/` custom protocol for non-streaming routes + TCP loopback for SSE routes. Peer (`r/tauri` 2026-03) shipped 18 MB binary with 114 axum routes; we target ≤25 MB. |
| **SSE** | `axum::response::sse::Sse<Stream<Item = Result<Event, anyhow::Error>>>` | builtin | Native to axum; `KeepAlive` keep-alives the wire automatically. |
| **LLM/VLM SDK** | `genai-rs` ≥ 0.7 ([docs.rs/genai-rs](https://docs.rs/genai-rs)) | latest | Uses Gemini Interactions API natively. **`previous_interaction_id` mechanism means Gemini server owns chat history, we get back a content-addressed ID**. Eliminates our need to manage `conversation_context` HashMaps across sessions. `with_response_format` for strict JSON; `#[tool]` macro for AFC; `StreamChunk::Delta` typed streaming; multimodal via `with_image_bytes`. |
| **VLM model** | `gemini-2.5-flash-image` (pin at M3) | current Gemini image-capable | Forced JSON via `with_response_format(JSON)`. |
| **Diff** | `similar` ≥ 2.6 ([docs.rs/similar](https://docs.rs/similar)) | latest | State-of-the-art Rust diff (used by rustc itself). Unified diff and line-level via `Algorithm::Myers` and patience fallback. |
| **PDF rasterize (crop + full viewer)** | `pdfium-render` ≥ 0.8 + tectonic PDF output | latest | Google's pdfium via Rust bindings (BSD-3); `PdfRender::render_pdf_page_to_bitmap` with clip rect for the visual-fix crop, full-page render for the viewer (`GET /api/pdf/page`, replaces PDF.js — §10.1). `ferroleaf` proved this combo works in production Rust 2026. |
| **Fuzzy search** | `nucleo` ≥ 0.5 | latest | Helix's matcher. Backs `GET /api/search` for command-palette/file fuzzy queries (replaces frontend fuse.js — §10.1). |
| **SyncTeX parser** | **NEW `latex-ai-synctex` crate** (we publish) | NEW | No mature Rust `.synctex.gz` parser exists. Port the regex-over-GZIP-format via `flate2` + `nom` parser combinators (~600 LOC). MIT-licensed OSS contribution. |
| **LSP server** | `tower-lsp-server` (community fork) + `lsp-types` | latest | In-binary LaTeX LSP — no texlab subprocess dependency. We become the LSP. Invention: built-in diagnostics/completions even on machines with no `texlab` installed. |
| **Database** | `sqlx` ≥ 0.8 + `sqlx` runtime `sqlite` (rustls) + `sqlx-cli` for migrations | latest | Compile-time-checked queries (`query!`, `query_as!` macros). Whole classes of SQL bugs eliminated at `cargo check`. SQLite (file or in-memory) valid for our local-first single-user scale. |
| **Cache** | `moka` ≥ 0.12 with features `["future", "sync"]` | latest | State-of-the-art Rust cache; TinyLFU + LRU + size-aware. `Expiry` trait per-key. |
| **Rate limiting** | `tower-governor` ≥ 0.5 + `governor` ≥ 0.7 | latest | Generic Cell Rate Algorithm. Tower middleware wired as `tower::ServiceBuilder`. Per-key on `X-Session-ID` header. |
| **Hashing (ETag)** | `blake3` ≥ 1.5 | latest | SIMD parallel, ~10× faster than SHA-256, byte-stream incremental. |
| **Errors** | `thiserror` ≥ 2 for libraries; `anyhow` ≥ 1 for app layer | latest | Production 2026 consensus; `.context()`/`.with_context()` at every trust boundary. |
| **Validation** | `serde` ≥ 1 + `serde_json` ≥ 1 + `garde` ≥ 0.20 | latest | `garde` is the 2026 successor to `validator`: async-capable, derives `Validate`, declares field rules. `#[serde(deny_unknown_fields)]` everywhere in payloads. |
| **Async cancellation** | `tokio-util` ≥ 0.7 `CancellationToken` | latest | Thread-safe token; `tokio::select!` against token cancel within `.await` boundaries. Required for SSE disconnect cleanup. |
| **Tracing/logging** | `tracing` ≥ 0.1 + `tracing-subscriber` ≥ 0.3 (`EnvFilter`, `fmt`, `json`) | latest | Structured spans with `#[instrument]` macro; layered output via `Layer`. Optional `tracing-opentelemetry` at M5+. |
| **Config** | `serde` + `toml` + `figment` ≥ 0.10 | latest | Layered env+file+args config with auto-generated `.config.example.toml` from `figment`'s derive. No env-vs-toml split-brain (kills the Python bug). |
| **Frontend** | Svelte 5 (kept — JS) + Vite 8 + Monaco + native `fetch`; PDF view via backend `pdfium-render`, fuzzy via backend `nucleo` | latest | Frontend is JavaScript (language decision LOCKED, §10.1). Pivot of backend to Rust doesn't force a frontend rewrite. Rust-leaning pieces (PDF render, fuzzy search) land as backend endpoints, not a UI rewrite. axios/PDF.js/fuse.js dropped. |
| **CLI** | `clap` ≥ 4.5 with `derive` | latest | Single `latex-ai` binary entry; subcommands `serve`, `desktop` (Tauri shell), `tui` (ratatui, M5+ optional). |
| **Container build** | `rust:1-slim-bookworm` builder + **`scratch`** runtime holding the binary's `ldd` closure (§15.1) | — | **62.8 MB unpacked, 24.9 MB compressed**, against a ≤ 200 MB target (vs ~4 GB via full TeX Live). Sizes are measured unpacked; `docker images` double-counts on the containerd store. |
| **CI** | `cargo nextest` + `cargo tarpaulin` + `cargo deny` + `cargo audit` + `cargo machete` + `cargo clippy -- -D warnings -D clippy::pedantic` + `cargo fmt --check` | latest | All gates mandatory in `.github/workflows/ci.yml`. |
| **Workspace builder** | `cargo` with single `Cargo.toml` workspace + single `Cargo.lock` | latest | No `cargo-hakari` unless proven scale need. Simpler builds, smaller closure. |

### 2.2 Versions pin (illustrative — exact in `Cargo.lock`)

```toml
# Cargo.toml (workspace root)
[workspace]
resolver = "2"
members = ["crates/*"]

[workspace.package]
edition = "2024"
rust-version = "1.85"
license = "MIT OR Apache-2.0"

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
```

---

## 3. System Architecture

### 3.1 Layered view (Cargo workspace)

```
                  ┌─────────────────────────────────────────────────────────┐
                  │ Browser tab OR Tauri webview (single user, local-first)  │
                  │  Svelte 5 SPA:  Monaco + pdfium PNG viewer + SSE parser │
                  └─────────────────────┬──────────────────────────────────┘
                                        │ HTTP/SSE + WebSocket (lsp)
                                        │  (Tauri: axum://localhost/ custom protocol
                                        │         OR 127.0.0.1:<port> TCP for SSE)
                  ┌─────────────────────▼──────────────────────────────────┐
                  │ crate `latex_ai_server` (axum app assembly + middleware)│
                  │  routes/ : files.rs, agent.rs, compile.rs, visual.rs    │
                  │             history.rs, health.rs, lsp.rs, metrics.rs │
                  │  middleware: trace_id, governor, cors, cancel_on_disconnect│
                  │  state: AppState (Arc) hand-pasted into every handler   │
                  └─────────────────────┬──────────────────────────────────┘
                                        │
                  ┌─────────────────────▼──────────────────────────────────┐
                  │ crate `latex_ai_core` (use-case agnostic services)     │
                  │  modules: storage / ai / compile / visual / security    │
                  │           / queue / models_registry / telemetry        │
                  └──┬───────────────┬───────────────┬─────────────────────┘
                     │               │               │
        ┌────────────▼────┐ ┌────────▼────────┐ ┌────▼──────────────┐
        │ crate `compiler`│ │   crate `lsp`   │ │ crate `visual`     │
        │ Tectonic in-proc│ │ tower-lsp-server│ │ pdfium-render      │
        │ + queue + cache │ │ + latex syntax  │ │ + crop render      │
        └────────┬────────┘ └────────┬────────┘ └────┬──────────────┘
                 │                   │               │
        ┌────────▼───────────────────▼───────────────▼───────────────┐
        │ crate `synctex` (NEW we publish; port of C + our own nom impl"  │
        └─────────────────────────────────────────────────────────────┘
                 │
        ┌────────▼────────────────────────────────────────────────────┐
        │ crate `latex_ai_tauri` (Tauri shell — thin wrapper that     │
        │  embeds the axum router + opens a webview of the SPA build) │
        └─────────────────────────────────────────────────────────────┘

Persistence: SQLite (~/.latex-ai/history.db) via sqlx; on-disk scratch + cache dir
```

Boundaries (P1 modularity, enforced by code review and lint discipline; `cargo-modules` once mature):

- `latex_ai_server` never imports `tectonic` nor `tower_lsp_server` nor `pdfium_render` directly; only via `core::compile::CompileRequest`/`core::lsp`/`visual` modules.
- `latex_ai_core` never imports from `latex_ai_server` or `latex_ai_tauri`. Pure services.
- `compiler` crate owns `tectonic` (and global mutex serialization). Other crates use `compiler::Engine::submit` only.
- `lsp` crate owns `tower_lsp_server` + language-specific logic.
- `visual` crate owns `pdfium_render`. Pulls `compiler::CompileResult` for PDF path.
- `synctex` crate has zero knowledge of LaTeX semantics — file-format parser only.

### 3.2 Two runtimes in one binary

```
DEV (2 procs):
  cd frontend && npm run dev    # Vite 8 dev server :5173 with proxy → backend :3000
  cargo run -p latex_ai_server # axum on 127.0.0.1:3000; cargo watch -x run

PROD (1 binary):
  npm run build (Vite 8 → frontend/dist)
  cargo build --release -p latex_ai_tauri   # bundles frontend/dist + axum into Tauri shell
  ./target/release/latex-ai desktop    OR
  ./target/release/latex-ai serve --port 8000
```

Per `idea.md` Non-Goals: no multi-process, no Redis, no Postgres at V1. Singletons become `OnceLock<Arc<X>>` or `axum::extract::State` fields. M5+ escalates only with documented requirement.

### 3.3 Reversed call paths

```
HUMAN EDIT (deterministic):
  Editor.svelte → POST /api/apply {file_path, command, dry_run, if_match}
  → router::files::apply
  → core::storage::save_with_etag_history (atomic write + history entry)
  → core::document::LatexEngine::apply_command (command registry + regex fallback)
  → returns ApplyResponse with diff (similar::udiff)

AI PLAN STREAM:
  Editor.svelte → POST /api/plan/stream (SSE)
  → router::agent::plan_stream
  → TokenBucketRateLimiter (tower-governor middleware)
  → core::security::validate_path
  → core::ai::EditPlanner::edits_stream(session, snapshot, intent) → tokio_stream
  → SSE emits {Event::default().event("progress").data(json)}
  → cancel_on_disconnect_layer detects HTTP close → cancel token signal

AGENT HEAL LOOP:
  Editor.svelte → POST /api/agent-loop/stream
  → router::agent::agent_loop
  → loop iter 0..max_iter:
      snapshot = core::bundle::DocumentBundle::load_async(main_file)
      edits   = planner.edits_stream(session, snapshot, intent).next()
      scratch = core::compile::ScratchPad::create(session_id, snapshot)
      scratch.spawn_apply_edits(edits)
      compile_result = compiler::Engine::submit(scratch.main_path, CancellationToken).await
      match compile_result:
        Ok(r) if r.success ⇒ apply edits to user file, write history, emit done
        Ok(r)              ⇒ consume structured errors → planner.next_turn(errors) → continue
        Err(timeout)       ⇒ emit done(success=false), break
  → on disconnect: cancel the in-flight tectonic invocation (drop cancels future chain)

VISUAL FIX:
  Editor.svelte → POST /api/visual-fix {file_path, page, x, y, w, h, intent}
  → router::visual::fix
  → core::security::validate_path
  → synctex::backward_search(synctex_path, page, x, y) ⇒ SourceHit{file, line}
  → snippet = expand ±30 lines
  → visual::render_crop(pdf_path, page, x, y, w, h, dpi=150) ⇒ PNG bytes (Arc<[u8]>)
  → planner.visual_fix(snippet, png_bytes.clone(), intent) ⇒ Vec<AgentEdit>
  → return ProposalResponse (do not auto-apply)

PER-HUNK ACCEPT:
  Editor.svelte → POST /api/agent/apply-selected { edits, selected_hunk_ids }
  → router::agent::apply_selected
  → hunk_id = blake3(edit.search).to_hex()[..16] (server-side check)
  → core::storage::atomic_apply_selected_async(file_path, selected_edits, history_tag="apply-selected")
  → Either<Ok{diff}, Err{ConflictReason, current_etag, current_content}>
```

---

## 4. Component Design

### 4.1 `crates/compiler` — Tectonic wrapper + queue + cache + scratchpad

Why: Tectonic instances are not thread-safe; `OnceLock<tokio::sync::Mutex<()>>` serializes. The queue's `Semaphore::new(1)` mirrors this. `ScratchPad` writes per-session working tree at `~/.latex-ai/scratch/<session_id>/`.

```rust
// crates/compiler/src/lib.rs
use std::sync::OnceLock;
use tokio::sync::Mutex;
use blake3::Hasher;

pub struct Engine {
    bundle_cache: PathBuf,
    scratchpad_root: PathBuf,
    cache: moka::future::Cache<String, Arc<CompileResult>>,
}

static TECTONIC_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();

impl Engine {
    pub async fn compile_in_scratchpad(
        &self,
        req: CompileRequest,
        cancel: CancellationToken,
    ) -> Result<CompileResult, CompileError> {
        let scratchpad = self.scratchpad_for(&req.session_id);
        scratchpad.apply_edits(&req.edits).await?;
        let bundle_hash = self.compute_bundle_hash(&scratchpad.main_file).await?;

        if let Some(cached) = self.cache.get(&bundle_hash).await {
            return Ok((*cached).clone());
        }

        let latex = tokio::fs::read_to_string(&scratchpad.main_file)
            .await.context("reading scratch main")?;

        let _guard = TECTONIC_MUTEX
            .get_or_init(|| Mutex::new(()))
            .lock()
            .await;

        // Tectonic 0.16 latex_to_pdf is sync; run on blocking pool and watch cancel
        let latex_clone = latex.clone();
        let pdf_bytes = tokio::select! {
            r = tokio::task::spawn_blocking(move || tectonic::latex_to_pdf(&latex_clone)) => {
                r.context("tectonic join")?
                    .map_err(|e| CompileError::Tectonic(e.into()))?
            }
            _ = cancel.cancelled() => return Err(CompileError::Cancelled),
        };

        let synctex_path = scratchpad.synctex_path();
        let pdf_path = scratchpad.pdf_path();
        tokio::fs::write(&pdf_path, &pdf_bytes).await?;

        let result = CompileResult {
            success: !pdf_bytes.is_empty(),
            pdf_bytes: Arc::from(pdf_bytes),
            synctex_path,
            pdf_path,
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

Two structural wins over the Python iteration:
1. `tectonic::latex_to_pdf` returns `Result<Vec<u8>>` — the entire PDF is in memory; we write to disk only once for runtime scratchpad. No `.synctex.gz` strip-down needed; we accept full SyncTeX file from the scratchpad.
2. ~~No subprocess: cancellation is a tokio future drop.~~ **Amended in M10 (§15.1):** Tectonic cannot be interrupted and holds a process-wide lock for the whole compile, so dropping a future left the engine (and every later compile) stuck behind it. Compiles run in a short-lived worker process — this same binary, re-executed — which is killed on cancel or timeout. There are still no TeX Live or `pdflatex` children.

**Engine auto-selection**: Tectonic is XeTeX-based by default; this question simply doesn't apply for V1 — every document compiles with XeLaTeX. (LuaLaTeX support deferred until Tectonic ships it; tracked Q2.)

### 4.2 `crates/lsp` — in-binary LaTeX LSP

Why: We become the LaTeX LSP. No `texlab` subprocess dependency. Invention — unique in the peer landscape.

Surface (minimal V1):
- `textDocument/didOpen`, `textDocument/didChange` — refresh internal `Cow<str>` buffer.
- `textDocument/publishDiagnostics` — basic checks: `\begin{...}` without matching `\end`, unbalanced braces, missing required packages vs. observed ones.
- `textDocument/completion` — package names (curated list, moka cache), environment names, `\ref{}` label completion against document AST.
- `textDocument/hover` — package docs (short moka-cached descriptions).
- `textDocument/definition` — `\ref{}` resolution to label positions in `DocumentBundle`.

M5+ features: `textDocument/formatting` via `latexformat` or shell-out to `latexindent`; TeXLab bridge if a `texlab` binary exists on PATH (proxy advanced features to it).

### 4.3 `crates/synctex` — the published OSS crate

Why: No mature Rust `.synctex.gz` parser exists in 2026. Format is a text GZIP with a single-letter-prefix state-machine structure (see [jlaurens/synctex](https://github.com/jlaurens/synctex/blob/main/synctex_parser_readme.md)). Port is mechanical ~600 LOC.

```rust
// crates/synctex/src/lib.rs
pub struct SyncTeXData {
    pub inputs: BTreeMap<u32, PathBuf>,
    pub pages: BTreeMap<u32, Vec<HBox>>,
    pub magnification: u32,
    pub unit: u32,
    pub x_offset: i32,
    pub y_offset: i32,
}

pub fn parse(synctex_gz_path: &Path) -> Result<SyncTeXData, SyncTeXError> {
    let f = std::fs::File::open(synctex_gz_path)?;
    let gz = flate2::read::GzDecoder::new(f);
    let reader = std::io::BufReader::new(gz);
    parse_stream(reader)
}

pub fn backward_search(
    data: &SyncTeXData,
    page: u32,
    x: f64,
    y: f64,
) -> Result<SourceHit, SyncTeXError> {
    // nearest hbox on `page` to (x, y); convert coords from scaled points via unit/magnification
}

pub fn forward_search(
    data: &SyncTeXData,
    file: &str,
    line: u32,
) -> Result<PdfHit, SyncTeXError> {
    // all hboxes with matching file+line; pick page of first match
}
```

Open-source release: published to crates.io as `latex-ai-synctex` v0.1.0 at M3 end. MIT + Apache-2 dual-licensed.

### 4.4 `crates/visual` — PDF crop + render

```rust
// crates/visual/src/lib.rs
use pdfium_render::prelude::*;
use std::sync::Arc;

pub fn render_crop(
    pdf_path: &Path,
    page_idx: u32,
    x: f32, y: f32, w: f32, h: f32,
    dpi: u16,
) -> Result<Arc<[u8]>, VisualError> {
    let pdfium = Pdfium::new(Pdfium::bind_to_system()?);
    let doc = pdfium.load_pdf_from_file(&pdf_path, None)?;
    let page = doc.pages().get(page_idx)?;
    let render_cfg = PdfRenderConfig::new()
        .set_dpi(dpi)
        .set_target_rect(PdfRect::new_from_lryb(y, x, y + h, x + w));
    let bitmap = page.render_with_config(render_cfg)?.as_rgb();
    let mut buf = Vec::with_capacity(64 * 1024);
    bitmap.write_to_stream(&mut std::io::Cursor::new(&mut buf), 0)?;
    Ok(Arc::from(buf))
}
```

NB: final `pdfium-render` API verified at M3 pinning time. `set_target_rect` API may differ in exact name; treat signature as illustrative until then.

### 4.5 `crates/latex_ai_core` — use-case agnostic services

| Submodule | Why | Key signature |
|---|---|---|
| `storage` | Atomic writes + history + ETag + backup rotation | `save_with_etag_history(path, content, etag, history_kind) -> Result<SaveOutcome, StorageError>` |
| `security` | Workspace sandbox (Path canonicalize + extension allow-list + symlink cycle check) | `validate_path(workspace, path, AccessKind) -> Result<PathBuf, SecurityError>` |
| `ai` | Adapter around `genai-rs` (Gemini provider) with `EditPlanner` protocol | `EditPlanner::edits_stream(session, snapshot, intent) -> BoxStream<Result<Event, _>>` |
| `compile` | Shared types (`CompileRequest`, `CompileResult`, `ScratchPad`) used by `compiler` crate | `ScratchPad::create_for(session_id)` |
| `bundle` | Multi-file LaTeX dependency bundle via `walkdir` + cycle detection + workspace containment | `DocumentBundle::load_async(main_file) -> Result<DocumentBundle, BundleError>` |
| `document` | Domain: LaTeX `LatexEngine` (commands + regex FSM) + `parser` for AST | `LatexEngine::apply_command(doc, command) -> EditResult` |
| `commands` | Extensible `CommandRegistry`; manual registration (no `inventory` crate; CONTEXT.md §5 doesn't authorise speculative scaffolding) | `register_all(reg: &mut CommandRegistry)` called at boot |
| `models_registry` | Strong-typed `ModelInfo` with serde defaults | `available_models() -> &'static [ModelInfo]` |
| `metrics` | Atomic counters (`AtomicU64`) for prometheus text + JSON endpoints | `record_request`, `record_compile`, `record_agent` |
| `prompt_safety` | `sanitize_user_instruction` regex-set + length cap before sending to AI | `sanitize(intent) -> Result<String, PromptInjectionError>` |

### 4.6 `crates/latex_ai_server` — axum assembly

```rust
// crates/latex_ai_server/src/lib.rs
use axum::Router;
use std::sync::Arc;
use tower_http::{cors::CorsLayer, trace::TraceLayer, limit::RequestBodyLimitLayer};

#[derive(Clone)]
pub struct AppState {
    pub engine: Arc<compiler::Engine>,
    pub queue: Arc<compiler::CompilationQueue>,
    pub planner: Arc<core::ai::EditPlanner>,
    pub storage: Arc<core::storage::Store>,
    pub history_db: Arc<sqlx::SqlitePool>,
    pub config: Arc<Config>,
}

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .merge(routes::files::router(&state))
        .merge(routes::agent::router(&state))
        .merge(routes::compile::router(&state))
        .merge(routes::visual::router(&state))
        .merge(routes::history::router(&state))
        .merge(routes::health::router(&state))
        .merge(routes::lsp::router(&state))
        .merge(routes::metrics::router(&state))
        .layer(TraceLayer::new_for_http())
        .layer(RequestBodyLimitLayer::new(50 * 1024 * 1024))
        .layer(CorsLayer::new().allow_origin(state.config.cors_origins.clone()))
        .with_state(state)
}
```

### 4.7 `crates/latex_ai_tauri` — desktop shell

```rust
// crates/latex_ai_tauri/src/main.rs
fn main() {
    let app_state = latex_ai_server::init().expect("init");
    let router = latex_ai_server::build_router(app_state.clone());

    tauri::Builder::default()
        .plugin(tauri_plugin_axum::init(router))
        .run(tauri::generate_context!())
        .expect("error while running tauri app");
}
```

Tauri embeds the router; the SPA's `index.html` loads via `http://axum.localhost/` for non-streaming or `http://127.0.0.1:<port>/` for SSE (custom protocol doesn't support streaming per Tauri issue #1404).

---

## 5. Module Boundaries (Pillar 1 — Modularity)

Enforced via code review; boundary bullets (lint rules when `cargo-modules` matures):

1. **`latex_ai_server` cannot import `tectonic`, `tower_lsp_server`, `pdfium_render`, `flate2`, `nom`.** Only via core/compiler/lsp/visual crates.
2. **`latex_ai_core` cannot import `axum`, `tokio::process`, `tower-lsp-server`, `tauri`, `tectonic`, `pdfium-render`.** Pure services.
3. **`latex_ai_tauri` only imports `latex_ai_server` + `tauri` + `tauri-plugin-axum`.**
4. **`compiler` crate owns `tectonic`.** No other crate sees it.
5. **`lsp` crate owns `tower-lsp-server`.**
6. **`visual` crate owns `pdfium-render`.**
7. **`synctex` crate has zero knowledge of LaTeX.** Inputs/outputs `SyncTeXData` and search primitives only.
8. **No `#[allow(clippy::unwrap_used)]` outside `#[cfg(test)]`.** No `#[allow(clippy::pedantic)]` blanket-lifts.

---

## 6. Data Flows

### 6.1 Deterministic edit apply (manual)

```
1. Editor sends POST /api/apply { file_path, command, dry_run, if_match }.
2. files::apply_endpoint:
   a. validate_path(&workspace_root, &file_path, AccessKind::Write)
   b. tokio::fs::read(&path).await
   c. compute_etag(content) via blake3
   d. match cache.diff.get((content_hash, command.as_str())) → cached diff (moka)
   e. core::document::LatexEngine::apply_command(&doc, &command) → EditResult
   f. similar::utils::unified_diff for the diff blob
   g. (dry run) return ApplyResponse { diff }
   h. (write) core::storage::save_with_etag_history(path, new_text, etag, "manual-apply")
   i. return ApplyResponse { diff, backup_path, history_id }
```

### 6.2 AI plan stream (SSE)

```rust
async fn plan_stream(
    State(s): State<AppState>,
    Query(q): Query<PlanStreamQuery>,
    cancel: CancellationToken,
) -> Sse<impl Stream<Item = Result<Event, anyhow::Error>>> {
    let (tx, rx) = tokio::sync::mpsc::channel(32);

    s.planner.clone().spawn_plan_stream(
        s.planner.session_for(q.session_id),
        validate_path(&s.config.workspace_root, &q.file_path, AccessKind::Read)?,
        q.request_text,
        cancel,
        tx.clone(),
    );

    Sse::new(tokio_stream::wrappers::ReceiverStream::new(rx))
        .keep_alive(KeepAlive::default())
}
```

Generator yields events matching the §8.2 taxonomy. On `CancellationToken::cancelled`, the spawned task exits cleanly and drops its `tx` (that closes the receiver — SSE ends gracefully).

### 6.3 Agent healed loop

```rust
async fn agent_loop_stream(/*...*/) -> Sse<_> {
    let (tx, rx) = mpsc::channel(64);
    let planner = s.planner.clone();
    let engine = s.engine.clone();
    let snap = DocumentBundle::load_async(main_file).await?;

    tokio::task::spawn(async move {
        let mut last_error: Option<String> = None;
        for iter in 0..req.max_iterations {
            let intent = if let Some(err) = &last_error {
                format!("{}\n\nCompilation errors so far:\n{}", req.instruction, err)
            } else { req.instruction.clone() };

            let _ = tx.send(Event::default().event("thinking").data(json!({"iteration": iter}))).await;

            let edits = match planner.edits_for(snap.clone(), intent).await {
                Ok(e) => e, Err(e) => { let _ = tx.send(Event::default().event("error").data(json!({"message": format!("{e:#}")}))).await; return; }
            };

            if edits.is_empty() {
                let _ = tx.send(Event::default().event("error").data(json!({"message": "no edits proposed"}))).await;
                return;
            }

            let _ = tx.send(Event::default().event("proposal").data(json!({"edits": &edits, "iteration": iter}))).await;

            if !req.auto_apply {
                let _ = tx.send(Event::default().event("awaiting_approval").data(json!({}))).await;
                return;
            }

            let _ = tx.send(Event::default().event("applying").data(json!({}))).await;

            let scratch = engine.scratchpad_for(&scratch_session_id);
            scratch.apply_edits(&edits).await.ok();

            let cancel = CancellationToken::new();
            let compile = tokio::spawn({
                let cancel = cancel.clone();
                let engine = engine.clone();
                let req_clone = req.clone();
                async move { engine.compile_in_scratchpad(req_clone.clone(), cancel).await }
            });

            tokio::select! {
                _ = cancel_loop.cancelled() => {
                    compile.abort();
                    return;
                }
                r = compile => match r.unwrap() {
                    Ok(cr) if cr.success => {
                        s.storage.save_with_etag_history(/* user file with edits */, "agent-apply").await.ok();
                        let _ = tx.send(Event::default().event("done").data(json!({"success": true, "page_count": 1}))).await;
                        return;
                    }
                    Ok(cr) => {
                        let last_err = extract_error_summary(&cr);
                        let _ = tx.send(Event::default().event("compiled").data(json!({"success": false, "error_summary": last_err}))).await;
                        last_error = Some(last_err);
                    }
                    Err(e) => {
                        let _ = tx.send(Event::default().event("error").data(json!({"message": format!("{e:#}")}))).await;
                        return;
                    }
                }
            }
        }
        let _ = tx.send(Event::default().event("done").data(json!({"success": false, "message": "Max iterations reached"}))).await;
    });

    Sse::new(ReceiverStream::new(rx)).keep_alive(KeepAlive::default())
}
```

### 6.4 Visual fix

```
Editor.svelte → POST /api/visual-fix { file_path, page, x, y, w, h, intent }
  → router::visual::fix
  → synctex::backward_search(parse(synctex_path), page, x, y) → SourceHit
  → snippet = read file, expand ±30 lines to a Cow<str>
  → visual::render_crop(pdf_path, page, x, y, w, h, dpi=150) → Arc<[u8]>
  → planner.visual_fix(session, snippet, Arc<[u8]>, intent) → Vec<AgentEdit>
  → return ProposalResponse (NOT auto-apply — Pillar 3)
  → user accepts via /api/agent/apply-selected
```

### 6.5 Per-hunk accept

```rust
#[derive(Deserialize, Validate, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ApplySelectedRequest {
    #[garde(length(min = 1, max = 64))]
    pub edits: Vec<AgentEdit>,
    pub selected_hunk_ids: HashSet<HunkId>,
}

async fn apply_selected(
    s: State<AppState>,
    body: Json<ApplySelectedRequest>,
) -> Result<Json<AgentResponse>, ApiError> {
    body.0.validate().map_err(|e| ApiError::UnprocessableEntity(e.into()))?;

    let mut selected: Vec<&AgentEdit> = vec![];
    for e in body.edits.iter() {
        let hk = HunkId::from_search(&e.search_text());
        if body.selected_hunk_ids.contains(&hk) { selected.push(e); }
    }
    s.storage.atomic_apply_selected(path, &selected, "apply-selected").await?;
    Ok(Json(AgentResponse { success: true, results: vec![] }))
}
```

---

## 7. Database Schema

### 7.1 SQLite schema (`~/.latex-ai/history.db`) via sqlx migrations

```sql
-- migrations/0001_baseline.up.sql
CREATE TABLE history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    file_path TEXT NOT NULL,
    command TEXT NOT NULL,
    timestamp DATETIME DEFAULT CURRENT_TIMESTAMP,
    new_text TEXT NOT NULL,
    diff TEXT NOT NULL,
    success BOOLEAN DEFAULT 1,
    edit_type TEXT DEFAULT 'manual-save'
);

CREATE TABLE ai_cache (
    cache_key TEXT PRIMARY KEY,             -- "file_hash\x1fmodel\x1frequest_hash"
    plan_json TEXT NOT NULL,
    timestamp DATETIME DEFAULT CURRENT_TIMESTAMP,
    ttl_seconds INTEGER DEFAULT 3600
);
CREATE INDEX idx_ai_cache_timestamp ON ai_cache(timestamp);
```

```sql
-- migrations/0002_compile_queue.up.sql (M2)
CREATE TABLE compile_queue (
    job_id TEXT PRIMARY KEY,
    file_path TEXT NOT NULL,
    bundle_hash TEXT NOT NULL,
    options_json TEXT NOT NULL,
    status TEXT NOT NULL,
    progress REAL DEFAULT 0.0,
    pdf_blob_path TEXT,
    diagnostics_json TEXT,
    error TEXT,
    created_at REAL NOT NULL,
    started_at REAL,
    completed_at REAL
);
```

```sql
-- migrations/0003_compile_errors.up.sql (M5)
CREATE TABLE compile_errors (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    history_id INTEGER NOT NULL,
    iteration INTEGER NOT NULL,
    line INTEGER,
    file TEXT,
    message TEXT NOT NULL,
    error_type TEXT NOT NULL,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (history_id) REFERENCES history(id) ON DELETE CASCADE
);
CREATE INDEX idx_errors_history ON compile_errors(history_id);
```

### 7.2 Migration reversibility (P3 audit)

Every `0001.up.sql` has matching `0001.down.sql` dropping the tables in reverse dependency order. CI test routine:

```
sqlx migrate run up && sqlx migrate run down && sqlx migrate run up
```

CI fails the job if reversibility test does not pass on a clean DB.

### 7.3 Query ergonomics with sqlx compile-time checks

```rust
// core/storage/history.rs
pub async fn add_entry(
    pool: &Pool<Sqlite>,
    file_path: &str,
    command: &str,
    new_text: &str,
    diff: &str,
    edit_type: &str,
) -> Result<i64, sqlx::Error> {
    let row: (i64,) = sqlx::query_as(
        "INSERT INTO history (file_path, command, new_text, diff, edit_type) VALUES (?, ?, ?, ?, ?) RETURNING id"
    ).bind(file_path).bind(command).bind(new_text).bind(diff).bind(edit_type).fetch_one(pool).await?;
    Ok(row.0)
}
```

Compile-time-checked equivalent (preferred):

```rust
let id: i64 = sqlx::query_scalar!(
    "INSERT INTO history (file_path, command, new_text, diff, edit_type) VALUES (?, ?, ?, ?, ?) RETURNING id",
    file_path, command, new_text, diff, edit_type
).fetch_one(pool).await?;
```

`sqlx::query!` macro uses offline data via `cargo sqlx prepare --check` in CI.

---

## 8. API Design

### 8.1 Strict serde payload policy

All request/response types derive (no exceptions):

```rust
#[derive(serde::Deserialize, garde::Validate, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AgentEdit {
    #[garde(length(min = 1, max = 4096))]
    pub file_path: String,
    pub op: AgentOp,
    pub payload: Option<String>,
    pub text: Option<String>,
    pub start_line: Option<NonZeroU64>,
    pub end_line: Option<NonZeroU64>,
    pub line: Option<NonZeroU64>,
    pub insert_text: Option<String>,
    pub expected_etag: Option<String>,
}

#[derive(Deserialize, Serialize, Clone, Copy)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentOp {
    RewriteFile, ReplaceRange, Insert, ApplyUnifiedDiff,
    // NO apply_dmp_patch (CONTEXT §5 — drop ambiguity)
}
```

Error response envelope:

```rust
#[derive(Debug, serde::Serialize, thiserror::Error)]
#[serde(tag = "error", deny_unknown_fields)]
pub enum ApiError {
    #[error("etag mismatch")] EtagConflict,
    #[error("rate limited, retry after {retry_after_s}s")] RateLimited { retry_after_s: u32 },
    #[error("security: {0}")] InvalidPath(String),
    #[error("compile timeout")] CompileTimeout,
    #[error("VLM parse error: {0}")] VlmParseError(String),
    #[error("synctex miss on page {0} at ({1}, {2})")] SyncTeXMiss(u32, f64, f64),
    #[error("unauthorized")] Unauthorized,
    #[error("bad request: {0}")] BadRequest(String),
}
impl IntoResponse for ApiError { /* statuscode mapping */ }
```

Status code mapping:
- 400 BadRequest/InvalidPath; 401 Unauthorized; 403 Security-related; 404 FileNotFound; 409 EtagConflict; 413 PayloadTooLarge; 422 garde validation; 429 RateLimited; 500 Uncaught; 503 CircuitBreakerOpen or PlannerUnavailable.

### 8.2 SSE event taxonomy (locked)

| event | data shape | when |
|---|---|---|
| `progress` | `{ "message": _, "progress": 0.0..1.0 }` | UI status update |
| `step` | `{ "command": _, "description": _, "rationale": _ }` | AI plan step emitted |
| `proposal` | `{ "summary": _, "edit_count": _, "edits": [AgentEdit], "diffs": {file: diff} }` | Batch ready for review |
| `applying` | `{ "message": _ }` | Backend applying edits to scratchpad |
| `compiled` | `{ "success": bool, "page_count"?, "error_summary"? }` | Compile finished |
| `thinking` | `{ "message": _, "iteration": u8 }` | Mid-iteration placeholder |
| `done` | `{ "success": bool, "message"?, "diffs"? }` | Stream complete |
| `error` | `{ "message": _ }` | Stream errored |
| `awaiting_approval` | `{ "message": _ }` | paused in auto-apply=false mode |
| `keepalive` | `{}` | negotiated with axum `KeepAlive` |

Hard rule: endpoints emit only events from this set — CI regex check `r#event\("(?!progress|step|proposal|applying|compiled|thinking|done|error|awaiting_approval|keepalive)"#` fails the run.

### 8.3 Endpoint table (post-M5)

Same surface as before (mostly): files / agent / compile / visual / history / health / lsp / metrics routers. New endpoints:

```
POST /api/search                (M2 NEW) → SearchResponse { hits }   (nucleo, replaces fuse.js; interim POST {query,candidates} → GET ?q= once server owns the file list)
POST /api/visual-fix            (M3 NEW) → VisualFixResponse { snapshot, edits }
GET  /api/pdf/page              (M3 NEW) → image/png                 (pdfium full-page render, replaces PDF.js)
POST /api/agent/apply-selected  (M4 NEW) → AgentResponse
GET  /api/synctex/forward       (M15 NEW) → { build, stale, region, hits }   source line → PDF region (points)
GET  /api/synctex/backward      (M15 NEW) → { file, line, column, exact, stale }   PDF point → source line
```

### 8.4 WebSocket LSP bridge

```
WS /ws/lsp    (M2 + ticks at M5)
Frontend → JSON-RPC LSP messages; backend routes them into the in-binary
`tower-lsp-server` instance.
```

---

## 9. Infrastructure & Deployment

### 9.1 Dockerfile (single stage with tectonic)

```dockerfile
FROM rust:1.85-slim-bookworm AS builder
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev libsqlite3-dev libglib2.0-dev build-essential \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY frontend ./frontend
RUN cargo build --release -p latex_ai_server

FROM debian:trixie-slim
LABEL maintainer="LaTeX-AI"
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates libsqlite3-0 libglib2.0-0 \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /build/target/release/latex-ai /usr/local/bin/latex-ai

# Tectonic fetches bundles to ~/.cache/Tectonic on first run (~1GB global font cache)
ENV LATEX_AI_HOST=0.0.0.0 LATEX_AI_PORT=8000
EXPOSE 8000
HEALTHCHECK --interval=30s --start-period=5s --timeout=10s CMD ["curl", "-fsS", "http://localhost:8000/health"]

RUN useradd -m -u 1000 latexai && chown -R latexai:latexai /home/latexai
USER latexai
CMD ["latex-ai", "serve", "--port", "8000"]
```

Image size target: Tectonic embeds XeTeX (release binary ~200 MB compiled); final image will be bigger than ideal. M5 optimization: pre-fetch tectonic bundle cache into the image or split bundle layer.

### 9.2 docker-compose.yml

```yaml
services:
  latex-ai:
    build: .
    ports: ["8000:8000"]
    environment:
      GEMINI_API_KEY: ${GEMINI_API_KEY}
      LATEX_AI_HOST: 0.0.0.0
      LATEX_AI_PORT: 8000
      ENV: production
      ALLOWED_ORIGINS: http://localhost:8000
    volumes:
      - ./examples:/home/latexai/workspace
      - latex-ai-cache:/home/latexai/.cache/Tectonic
      - latex-ai-state:/home/latexai/.latex-ai
    restart: unless-stopped

volumes:
  latex-ai-cache:
  latex-ai-state:
```

### 9.3 Production considerations

| Concern | Solution |
|---|---|
| Tectonic first-run network fetch (~1 GB bundle) | Pre-fetch during docker build (`cargo run -p latex_ai_server --features tectonic-warmup`); document for users |
| NAT (remote box) | Caddy TLS terminator in front; bind axum to 127.0.0.1 only |
| Tauri binary distribution | `.deb`/`.dmg`/`.msi`/`.AppImage` installers per Tauri bundle |
| Crash recovery | Tectonic global mutex recovers on next compile; queue items recovered via `compile_queue` table (M5) |
| Secrets | `GEMINI_API_KEY` env or `~/.latex-ai/.env` (gitignored). `tracing-subscriber` filter strips key names |
| SyncTeX cache | `~/.latex-ai/cache/<content_hash>/{document.pdf, document.synctex.gz}` |
| CPU profile | `opt-level = "z"` + `lto = "fat"` + `codegen-units = 1` + `strip = true` + `panic = "abort"` in `[profile.release]` |

---

## 10. Frontend Architecture

### 10.1 Stack (kept JS, not rewritten)

The frontend stays JS — pivot is backend+binary-only.

**Frontend-language decision (LOCKED 2026-06-25).** The UI stays JS/Svelte; we do
**not** rewrite it in Rust (Dioxus/Leptos) nor go native (egui/iced). Criterion
applied: *adopt Rust only where it raises performance without raising complexity
or token cost.* A Rust frontend fails that test by construction — Tauri and
Dioxus both render through the same `wry` webview (no runtime perf gain), and
Monaco has no Rust equal so a JS island survives on every path. All-Rust UI =
same perf, more complexity, more tokens → rejected. Rust's perf wins live in the
backend (in-process Tectonic, no subprocess), which we already have. Where a
Rust component *does* pass the test, it lands as a backend endpoint feeding the
JS UI (see the two swaps below), not as a frontend rewrite.

| Layer | Pick | Why |
|---|---|---|
| Framework | Svelte 5 (runes API) | Existing. Per-PR incremental runes migration across M2–M4. |
| Build | Vite 8 | Existing 6 → bump to 8 (latest stable) |
| Editor | Monaco Editor | Existing. No Rust-native equal — the anchor that keeps a JS layer. |
| PDF viewer | **pdfium-render full viewer** (backend renders page PNGs) + bbox overlay canvas | NEW (M3). Replaces PDF.js: backend `pdfium-render` rasterizes whole pages to PNG (`GET /api/pdf/page`), JS shows them with zoom/pan. Same crate already used for the visual-fix crop. **Cost accepted:** loses PDF.js's text layer (in-PDF select/search) unless a text layer is added later. |
| Fuzzy search | **`nucleo` (Rust) via `GET /api/search`** | NEW. Replaces fuse.js. Command-palette/file queries hit a backend endpoint backed by `nucleo` (Helix's matcher). Local 127.0.0.1 → effectively instant; no WASM build pipeline. |
| HTTP client | native `fetch` | Replaces axios. Fewer deps, less code; webview ships `fetch`. |
| Tailwind | 4 via standalone CLI (no node) | Existing styling; standalone binary removes node from the *build*. |
| SSE parser | One consolidated `services/_stream.js#streamSSE` (replaces today's duplicated readers) | Lockdown |
| Test | Vitest + `@testing-library/svelte` | Existing; new integration tests per phase |
| `lib/api/client.js` | Build as part of M2 (replace missing module) | Single tiny `fetch` wrapper (not axios) |

**Rust-leaning swaps to schedule** (backend endpoints, not a UI rewrite):
- `axios` → native `fetch` (M2, folded into `client.js`).
- Tailwind node toolchain → standalone CLI binary (M2 build cleanup).
- `fuse.js` → `nucleo` behind `GET /api/search` (M2 — new `core` search module + route).
- `PDF.js` → `pdfium-render` full-page viewer behind `GET /api/pdf/page` (M3 — extends the visual-fix crop renderer).

### 10.2 Screens

Same as before — minimal change. NEW this round: `VisualFixOverlay.svelte` (M3) drawing bbox over a pdfium-rendered page PNG; `DiffView.svelte` hunk-level accept UI (already scaffolded in App.svelte, promoted to standalone in M4).

### 10.3 SSE contract lock

`services/_stream.js` becomes the single canonical SSE parser:

```js
// frontend/src/lib/api/_stream.js
export async function* streamSSE(response) {
  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  let buffer = '';
  let event = 'message';
  while (true) {
    const { done, value } = await reader.read();
    if (done) break;
    buffer += decoder.decode(value, { stream: true });
    const lines = buffer.split('\n');
    buffer = lines.pop();
    for (const line of lines) {
      if (line.startsWith('event: ')) event = line.slice(7).trim();
      else if (line.startsWith('data: ')) {
        const payload = line.slice(6);
        if (!payload.trim()) continue;
        yield { event, data: JSON.parse(payload) };
        event = 'message';
      }
    }
  }
}
```

---

## 11. Security & Compliance

### 11.1 Authentication

None for V1 (local-first, 127.0.0.1 binding). M5+ introduces an optional bearer token if remote-mount surfaces — no current requirement.

### 11.2 Workspace sandbox (Rust translation)

```rust
pub fn validate_path(workspace_root: &Path, requested: &str, kind: AccessKind)
    -> Result<PathBuf, SecurityError>
{
    if requested.contains('\0') { return Err(SecurityError::NullByte); }
    if Path::new(requested).is_absolute() && !Path::new(requested).starts_with(workspace_root) {
        return Err(SecurityError::AbsoluteOutsideWorkspace);
    }
    if requested.contains("..") { return Err(SecurityError::ParentDir); }
    let target = workspace_root.join(requested.strip_prefix("./").unwrap_or(requested));
    let abs = std::fs::canonicalize(&target).map_err(|_| SecurityError::NotFound)?;
    if !abs.starts_with(workspace_root) { return Err(SecurityError::EscapesWorkspace); }
    if let Ok(meta) = std::fs::symlink_metadata(&abs) {
        if meta.file_type().is_symlink() {
            let resolved = std::fs::canonicalize(&abs)?;
            if !resolved.starts_with(workspace_root) { return Err(SecurityError::SymlinkEscape); }
        }
    }
    if matches!(kind, AccessKind::Write) {
        let ext = abs.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        if !ALLOWED_WRITE_EXTENSIONS.contains(&ext.as_str()) {
            return Err(SecurityError::ForbiddenExtension(ext));
        }
    }
    Ok(abs)
}

const ALLOWED_WRITE_EXTENSIONS: &[&str] = &["tex", "bib", "sty", "cls", "tikz"];
const BLOCKED_EXTENSIONS: &[&str] = &["py", "sh", "exe", "env", "key", "pem", "cert"];
```

### 11.3 Compile sandbox

Tectonic with `--untrusted` runs with all dangerous features disabled (no shell-escape by construction). Strictly safer than the old pdflatex subprocess + rlimits approach.

### 11.4 Prompt-injection

```rust
const PROMPT_INJECTION_PATTERNS: &[&str] = &[
    r"ignore\s+previous", r"system\s*:", r"<\|im_start\|>", r"###\s*Instruction",
    r"<\|\s*system\s*\|>",
];

pub fn sanitize(instruction: &str, max_len: usize) -> Result<String, PromptInjectionError> {
    if instruction.is_empty() || !instruction.chars().any(|c| !c.is_whitespace()) {
        return Err(PromptInjectionError::Empty);
    }
    if instruction.len() > max_len { return Err(PromptInjectionError::TooLong); }
    let cleaned: String = instruction.chars()
        .filter(|c| !(*c as u32).is_control() || matches!(c, '\n' | '\t' | '\r')).collect();
    let lowered = cleaned.to_lowercase();
    for pat in PROMPT_INJECTION_PATTERNS {
        if let Ok(re) = regex::Regex::new(pat) {
            if re.is_match(&lowered) {
                tracing::warn!(intent = %instruction.chars().take(100).collect::<String>(), "blocked injection attempt");
                return Err(PromptInjectionError::Injection);
            }
        }
    }
    Ok(cleaned.trim().to_string())
}
```

Wired on every `intent` field in agent routes via `garde::Validate` rule.

### 11.5 Secrets

`GEMINI_API_KEY` via `figment`-managed Config. `tracing-subscriber` `EnvFilter` + custom `FieldFormatter` redacts known key names.

### 11.6 CORS

Dev: permissive (`http://localhost:5173`, `http://127.0.0.1:5173`).
Prod (`ENV=production`): **`ALLOWED_ORIGINS` required.** Startup fails immediately if missing. In Tauri shell, CORS not needed because SPA loads via `axum://localhost/` same-origin.

---

## 12. Phased Implementation Roadmap

Phases map 1:1 with `idea.md` §4. Each phase carries an **Audit Gate** (CONTEXT.md P3). Phases are sequential.

### M1 — Backend skeleton port + Python removal · ~3–5 days

**Goal**: Cargo workspace green; old Python `src/latex_ai/` deleted; Tectonic compiles `examples/sample.tex` in-process; axum serves `/health` alive.

**Deliverables**:
- [x] `Cargo.toml` workspace with members: `crates/{compiler,latex_ai_core,latex_ai_server}`. (synctex/visual/lsp at M3/M5, latex_ai_tauri at M5 — created when implemented, not scaffolded empty per rust.md I.15.)
- [x] `cargo deny init` + `.cargo-deny.toml` (advisories + license + bans config).
- [x] `crates/compiler/src/lib.rs`: `tectonic::latex_to_pdf` wrapper + global mutex + tiny in-memory compile cache (moka replaces at M2).
- [x] `crates/latex_ai_server/src/main.rs`: axum app with `health` route only + `TraceLayer` configured.
- [x] One integration test: `examples/sample.tex` → `latex-ai compile examples/sample.tex -o /tmp/sample.pdf` exits 0 and PDF bytes non-empty.
- [x] `clippy.toml` with `-D warnings -D clippy::pedantic -D clippy::unwrap_used -D clippy::expect_used` configured.
- [x] `.github/workflows/ci.yml`: cargo fmt/check/clippy/nextest/deny/audit/machete on push + PR.
- [x] Delete `src/latex_ai/` and `Makefile` (Python residue).
- [x] Update `README.md` from scratch with build/run instructions.

**Acceptance**:
- `cargo nextest run` green; `cargo clippy` green; `cargo deny check` clean.
- `./target/release/latex-ai compile examples/sample.tex` produces a valid PDF (page count ≥ 1).
- `./target/release/latex-ai serve --port 3000` boots; `curl http://localhost:3000/health` returns 200.

**Audit Gate (M1)**:
- Data Integrity: workspace compiles; no orphan `extern crate`; `cargo modules` confirms boundary rules.
- Logic Correctness: explicit walk of every error variant (`TectonicError`, `CompileError`, `StorageError`); no swallowing.
- Integration: Tauri shell crate exists but compiles without code (route assembly deferred to M2).
- Security: no `expect()`/`unwrap()` outside `#[cfg(test)]`. Grep `unwrap()\|expect()` outside `tests/` returns zero.

### M2 — Submit/Compile/Agent-Loop · ~5–7 days

**Goal**: agent-loop self-heals in ≤3 iterations; pillar 1 lands.

**Deliverables**:
- [x] `crates/compiler/queue.rs`: full `CompilationQueue` with Moka dedup, debounce, `Semaphore(cap=1)` sized to Tectonic's global mutex.
- [x] `crates/compiler/scratchpad.rs`: per-session `~/.latex-ai/scratch/<session_id>/` tree; `apply_edits(edits) -> Result<()>`; `cleanup()` on session drop.
- [x] `crates/latex_ai_core/ai.rs`: `EditPlanner` trait + `GeminiPlanner` impl using `genai_rs::Client` + `client.interaction().with_store_enabled().create().await` to retrieve `interaction_id` for next turn.
- [x] `router::agent::plan_stream` + `router::agent::agent_loop_stream` SSE handlers emitting the locked taxonomy.
- [x] Cancellation-deconnect tower middleware (invention): the stream's `Drop` triggers a bound `CancellationToken`.
- [x] `core::prompt_safety::sanitize_user_instruction` wired on every `intent` field.
- [x] `crates/latex_ai_core/search.rs` + `GET /api/search` (nucleo matcher) — replaces frontend fuse.js (§10.1).
- [x] Frontend swaps: `lib/api/client.js` on native `fetch` (drop axios ✅, drop fuse.js ✅); command palette/file search call `/api/search` ✅. **Tailwind standalone CLI: consciously skipped** — Vite already requires node for the SPA build, so the swap removes no dependency (§15.1).
- [x] Tests: induce `\begin{tabular}` without `\end`; assert 3 iterations; final `done(success=False)` with compile errors; induce trivial `\frac{`-typo; assert 1–2 iterations ending in `done(success=True)`.
- [x] Tests: disconnect mid-iteration; assert compile-future cancelled within 200 ms.

**Acceptance**:
- Send "make table 3-col" via curl; SSE stream shows `thinking → proposal → applying → compiled → done`.
- Induce broken latent; agent self-corrects in ≤3 iter; emits `done` with compilable diff.
- Disconnect mid-iter: scratchpad compile cancelled within 200ms; history untouched.

**Audit Gate (M2)**:
- Data Integrity: chat history lives in `genai_rs::Interaction` keyed via `previous_interaction_id`; no `HashMap<session_id, Vec<Content>>` in our crate.
- Logic Correctness: state-machine total — `queued → compiling → compiled(done) | compiled(failed, retry<max) | error`; no silent swallow.
- Integration: SSE event taxonomy consistent; frontend `streamSSE` parser unit-tested.
- Security: prompt-injection sanitizer applied before every LLM call; cancellation `cargo tarpaulin` covers the path.

### M3 — Reverse SyncTeX + VLM · ~4–6 days (M2 prerequisite)

**Goal**: visual bbox → bounded edit proposal grounded in PDF coordinates.

**Deliverables**:
- [x] `crates/synctex/`: full parser, `parse()`, `backward_search()`, `forward_search()` APIs — **hand-written recursive reader, not nom** (§15.1): the grammar is line-oriented with single-char prefixes, so a parser-combinator dependency bought nothing. Adds the scaled-point -> PDF-point conversion the format actually requires.
- [x] `crates/visual/`: `pdfium_render` clipped rasterize (crop) **and** full-page render; one integration test on a real `.synctex.gz` fixture.
- [x] `GET /api/pdf/page?n=&dpi=` → `image/png` full-page render — replaces PDF.js viewer (§10.1). Frontend PDF view becomes an image viewer (zoom/pan) over these PNGs.
- [x] `router::visual::fix` endpoint: validate bounds → SyncTeX backward_search → ±30-line snippet → PNG crop → GeminiPlanner `visual_fix()` → return `VisualFixResponse { edits }` (no auto-apply).
- [x] `crates/latex_ai_core/ai.rs::visual_fix`: uses `genai_rs::Interaction::with_response_format(ResponseFormat::Json)` for VLM call.
- [x] `frontend/src/lib/api/client.js#repairVisualCrop`: SSE-style POST with bbox JSON; `VisualFixOverlay.svelte`: pdfium page PNG (`GET /api/pdf/page`) + canvas overlay for drag select.
- [x] Tests: bbox over overflowing image in `examples/` fixture; intent `"shrink this image to fit column"`; assert `edits` contains exactly one `replace_range` touching the `\includegraphics` line range only.
- [x] Tests: missing synctex file → 404 with proper envelope; out-of-bounds bbox → 422.

**Audit Gate (M3)**:
- Data Integrity: `backward_search` return shape always present; null only when SyncTeX miss (404).
- Logic Correctness: VLM JSON parse strict; 422 on bad shape + WARN log.
- Integration: `POST /api/visual-fix` reuses `apply_agent_edits`; cache invalidates on bundle-hash drift.
- Security: PNG crop bytes capped ≤512px long edge; bbox validated; workspace path re-validated from SyncTeX return.

### M4 — Per-hunk accept UX · ~2–4 days

**Goal**: user accepts/rejects per hunk atomically; hunk_id stable across requeries.

**Deliverables**:
- [x] `router::agent::apply_selected`: atomic single-file lock + single etag check + single history entry tagged `apply-selected`.
- [x] `HunkId = blake3::hash(search_text).to_hex()[..16]` type (Newtype).
- [x] `DiffView.svelte` promoted to standalone component with hunk-level accept/reject affordances.
- [x] Per-PR Svelte 5 runes migration in touched files (no forced repo-wide migration) — `DiffView.svelte` and `VisualFixOverlay.svelte` use runes; the other 14 components remain Svelte 4 syntax by design, migrating as they are touched.
- [x] Tests: same plan twice → identical hunk_id set; reject-all → file unchanged; accept 1/3 → only that range modified.

**Audit Gate (M4)**:
- Data Integrity: identical hunks across two prompts collapse; reject stale `hunk_id` with 409 when underlying `expected_etag` drifts.
- Logic Correctness: subset-apply is atomic per-file lock; aborts on any selection failure.
- Integration: one history row per `apply-selected` regardless of hunk count.
- Security: per-file etag check precedes hunk application; `AgentEdit` schema validates `start_line ≤ end_line`.

### M5 — Hardening + Tauri desktop shell + alpha release · ~4–6 days

**Goal**: production-grade local-first release; Tauri binary ≤ 25 MB; metrics emit; persisted queue; history auditable.

**Deliverables**:
- [x] `crates/latex_ai_tauri/src/main.rs`: Tauri shell wrapping `latex_ai_server::build_router`, opening SPA in a webview.
- [x] SSE routes via `http://127.0.0.1:<port>` — **`tauri-plugin-axum` evaluated and rejected** (§15.1): its own README confirms custom protocols cannot stream (#1404), so the TCP listener is needed regardless and the plugin would add a protocol, an npm package and per-route URL branching for no reduction in surface. The port is ephemeral rather than a fixed 3000.
- [x] `core::metrics::record_*` wired as axum middleware (`from_fn` rather than trace-span capture — §15.1), exposed at `GET /api/metrics` as JSON and Prometheus text.
- [x] Compile jobs persisted to `compile_queue` on submit and closed out on completion; startup marks anything still `running` as `abandoned`.
- [x] `compile_errors` realigned to §7.1 with the `history_id` FK + cascade (corrective migration, §15.1); the agent loop records each healed iteration's failure against the resulting history row.
- [x] `ENV=production` + missing/blank `ALLOWED_ORIGINS` → startup refuses with an explicit message (returned as an error rather than a panic, so the binary reports it cleanly).
- [x] Cargo release profile: `opt-level="z"`, `lto="fat"`, `codegen-units=1`, `strip=true`, `panic="abort"`.
- [x] `cargo deny` policies: advisories, license allow-list, sources — green, with every unavoidable ignore carrying a documented reason.
- [x] Tauri bundlers configured for `.deb`/`.appimage`/`.dmg`/`.msi` in `tauri.conf.json`; `.deb` built and measured (**6.5 MB bundle, 15.4 MB binary — inside the <=25 MB target**). Only the Linux targets are buildable here: Tauri does not cross-compile bundles.
- [x] `cargo tarpaulin --workspace --fail-under-80` green — **86.73% (2602/3000 lines)**, measured 2026-08-02 under `scripts/capped` (tarpaulin rebuilds instrumented and re-runs every test under ptrace; unbounded it OOMed this machine). CI runs the same measurement in a `coverage` job.
- [x] `README.md` documents desktop build/install and production deployment.
- [x] Part II of `rust.md` removed (1276 -> 382 lines), with a note recording why.

**Acceptance**:
- Desktop bundle launches, opens `examples/sample.tex`, compiles, all major flows work.
- `/api/metrics` returns non-zero counts after a fixture request.
- `docker compose up` boots healthy without manual intervention.
- Tauri bundle ≤ 25 MB on disk for at least one platform.

**Audit Gate (M5)**:
- Data Integrity: every FK on `compile_errors(history_id)` exists; reversible migration verified.
- Logic Correctness: every router path instruments exactly one `record_*` call.
- Integration: spend counter increments BEFORE the LLM call returns.
- Security: prod denies on missing `ALLOWED_ORIGINS`.

### Out-of-scope for V1

- Multi-user / multi-tenant.
- Multi-worker horizontal scale (would need out-of-process queue).
- Real-time collaborative editing (CRDT).
- Generic IDE features beyond MVP.
- Cloud-hosted primary deployment.
- `texlab` external binary proxy path — our in-binary LSP overlaps enough for V1.
- AST-grounded error localisation (Bibby AI insight) — needs `pylatexenc`-equivalent Rust LaTeX AST walker (future `latex-ai-ast` crate).
- Overleaf sync, publishing infra.
- LuaLaTeX support (Tectonic is XeTeX only).

---

## 13. Key Design Decisions Summary

| Decision | Choice | Rationale |
|---|---|---|
| Language | Rust 2024 edition (1.85+) | `genai-rs`, tectonic, axum, sqlx, tauri are state-of-the-art; borrow checker + zero-cost abstractions |
| Web framework | axum 0.8 + tokio + tower | Canonical Rust web stack. Native SSE. |
| Desktop | Tauri 2 + `tauri-plugin-axum` | Same codebase, ≤25 MB binary, embedded axum router w/ axum://localhost for non-streaming + TCP loopback for SSE |
| LaTeX engine | `tectonic::latex_to_pdf` in-process | **Killer USP.** No subprocess. No TeX Live install. XeTeX-based. Auto-fetches missing packages. |
| SyncTeX parser | NEW `latex-ai-synctex` crate we publish | No mature Rust crate; we port. OSS contribution. |
| LSP | `tower-lsp-server` in-binary | We become the LaTeX LSP. Eliminates `texlab` external binary dep. |
| LLM SDK | `genai-rs` (Interactions API) | Gemini's `previous_interaction_id` makes the server own chat history; we never store it. |
| VLM | `gemini-2.5-flash-image` + `with_response_format(JSON)` | Forced JSON; strict serde decode. |
| Database | sqlx + SQLite local-first | Compile-time-checked queries; whole class of bugs eliminated. |
| Cache | `moka::future::Cache` + expiry trait | State-of-the-art concurrent cache. |
| Rate limit | `tower-governor` (GCRA) + `governor` | Tower middleware, per-key on `X-Session-ID`. |
| Hashing | `blake3` 16-hex prefix for ETag + `hunk_id` + `cache_key` | 10× faster than sha256; SIMD. |
| Errors | `thiserror` for libs, `anyhow` for app | Production 2026 consensus. |
| Validation | `serde` strict + `garde::Validate` | 2026 successor to `validator`. |
| Async cancellation | `tokio-util::CancellationToken` | True aborts in O(1) at await points. |
| Tracing | `tracing` + `tracing-subscriber` | Modern structured logging; replaces `structlog`. |
| Config | `figment` (env + toml) | No split-brain. |
| Frontend | Svelte 5 + Vite 8 (kept JS) | Pivot is backend-only. |
| Container | `rust:1.85-slim-bookworm` + `debian:trixie-slim` | Tectonic bundle is the only heavy external asset. |
| Compile dedup | Bundle blake3 hash keyed `moka` cache | Multi-file diff; cache invalidation trivial. |
| Tectonic global mutex | `OnceLock<Mutex<()>>` at `crates/compiler` | Tectonic admits not-thread-safe; we serialize inside our queue. |
| Hunk identity | `blake3::hash(search_text)` 16-hex prefix | Stable across replans; collision-resistant. |
| Cancellation propagation | `tokio::select!` against `CancellationToken`; `JoinHandle::abort()` | Compile stays fast to abort; AI stream task-aborted via token. |
| Plugin loader | Deleted (CONTEXT.md P4 — speculative) | Re-evaluate at M6+ with confirmed need. |
| Tauri SSE | TCP loopback for SSE; custom protocol for everything else | Per Tauri issue #1404. |
| Binary size | opt-level z + fat LTO + strip + panic=abort ≤ 25 MB | Edge peer shipped 18 MB at 114 routes; we target 25 MB with Tauri + Tectonic inclusion. |

---

## 14. Reference Architecture (peer state-of-practice, Rust-relevant)

| Peer project | Insight borrowed | Where it lands |
|---|---|---|
| `TexGuardian` (Python) | Annotation-anchored review; unified-diff approval pattern | M4 per-hunk apply UX |
| `PaperFit` (Python) | Vision-in-loop: render → diagnose → fix → re-render | M2 self-healing loop; M3 visual-fix |
| `scholia`/`texwatch` (Rust LSP bridge to MCP) | SyncTeX bbox → comment queue, structured compile oracle | M3 /api/visual-fix; M2 CompileError JSON contract |
| `latex-engineer` (skill plugin) | latexmk orchestrator; tlmgr self-install | **Superseded** — Tectonic makes both obsolete |
| `latex-agentic` (Claude Code plugin) | Specialised subagents (error-doctor, typesetter) | Idea only — out of V1 scope |
| `BibbyAI` (Python) | AST-grounded error localisation | M6+ candidate; needs Rust LaTeX AST walker (future `latex-ai-ast` crate) |
| `agentic-latex-writer` (Rust+TS) | Local-first default; external CLI/LLM pluggable | Confirms our Tauri local-first binary-only architecture |
| `ferroleaf` (Rust+Iced+pdfium-render) | Rust-native LaTeX editor proof-of-concept | Inspiration for `pdfium-render` selection |
| `XandSuite` (Rust+Tauri v2+React) | Local-first AI desktop suite pattern with embedded axum | Architectural template shape for `latex_ai_tauri` |

---

## 15. Compliance with CONTEXT.md — Per-Pillar Compliance Map

| CONTEXT.md pillar | Where in masterplan |
|---|---|
| §2 Modularity | §3 workspace crates + strict dependency direction; §4 component separation; §13 plugin_loader deletion |
| §2 Anti-Patterns | §5 module boundary rules enforce no business logic in infrastructure |
| §3 Completeness | §12 phase gate acceptance blocks; no milestone merges without `cargo nextest run` + clippy pedantic + deny clean |
| §3 Stop Rule | §12 per-milestone explicit "if any audit fails, revert and stop" |
| §4 Audit Protocol | §7 reversibility tests; §12 per-phase Audit Gate blocks; §13 rationale column |
| §4.1 Data Integrity | §7.1 schema audit findings; §12 M2 audit for no HashMap<session_id, Vec<Content>> |
| §4.2 Logic Correctness | §4.1 tectonic global mutex; §6.3 total state machine claim |
| §4.3 Integration | §8.2 SSE event taxonomy hard rule; §8.1 error envelope |
| §4.4 Security | §11 sandbox, CORS, secrets, prompt-injection; §12 M2 sanitize-on-incoming |
| §5 Intentionality | §11.6 deletion candidates; §13 rationale column |
| §6 Workflow | §12 phase gate discipline |
| §6 Re-read | Masterplan + CONTEXT.md + idea.md + rust.md via `opencode.json` at session start |

---

## 15.1 Implemented Deviations (recorded 2026-08-02, at M2 completion)

Where shipped code intentionally differs from this document. The code is
authoritative; these entries explain why, so the delta is a decision and not
drift.

| Area | Plan said | Shipped | Why |
|---|---|---|---|
| LLM SDK | `genai-rs` 0.7 with Gemini **Interactions API**, `previous_interaction_id` so the provider owns chat history | `genai` 0.1 one-shot chat against **Groq** (`llama-3.3-70b-versatile`), no stored history | Provider switched to Groq (2026-08-02, user decision). Groq has no Interactions equivalent, so there is no server-side history to inherit. The agent loop folds compile errors into the next instruction instead, which keeps the §12-M2 audit rule ("no `HashMap<session_id, Vec<Content>>` in our crate") satisfied — we store no history at all. Revisit if a provider with server-side threads returns. |
| `EditPlanner` | `-> BoxStream<Result<Event, _>>` | `-> PlanFuture` (boxed future, whole batch) | Object safety was the actual requirement (it is what lets tests inject a scripted planner). Token-level streaming buys nothing while the response must be parsed as one JSON array. |
| Edit application | `patch(1)` for `apply_unified_diff` | Pure-Rust hunk application in `core::apply` | Shelling out contradicted the no-subprocess principle (§1) and made results depend on the host's `patch`. Also relocates hunks when line numbers drift. |
| Config | `figment` layered TOML + env | Plain env vars | Nothing yet needs layered files; `figment` would be speculative scaffolding (CONTEXT.md §5). Revisit when a config file is genuinely required. |
| Validation | `garde::Validate` on payloads | `serde(deny_unknown_fields)` + explicit checks | Current payloads have a handful of fields; a derive framework is not yet earned. |
| Rate limiting | `tower-governor` per `X-Session-ID` | Not implemented | Single-user loopback binding; no demonstrated need (CONTEXT.md §5 "defensive over-engineering"). |
| Tailwind | standalone CLI to "drop node from the build" | Kept as an npm dev dependency | Incoherent as stated: Vite already requires Node for the SPA build, so the swap removes no dependency. Consciously skipped. |
| Frontend fuzzy search | `nucleo` behind `GET /api/search` | Done, plus `POST /api/search` for caller-supplied candidates | The command palette ranks its own static command list; the server does not own that list. |
| `AgentEdit.hunk_id` | server-computed, `skip_deserializing` | plain optional field | With `deny_unknown_fields`, a skipped field made the client's echo of a proposal un-deserializable on apply-selected. |
| Scratchpad | `~/.latex-ai/scratch/<session>` | Same, `LATEX_AI_HOME`-overridable, LRU-10 + explicit cleanup | Matches Q3; the override exists so tests and containers do not write to `$HOME`. |
| `SyncTeX` emission (M3) | `synctex: true` in `Tectonic.toml` | `ProcessingSessionBuilder::synctex(true)` + `into_file_data()` | `tectonic::latex_to_pdf` cannot emit `SyncTeX` at all. Driving the session builder returns the PDF **and** the `.synctex.gz` in memory — no config file, no output directory, no temp files. |
| `SyncTeX` parser (M3) | `nom` combinators, ~600 LOC | hand-written line reader + unit conversion, ~210 LOC | The format is line-oriented with single-char record prefixes; `nom` would add a dependency and obscure it. The real work was the missing sp->bp conversion, verified against a known value (a 1in margin is 4736287sp = 72bp). |
| `SyncTeX` input name (M3) | source path recorded in the map | Tectonic always records the primary input as `texput` | Not configurable via `tex_input_name`. Single-document hits therefore resolve to the file the caller compiled; a real name (multi-file, once `bundle` lands) is re-validated against the workspace before being read. |
| VLM call (M3) | `gemini-2.5-flash-image` | separate `LATEX_AI_VISION_MODEL`, image sent as a real `ContentPart::Image` + `ChatResponseFormat::JsonMode`; honest text-only fallback when unset | Follows the provider switch (row 1). Verified live 2026-08-02: **Groq serves no vision model** (`llama-3.2-*-vision-preview`, which `genai` still lists, are retired), and a text-only model rejects image parts with a 400. So vision is its own configurable model — any provider `genai` routes — and with none configured the route degrades to a source-only fix and logs that it did, rather than pasting base64 into a prompt no model can read. |
| VLM response shape (M3) | strict JSON array | array, `{"edits": [...]}`, **or a lone edit object** | OpenAI-style JSON mode forces a top-level *object*, so a bare array is impossible for those providers regardless of the prompt. Groq returns a single edit object for a single-change request; the decoder accepts all three and still 422s on anything else. |
| Hunk preview (M4) | not specified | new `POST /api/agent/preview` | Per-hunk accept needs to show what each hunk *does*, and computing that in the browser would drift from the Rust apply semantics (first-occurrence replace, line splice, diff relocation). The endpoint runs each hunk through the same `core::apply` the write uses, so a test can assert preview and apply produce byte-identical diffs — and does. |
| Hunk selection scope (M4) | "per hunk" | one hunk == one `AgentEdit` | `hunk_id` is already defined as blake3 of the anchor text (§13), so the edit *is* the unit of review. Sub-splitting an edit's diff would produce hunks with no stable id, which is what the milestone's stability requirement forbids. |
| `tauri-plugin-axum` (M5) | register the plugin, custom protocol for non-streaming + TCP for SSE | **rejected**; loopback TCP only | The plugin's value is removing the bound port. Its own README confirms custom protocols cannot stream (wry #1404), so our SSE-heavy API needs the TCP listener regardless — adopting it would mean a protocol *and* a port, plus an npm package and per-route URL branching in the SPA, for no reduction in surface. The port is instead bound as ephemeral (`:0`) rather than a guessable fixed 3000. |
| Desktop API base (M5) | not specified | injected via `initialization_script` | Found while evaluating the above: the SPA is served from `tauri://localhost`, so its relative `/api/...` resolved against that origin and never reached the server — the desktop shell could not have worked. The window is now built in code so the real loopback base is injected before any page script runs. |
| Desktop async runtime (M5) | not specified | `tauri::async_runtime`, not `#[tokio::main]` | Found by running the built bundle: the process stayed alive with a single thread and **nothing listening**. `tauri::run()` installs its own runtime and blocks; a server spawned with `tokio::spawn` on a separate `#[tokio::main]` runtime is orphaned the moment it does. Setup now runs under `tauri::async_runtime::block_on` and the server under `tauri::async_runtime::spawn`. Both this and the row above were invisible to `cargo check` — only launching the artefact exposed them. |
| Tectonic C libraries (M5) | vendored, as shipped | `tectonic/external-harfbuzz` | The desktop shell segfaulted on launch inside `hb_face_t::reference_table` on a corrupted function pointer. Cause, from the gdb stack: Tectonic statically links its **vendored** harfbuzz while webkit2gtk dynamically links the **system** one, and two copies in one process let the linker resolve symbols across implementations. Linking the system copy fixes it. **Caveat:** Tectonic also vendors freetype, graphite2 and icu but exposes an `external-*` feature only for harfbuzz, so the same collision class could resurface in an unexercised path. Stable across everything tested; not proven safe in general. |
| Desktop `devUrl` (M5) | not specified | removed | A debug build follows `devUrl`, so a manually-run debug binary showed "localhost refused to connect" unless a Vite server happened to be up. Both build types now load the bundled SPA; browser hot-reload stays in the documented `npm run dev` + `latex-ai serve` workflow. |
| Heavy-command limits (M5) | not specified | `scripts/capped` + `.config/nextest.toml` + `.cargo/config.toml` | `cargo tarpaulin` rebuilds the workspace instrumented *and* re-runs every test under ptrace; it exhausted a 14 GB machine twice during this milestone. The nextest and cargo caps cannot bound a tool that spawns its own build, so heavy commands now run in a systemd scope with `MemoryMax` — an overrun kills that cgroup rather than the session. |
| Metrics (M5) | `tower-http::trace` instrumentation with custom span capture | `axum::middleware::from_fn` + atomic counters | Counting requests is not a tracing concern; a middleware that owns the counter is both simpler and exactly-once by construction. Exposed as JSON and Prometheus text from one endpoint. |
| `compile_errors` schema (M5) | `history_id` FK per §7.1 | same, via a corrective migration | The table had shipped with `session_id` and no FK. Nothing had ever written it, so the migration recreates it rather than attempting a data-preserving alter. `PRAGMA foreign_keys = ON` is set at startup — SQLite ignores foreign keys otherwise, which would have made the constraint decorative. |
| Container bind address (M5) | `serve` binds `127.0.0.1` | `--host` / `LATEX_AI_HOST`, still defaulting to `127.0.0.1` | Found by reading the container gate rather than trusting it: Docker forwards a published port to the container's **interface** address, so a loopback-only bind is unreachable from the host — while the in-container `HEALTHCHECK` (which curls its own localhost) still reports healthy. `docker compose up` would have gone green with a dead API. The bind address is now explicit, loopback stays the default so the no-auth design (§11.1) is unchanged for direct runs, the container passes `0.0.0.0`, and a non-loopback bind logs a warning. Access control in the container comes from the host-side `127.0.0.1:3000:3000` mapping. |
| Container toolchain (M5) | `rust:1-slim-bookworm` builder, headers only | same base + `g++`, and `SQLX_OFFLINE=true` | The image had never built. `rust:*-slim` ships the Rust toolchain but no C/C++ one, and `tectonic_engine_xetex` compiles vendored C **and** C++ sources — its build script aborts with `failed to find tool "c++"` after ~390 crates. The Dockerfile installed the `-dev` headers those sources need but not the compiler that consumes them. `SQLX_OFFLINE=true` is set for the same reason CI sets it: the `query_scalar!` macro is checked at compile time and there is no database during an image build. |
| Container volume ownership (M5) | not specified | mount points created and chowned in the image | The daemon died on its first compile with `Permission denied` making the Tectonic cache root. Docker initialises a named volume from the image path, and when that path does not exist it creates it root-owned — so the unprivileged runtime user could not write it. Tectonic's cache-root creation is an `unwrap` and the release profile sets `panic="abort"`, which turned one failed request into the whole process aborting. Creating the three mount points as `latexai` before `USER` fixes it. Incidentally the abort proved the M5 crash-recovery path: the restarted process logged `recovered abandoned compile jobs count=1`. |
| Image build caching (M5) | not specified | BuildKit cache mounts for the cargo registry and `target/` | `COPY . .` precedes `cargo build`, so any source change invalidated all ~460 dependency crates — about an hour, mostly Tectonic's vendored C/C++ engines, plus minutes committing a multi-GB `target/` into the builder layer. Cache mounts survive that invalidation and are not part of the image, so the binary is copied out inside the same `RUN`. A `container` CI job now builds and boots the image against the GitHub Actions cache: the image had been broken for four milestones precisely because nothing ever built it. |
| Container runtime (M5) | `debian:trixie-slim` runtime, image target <= 200 MB | `FROM scratch` + the binary's `ldd` closure | The distro runtime cost 162 MB unpacked to carry a program whose real closure is ~60 MB: a userland the daemon never calls, plus the entire curl dependency tree, which existed only to run `HEALTHCHECK`. The runtime now holds the binary and exactly the shared objects `ldd` resolves for it, derived at build time rather than hand-listed, so a new transitive dependency cannot be missed. **62.8 MB unpacked / 24.9 MB compressed / 55 files.** Two things `ldd` cannot tell you, both of which fail only on a cold cache and so would survive a warm-volume smoke test: glibc `dlopen`s its NSS modules at call time (without them `getaddrinfo` fails and the Tectonic bundle never downloads), and OpenSSL's compiled-in CA directory does not exist on scratch (so TLS verification fails). Verified against wiped volumes: 42 MB / 270 files fetched, `compiles_succeeded=1`, `restarts=0`. Trade-off: no shell in the image, so `docker compose exec ... sh` is gone. |
| `HEALTHCHECK` client (M5) | `curl -fsS .../health` | `latex-ai healthcheck` | curl appeared nowhere in the binary's link closure — it was in the image purely for the health probe, and it dragged in a dependency tree far larger than the check. The binary now probes its own `/health` with one HTTP/1.1 request over a raw tokio socket, adding no crate. |
| Image size measurement (M5) | image target <= 200 MB | measured unpacked, via `docker history` or a flattened export | `docker images` on the containerd store sums the unpacked layers **and** the compressed blob, so it reported 222 MB for an image that was 162 MB unpacked and 56 MB to pull — and this milestone was briefly recorded as missing the <=200 MB target on that inflated figure. It never did. The CI size guard flattens the filesystem instead, so it means the same thing on any Docker version. |
| In-binary LSP (§4.2, §8.4) | `crates/lsp` + `WS /ws/lsp` via `tower-lsp-server` | **Never built; dropped.** | Nothing consumed it: the editor's LSP client was gated behind an env flag that no build set, and the endpoint never existed. Completion, outline and diagnostics are planned as a client-side scanner over the project plus compile diagnostics (plan M14), which needs no second protocol. |
| Paper tone-map (M6) | not in the plan | `crates/visual/src/paper.rs`: lightness compression in OKLab on the rendered page, applied to `/api/pdf/page` (`paper`, `warmth`) | Added after M5 (commits `f93aac1`, `1053602`). Dark UIs normally `filter: invert()` the PDF, which turns red warning boxes cyan; compressing lightness while leaving hue alone keeps colour meaning. |
| Save preconditions (M7) | `If-Match` optional | `If-Match` **required** on `POST /api/file` (428 when absent); `Precondition::{Etag, CreateNew}` in storage | The optional header made every blind overwrite legal, which is how concurrent edits were silently lost. A request that cannot name the version it is replacing is refused. |
| apply-selected etag (M7) | per-edit `expected_etag` | request-level `expected_etag`, required; per-edit etags still honoured | Only edits that happened to carry an etag were checked, so a `rewrite_file` without one overwrote concurrent changes. |
| Atomic write + history order (M7) | rename, then insert the history row | history row inserted in a transaction, file renamed, then commit (old bytes restored if the commit fails); uuid-named temp file in the target's directory | The old `with_extension("tmp")` made `a.tex` and `a.bib` share `a.tmp`, and a DB failure after the rename returned 500 for a file that had changed. |
| Read denylist (M7) | extension allow-list for writes only | `security::is_denied`: `.env*`, key/cert extensions, `.git`/`.ssh`/`.aws` dirs, `id_*` — enforced for every path the API validates, and filtered from the file listing | The sandbox kept paths inside the workspace but allowed reading `.env` there, and the agent sends file contents to a third-party model. |
| Host header check (M7) | not specified | requests whose `Host` is not loopback / an `ALLOWED_ORIGINS` host / `ALLOWED_HOSTS` get 403 | DNS rebinding: a hostile page can resolve its own name to 127.0.0.1 and reach an unauthenticated local server as a same-origin site, where CORS does not apply. |
| Tauri CORS (M7) | same-origin via `axum://` | dev/desktop default now includes `tauri://localhost` and `http(s)://tauri.localhost` | With the plugin rejected (row above), the SPA at `tauri://localhost` calls the loopback server cross-origin; the default origins did not include it. Production still requires explicit `ALLOWED_ORIGINS`. |
| Config file (M8) | `figment` provider chain, immutable `Arc<Config>` (rust.md I.12) | `toml` + serde, hand-layered: defaults < `~/.latex-ai/config.toml` < env; hot-swappable `ArcSwap<Snapshot>` with a per-request snapshot | Three fields do not justify figment's dependency tree; layering is ~20 lines and fully tested. Hot-swap is needed so the desktop app can accept an API key without a restart (rust.md I.12 amended to match). |
| API key storage (M8) | `GROQ_API_KEY` env only | env, else a `0600` `~/.latex-ai/secrets.toml` written through the Settings dialog; write-only API (`PUT/DELETE /api/config/api-key`), never returned | A GUI user cannot be asked to export env vars. A plain file, not the OS keyring, keeps Linux/headless/Docker behaviour identical. |
| History location (M8) | `<workspace>/.latex-ai.db` | `~/.latex-ai/projects/<id>/history.db`, id = blake3(canonical root)[..16]; legacy file imported once with `VACUUM INTO` and left in place | No database or WAL files inside the user's project. `VACUUM INTO` (not a file copy) because the legacy DB ran in WAL mode. |
| Reversible migrations (M8) | up-only `*.sql` | `*.up.sql` (byte-identical to the originals, so applied databases still validate) + `*.down.sql`; `tests/migrations.rs` proves up/down/up and pins the original checksums | CONTEXT.md §3. |
| `foreign_keys` pragma (M8) | one `PRAGMA` after pool creation | set in the connect options | The pragma is per-connection; the one-off form enabled it on a single pooled connection only. |
| AI unavailable (M8) | stream opens, then an `error` event | `503 unavailable` before any stream opens when no key is configured | Clients should not have to scan a 200 stream for a configuration problem. |
| Hunk identity (M9) | blake3 of the search text only | blake3 over file path, op, `text`, `payload`, `insert_text` and the line numbers, length-prefixed | The old id was `hash("")` for every insertion and line-range edit, so all of them shared one id, and two different replacements of one anchor were indistinguishable: selecting one hunk selected them all. `expected_etag` and the echoed `hunk_id` are excluded. |
| Batch semantics (M9) | edits applied one after another | all edits resolved to byte ranges against the original text, overlaps rejected, then spliced | Line numbers now always mean "the file the planner saw", and a batch gives one result in any order. Several edits with the same `text` take successive occurrences. `rewrite_file` must be alone for its file. |
| File format (M9) | `lines()` + join with `\n` | BOM, CRLF/LF and trailing newline preserved | The old split/join rewrote every line ending of a CRLF file and dropped a BOM when line 1 was replaced. |
| Policy lint (M9) | not specified | `core::policy`: text an AI edit *adds* may not name files outside the project from input-family commands, and may not use `\openout`, `\write18`, `\catcode`; lines already in the file are exempt | Tectonic cannot be sandboxed from inside (absolute reads are by design), so the model is stopped from writing the dangerous forms. Enforced in the plan/agent/visual-fix/preview routes and in `Store::atomic_apply_selected`. A guard against a model going wrong, not a sandbox against a hostile project author. |
| In-process engine (§1, M10) | Tectonic in-process, `Mutex`-serialised, cancel = future drop | Self-exec worker: the binary re-runs itself as `__compile-worker`; parent kills and reaps it on cancel/timeout; two compiles may run at once (one process each, so no shared lock) | `\def\a{\a}\a` wedged Tectonic's global lock forever and cancel only dropped our mutex. This is the one place the "no subprocess" principle yields; it is our own binary, with a scrubbed environment (allow-listed variables, empty `PATH` so `biber` cannot run). `worker::run_if_requested()` is the first statement of both `main`s. Release builds use `panic = "unwind"` so a panic cannot kill the server. |
| Compile timeout (M10) | none | `[compile] timeout_secs` (default 60, 5-600) in `config.toml`, read per compile | An unbounded compile is a hang. |
| Multi-file compile (M10) | one file copied to a scratchpad | `primary_input_path` on the real project; main file chosen by: explicit setting, `% !TEX root`, the active file if it has `\documentclass`, the only/including root; otherwise `409 main_file_ambiguous` with the candidates | Includes, bibliographies and images resolve against the real tree. The old path read only the active file, so chapters could not compile. |
| Diagnostics (M10) | scrape the engine's status text | `.log` and `.blg` parsed into `{severity, kind, file, line, message, context}`; golden fixtures are real Tectonic logs | Tectonic keeps the log even when a run fails. Errors inside a package are attributed to the project file without a line number rather than a wrong one. |
| PDF location and cache (M10) | `<source>.pdf` and `.synctex.gz` beside the source | Builds in `~/.latex-ai/projects/<id>/builds/<build id>/`, build id = fingerprint of every project file + engine version; `GET /api/builds/{id}`, `/pdf` (Export), page URLs by `?build=`; latest 12 kept; failures cached, timeouts/crashes not | Nothing is written into the project; an unchanged project is a free recompile; a page URL names one compile, so a stale page image is impossible. |
| SyncTeX input names (M10) | primary input is always `texput` | with a file on disk Tectonic records **absolute paths**; `synctex::resolve_input_name` maps them into the project and treats anything outside (or empty/`texput`) as the main file | Found in the M10 spike. Makes chapter hits resolve to the chapter. |
| Engine file access (M10) | "`--untrusted` disables dangerous features" | `do_not_write_output_files` (an absolute `\openout` lands in memory, tested); denylisted files `hide()`-n from `\input` (textual match only); AI-written `\input`/`\openout` forms refused by the M9 lint. **Not** achievable: stopping a user's own `\input{/abs/path}` — Tectonic hard-codes absolute reads on. | The remaining read exposure is the project author's own LaTeX, not the model's. |
| Review mode (M11) | `thinking -> proposal -> awaiting_approval`, unverified | Verify first: the loop runs on a working copy and compiles it; `step` events for attempts, then one `compiled`, `proposal` (`verified: true`, `proposal_id`) and `awaiting_approval`. The proposal is the **net change as independent per-hunk edits against the original files** (`core::apply::edits_from_diff`), not the iterations' own edits | The iterations' edits build on each other (attempt 2's line numbers refer to attempt 1's output) so they cannot be applied individually to the user's file. The net change can, hunk by hunk. A property test (random old/new text) pins that applying the hunks reproduces the new text; it also caught that `similar`'s op index fields are not reliable, so positions are tracked by walking the script. |
| Run lifecycle (M11) | `error` could end a stream with no `done` | `EventSink` enforces exactly one `done` per stream: failures are `error` then `done`; a run that ends any other way (a panic) is closed with `error` + `done` on drop. Events are a typed `SseEvent` enum | Clients can rely on one terminal event. |
| Agent working copy (M11) | the main file copied afresh each iteration | `ScratchProject`: the whole project mirrored once per run (sources copied, other files symlinked), edits accumulate, removed on drop (success, cancel, panic) | Fixes the loop discarding its own earlier edits and compiling a single file without its `\input`s. |
| Failed attempts (M11) | only compile errors fed back | edits refused by the lint, edits that do not apply, compile errors (with file:line), and timeouts all count as a failed iteration and are fed back to the planner; the prompt shows files with line numbers (`PlanRequest`) | One rule for every failure instead of some aborting the run. |
| Multi-file apply (M11) | one file per apply | `Store::apply_file_set`: every `ETag` checked first, then rows + files in one transaction, files restored on any failure; used by auto-apply and by `apply-selected` for a proposal spanning files (needs `proposal_id`) | A half-applied multi-file change would not compile. |
| Stale proposals (M11) | 409 only | the server keeps each proposal's base (`ProposalRegistry`, 32 entries, 1 h); `apply-selected` with `proposal_id` + `rebase: true` moves line-numbered hunks onto the changed file when every hunk's lines are unchanged (`core::apply::rebase_edits`), else 409. Unknown/expired id: 404 | Autosave while reviewing used to make every proposal unusable. |
| Visual-fix staleness (M11) | none | refused with 409 when the project changed since the build the box was drawn on (the build id is the project fingerprint); the response carries the build id and the hit file's `etag` | SyncTeX geometry from an old PDF maps to the wrong lines. Deviation: the box is still in pixels at a dpi, not in PDF points (that moves with the viewer rewrite in M15). |
| Frontend state (M12) | one `svelte/store` per concern, controllers and services between UI and API | Runes modules `lib/state/{documents,compile,project,ui}.svelte.js` + `lib/commands.js`; one `DocumentStore` owning a text model, ETag and save queue **per open file** | The shared single buffer let a save write file A's text to file B's path, discarded undo on every tab switch, and was overwritten by a 5 s poll. Dirty is "alternative version id differs from the saved one", so undoing back to the saved text is clean. Autosave is marked (`autosave: true`) so the server folds a burst into one history row. |
| Shell (M12) | stacked editor/PDF with dead dividers, dead shortcuts | `svelte-splitpanes` (sidebar \| editor \| PDF \| assistant, Problems/Log below), persisted sizes; one command registry whose single key matcher serves both the window and the editor (capture phase, so Monaco cannot swallow Ctrl+Enter); F1 lists every command | Shortcuts had four sources of truth and most did nothing. |
| PDF viewer (M12) | one page image at a time, no zoom | continuous scroll, pages loaded as they near the viewport, zoom and fit-width, every URL naming the build | Server-side pdfium PNGs are kept (no PDF.js). Dark-theme page toning and the visual-fix overlay are carried over. |
| External edits (M12) | 5 s poll that overwrote the buffer | on window focus: a clean file reloads quietly, a dirty one raises a conflict dialog ("keep mine" / "load from disk" / "decide later") | A poll cannot tell an external edit from the user's own typing. |
| File management (M13) | the tree was read-only | `POST /api/files/create`, `POST /api/files/rename`, `DELETE /api/file` (needs `If-Match`), `POST /api/upload` (raw body, 25 MB cap, type allowlist), `GET /api/raw`; every new name passes `security::resolve_new_path` | Creating a file is the one place a name comes from the user rather than from disk, so it gets its own check: no `..`, absolute, hidden, reserved or denylisted names, and the nearest existing ancestor must resolve inside the workspace (a symlinked folder cannot lead out). Rename and upload never replace (409). Delete is conditional like a save and keeps the removed text in history. Folders exist only through the files in them (no empty-folder create), and deleting one deletes its files one by one. |
| Main file (M13) | a browser `localStorage` preference | `GET`/`PUT /api/project` persists it in the per-project `project.toml`; compile resolution uses it when the request names no main file | It was invisible to the desktop app and to the agent. A chosen file that has since been removed is ignored, not an error. |
| Project switching (M13) | the project was whatever directory the process started in | Desktop: `pick_project` opens a native folder dialog, remembers the choice in `<home>/last_project` and **restarts** into it; a launcher start in `/` or `$HOME` opens a private starter folder instead of indexing the disk | A restart reuses the one startup path rather than adding an untested hot-swap of the server, database and build store (this supersedes the plan's D8 "hot-swap" for projects; config still hot-swaps). The GUI path is not exercised by automated tests: only the folder-choice rules and the frontend hooks are. |
| LaTeX editing (M14) | Monaco registered a `latex` id with no grammar, so the editor was plain text | Monarch grammars for LaTeX and BibTeX (`lib/latex/monarch.js`); one comment-aware scanner (`scan.js`, `bibtex.js`) feeds the outline, completion and the project index; completion for `\cite`, `\ref`, `\input`/`\include`, `\includegraphics`, `\bibliography`/`\addbibresource`, `\usepackage`, `\begin`/`\end` and commands (with project macros); an Outline panel in the sidebar; compile diagnostics as editor markers | Monaco ships no TeX support. Completion is context-based (`contextAt`), not parsed: it recognises constructs whose syntax is unambiguous and offers nothing elsewhere. Files that are not open are fetched once and refreshed on focus or when the tree changes; open files are read live. Markers come from the last compile and are dropped for lines that no longer exist. The editor now loads `editor.all` (the contributions: suggest widget, snippets, comment toggle, find); the bare `editor.api` import used since M12 had none of them. Added afterwards: Ctrl+click/F12 go-to-definition for labels, citations, macros and files across the project (a own resolver rather than Monaco's definition provider, which cannot target a file that has no open model); hover for the same, including a warning when a `\\ref` or `\\cite` points at nothing; spell check with the English Hunspell dictionary (`nspell` + `dictionary-en`, fetched on first use, ~0.6 MB; prose only, English only; quick fixes and a personal dictionary kept in the browser). Not done: a symbol palette, other languages. |
| SyncTeX both ways (M15) | `synctex/src/search.rs` existed but only the visual-fix route used it | `GET /api/synctex/forward` and `/backward`; Ctrl+Alt+J ("Find line") flashes the region for the cursor line, double-click on the PDF opens the source behind it | Coordinates are PDF points, the unit SyncTeX answers in, so nothing depends on zoom or DPI. Both answers carry `stale` (the project changed since the build) instead of refusing: the mapping is right for the PDF being looked at, and the UI says the line may have moved. Forward search returns one merged `region` (a point node has no height, and flashing a hairline helps nobody). Backward search refines the tightest line box to the nearest leaf on its baseline, because a line box records the line where its *paragraph ended*, not the text's own line. Parsed maps are cached (4, keyed by map path). |
| Visual-fix box (M15) | pixels of an image rendered at a stated DPI | PDF points, drawn in a layer on each page of the scrolling viewer | A box in pixels meant something only at the zoom and DPI it was drawn at; points do not. The crop sent to the model is rasterized at a fixed 200 DPI. The old single-page overlay (and its pixel conversion) is removed. |
| PDF viewer (M15) | restart showed an empty pane until the next compile | Ctrl+wheel / pinch zoom; the last build is restored on reload (`GET /api/builds/{id}` now also reports `page_count`) | The build store is already content-addressed and immutable, so showing yesterday's result costs one request. |
| libpdfium packaging (M15) | present only where a human installed it | Searched in `LATEX_AI_PDFIUM_DIR`, folders the app registers (the desktop shell registers its bundled resources), beside the executable, then the system. The Docker image downloads the pinned build (SHA-256 verified) and sets the variable; the Tauri bundle ships `resources/pdfium/` (filled by `scripts/fetch-pdfium.sh`). `latex-ai doctor` binds it and reports where from; CI runs it in the image | Both shipped artefacts previously had no PDF preview. This supersedes the plan's `render.pdfium_path` config key: an environment variable and a search order are enough, and a config field would need a restart-time reload nobody asked for. The desktop bundle path is unverified here (no bundling run); CI checks the shell compiles and the image's `doctor`. |
| Not done in M12 | | The AI chat, settings and history components still use their `svelte/store` state and legacy syntax; they move with their rewrite in M16. `svelte-splitpanes` needs the `$app/environment` alias, so it stays. `GET /api/tree` and `GET /api/builds/{id}/log` were added; `/api/agent/preview` now judges each hunk against its own file. | Honest scope: the plan listed these under M12, but their UI is rewritten in M16 anyway and migrating them twice would be churn. |

## 16. Open Questions

| # | Question | Default | Owner |
|---|---|---|---|
| Q1 | VLM model pin | `gemini-2.5-flash-image` | M3 start |
| Q2 | LuaLaTeX | n/a — Tectonic is XeTeX only | locked V1 |
| Q3 | Scratchpad lifecycle | persist at `~/.latex-ai/scratch/<session>/`; cleanup on session-close event + LRU-10 cap | M2 |
| Q4 | Hunk identity | blake3 prefix16 over file, op, anchor, replacement and payload (was: search text only — changed in M9, see §15.1) | locked M9 |
| Q5 | Tectonic bundle strategy | default `cosmic` bundle at first run; investigate `Tectonic.toml` minimal bundle for Tauri shipping | M5 |
| Q6 | Tauri SSE costream | TCP loopback for SSE; custom protocol for everything else | locked M2 |
| Q7 | `latex-ai-synctex` crate publish | publish to crates.io at M3 end | locked |
| Q8 | `tower-lsp` maturity | community fork stable; if upstream matures by M3, swap | tracked |
| Q9 | Tectonic async API | sync in 0.16; use `spawn_blocking` + tokio select; refactor if future release offers async | tracked |
| Q10 | Auth surface | None for V1 (local-first 127.0.0.1 binding) | locked |
| Q11 | Optional local LLM (Ollama) for fully-offline mode | Cloud Groq only for V1 (see §15.1 — the Gemini/Interactions rationale below is superseded; because we now keep no chat history at all, an Ollama backend no longer conflicts with the M2 audit rule and is a smaller change than originally assessed). A provider abstraction allowing a local model (Ollama) fits the local-first pillar, but conflicts with the genai-rs design where Gemini's `previous_interaction_id` makes the server own chat history — Ollama would force us to manage history ourselves (the `HashMap<session, Vec<Content>>` §12-M2 audit forbids). Defer; revisit only with a concrete offline requirement. (Inspiration: interviewstreet/hiring-agent dual Ollama/Gemini backend.) | M6+ |

---

*Document version: 2.0 (Rust Edition)*
*Date: 2026-06-24*
*Authors: synthesized from `idea.md`, `CONTEXT.md`, `rust.md`, existing repo audit, and peer-state-of-practice scan (TexGuardian, PaperFit, scholia, ferroleaf, XandSuite, r/tauri 2026-03 thread, genai-rs 0.7 docs, tectonic docs).*
*Status: M1–M5 complete; M6 (paper tone-map) shipped; M7 (data-integrity and access-control hotfixes) and M8 (config, bootstrap, per-project data, reversible migrations) M9 (edit engine) M10 (compiler worker, multi-file, diagnostics, builds) M11 (verify-first agent loop, proposals) M12 (frontend foundation), M13 (projects and files), M14 (LaTeX editing) and M15 (SyncTeX, PDF viewer, libpdfium packaging) implemented — see §15.1 for recorded deviations and §12 for the milestone record.*
*Re-read this doc + `CONTEXT.md` + `idea.md` + `rust.md` at the start of every implementation session.*