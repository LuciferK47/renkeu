# LaTeX-AI — Master Architecture Document (Rust Edition)

> **Purpose**: Implement the Autonomous Document Engine described in `idea.md` as a Rust-native codebase where Tectonic runs as an in-process LaTeX engine (no subprocess, no TeX Live install on host), axum serves both as standalone daemon and embedded inside a Tauri desktop shell (single binary, ≤25 MB), and `genai-rs` owns chat history via Gemini's Interactions API (we don't manage state).
> **Date**: 2026-06-24 (Rust pivot — supersedes the Python masterplan)
> **Status**: Master reference. Supersedes README/GUIDE/incomplete architectural comments in the existing repo when those conflict. `CONTEXT.md` is the binding law; `idea.md` is the product pitch; `rust.md` is the binding coding guideline + migration spec; this doc is the engineering execution plan and reads all three as inputs.
> **Reference sources**:
> - `CONTEXT.md` — four pillars (Modularity / Completeness / Auditability / Intentionality), workflow, red flags
> - `idea.md` — three-pillar pitch, M1–M5 milestones, locked Rust libraries, audit gates, 18 contracts
> - `rust.md` — Rust coding guidelines (Part I) + existing-codebase→Rust port spec (Part II)
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
| **Container build** | `rust:1.85-slim-bookworm` builder + `debian:trixie-slim` runtime | — | Tectonic ships musl-static binaries ~10 MB; our Docker image target: ≤ 200 MB release (vs current 4 GB via full TeX Live). |
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
2. No subprocess: cancellation is a tokio future drop. No leaked pdflatex children.

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
- [ ] `Cargo.toml` workspace with members: `crates/{compiler,latex_ai_core,latex_ai_server}`. (synctex/visual/lsp at M3/M5, latex_ai_tauri at M5 — created when implemented, not scaffolded empty per rust.md I.15.)
- [ ] `cargo deny init` + `.cargo-deny.toml` (advisories + license + bans config).
- [ ] `crates/compiler/src/lib.rs`: `tectonic::latex_to_pdf` wrapper + global mutex + tiny in-memory compile cache (moka replaces at M2).
- [ ] `crates/latex_ai_server/src/main.rs`: axum app with `health` route only + `TraceLayer` configured.
- [ ] One integration test: `examples/sample.tex` → `latex-ai compile examples/sample.tex -o /tmp/sample.pdf` exits 0 and PDF bytes non-empty.
- [ ] `clippy.toml` with `-D warnings -D clippy::pedantic -D clippy::unwrap_used -D clippy::expect_used` configured.
- [ ] `.github/workflows/ci.yml`: cargo fmt/check/clippy/nextest/deny/audit/machete on push + PR.
- [ ] Delete `src/latex_ai/` and `Makefile` (Python residue).
- [ ] Update `README.md` from scratch with build/run instructions.

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
- [ ] `crates/compiler/queue.rs`: full `CompilationQueue` with Moka dedup, debounce, `Semaphore(cap=1)` sized to Tectonic's global mutex.
- [ ] `crates/compiler/scratchpad.rs`: per-session `~/.latex-ai/scratch/<session_id>/` tree; `apply_edits(edits) -> Result<()>`; `cleanup()` on session drop.
- [ ] `crates/latex_ai_core/ai.rs`: `EditPlanner` trait + `GeminiPlanner` impl using `genai_rs::Client` + `client.interaction().with_store_enabled().create().await` to retrieve `interaction_id` for next turn.
- [ ] `router::agent::plan_stream` + `router::agent::agent_loop_stream` SSE handlers emitting the locked taxonomy.
- [ ] Cancellation-deconnect tower middleware (invention): the stream's `Drop` triggers a bound `CancellationToken`.
- [ ] `core::prompt_safety::sanitize_user_instruction` wired on every `intent` field.
- [ ] `crates/latex_ai_core/search.rs` + `GET /api/search` (nucleo matcher) — replaces frontend fuse.js (§10.1).
- [ ] Frontend swaps: `lib/api/client.js` on native `fetch` (drop axios); Tailwind via standalone CLI (drop node build dep); command palette/file search call `/api/search`.
- [ ] Tests: induce `\begin{tabular}` without `\end`; assert 3 iterations; final `done(success=False)` with compile errors; induce trivial `\frac{`-typo; assert 1–2 iterations ending in `done(success=True)`.
- [ ] Tests: disconnect mid-iteration; assert compile-future cancelled within 200 ms.

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
- [ ] `crates/synctex/`: full nom parser (~600 LOC); `parse()`, `backward_search()`, `forward_search()` APIs.
- [ ] `crates/visual/`: `pdfium_render` clipped rasterize (crop) **and** full-page render; one integration test on a real `.synctex.gz` fixture.
- [ ] `GET /api/pdf/page?n=&dpi=` → `image/png` full-page render — replaces PDF.js viewer (§10.1). Frontend PDF view becomes an image viewer (zoom/pan) over these PNGs.
- [ ] `router::visual::fix` endpoint: validate bounds → SyncTeX backward_search → ±30-line snippet → PNG crop → GeminiPlanner `visual_fix()` → return `VisualFixResponse { edits }` (no auto-apply).
- [ ] `crates/latex_ai_core/ai.rs::visual_fix`: uses `genai_rs::Interaction::with_response_format(ResponseFormat::Json)` for VLM call.
- [ ] `frontend/src/lib/api/client.js#repairVisualCrop`: SSE-style POST with bbox JSON; `VisualFixOverlay.svelte`: pdfium page PNG (`GET /api/pdf/page`) + canvas overlay for drag select.
- [ ] Tests: bbox over overflowing image in `examples/` fixture; intent `"shrink this image to fit column"`; assert `edits` contains exactly one `replace_range` touching the `\includegraphics` line range only.
- [ ] Tests: missing synctex file → 404 with proper envelope; out-of-bounds bbox → 422.

**Audit Gate (M3)**:
- Data Integrity: `backward_search` return shape always present; null only when SyncTeX miss (404).
- Logic Correctness: VLM JSON parse strict; 422 on bad shape + WARN log.
- Integration: `POST /api/visual-fix` reuses `apply_agent_edits`; cache invalidates on bundle-hash drift.
- Security: PNG crop bytes capped ≤512px long edge; bbox validated; workspace path re-validated from SyncTeX return.

### M4 — Per-hunk accept UX · ~2–4 days

**Goal**: user accepts/rejects per hunk atomically; hunk_id stable across requeries.

**Deliverables**:
- [ ] `router::agent::apply_selected`: atomic single-file lock + single etag check + single history entry tagged `apply-selected`.
- [ ] `HunkId = blake3::hash(search_text).to_hex()[..16]` type (Newtype).
- [ ] `DiffView.svelte` promoted to standalone component with hunk-level accept/reject affordances.
- [ ] Per-PR Svelte 5 runes migration in touched files (no forced repo-wide migration).
- [ ] Tests: same plan twice → identical hunk_id set; reject-all → file unchanged; accept 1/3 → only that range modified.

**Audit Gate (M4)**:
- Data Integrity: identical hunks across two prompts collapse; reject stale `hunk_id` with 409 when underlying `expected_etag` drifts.
- Logic Correctness: subset-apply is atomic per-file lock; aborts on any selection failure.
- Integration: one history row per `apply-selected` regardless of hunk count.
- Security: per-file etag check precedes hunk application; `AgentEdit` schema validates `start_line ≤ end_line`.

### M5 — Hardening + Tauri desktop shell + alpha release · ~4–6 days

**Goal**: production-grade local-first release; Tauri binary ≤ 25 MB; metrics emit; persisted queue; history auditable.

**Deliverables**:
- [ ] `crates/latex_ai_tauri/src/main.rs`: Tauri shell wrapping `latex_ai_server::build_router`, opening SPA in a webview.
- [ ] `tauri-plugin-axum` registered with our router; SSE routes via `http://127.0.0.1:<port>` (Tauri custom protocol streaming limitation per issue #1404).
- [ ] Wire `core::metrics::record_*` as axum middleware dependency (`tower-http::trace` instrumentation with custom span name capture).
- [ ] Persist `CompileQueue.active_jobs` to `compile_queue` table on `submit`; recover on startup (mark in-flight as `completed` with `error=abandoned`).
- [ ] Add `compile_errors` table migrations + `compiler::CompilationError::for_history(history_id, iter, errors)`.
- [ ] Tighten CORS: `ENV=production` + missing `ALLOWED_ORIGINS` → startup panic with explicit message.
- [ ] Cargo release profile: `opt-level="z"`, `lto="fat"`, `codegen-units=1`, `strip=true`, `panic="abort"`.
- [ ] `cargo deny` policies: RUSTSEC advisories, license whitelist (MIT, Apache-2.0, BSD-3, MPL-2.0).
- [ ] Tauri bundlers configured for `.deb`/`.dmg`/`.msi`/`.AppImage` via `tauri.conf.json`.
- [ ] `cargo tarpaulin --workspace --fail-under-80` green.
- [ ] Update `README.md` final with desktop install instructions for supported OS.
- [ ] Remove Part II of `rust.md` (the port spec) per the user's directive.

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

## 16. Open Questions

| # | Question | Default | Owner |
|---|---|---|---|
| Q1 | VLM model pin | `gemini-2.5-flash-image` | M3 start |
| Q2 | LuaLaTeX | n/a — Tectonic is XeTeX only | locked V1 |
| Q3 | Scratchpad lifecycle | persist at `~/.latex-ai/scratch/<session>/`; cleanup on session-close event + LRU-10 cap | M2 |
| Q4 | Hunk identity | blake3 search-text prefix16 | locked M4 |
| Q5 | Tectonic bundle strategy | default `cosmic` bundle at first run; investigate `Tectonic.toml` minimal bundle for Tauri shipping | M5 |
| Q6 | Tauri SSE costream | TCP loopback for SSE; custom protocol for everything else | locked M2 |
| Q7 | `latex-ai-synctex` crate publish | publish to crates.io at M3 end | locked |
| Q8 | `tower-lsp` maturity | community fork stable; if upstream matures by M3, swap | tracked |
| Q9 | Tectonic async API | sync in 0.16; use `spawn_blocking` + tokio select; refactor if future release offers async | tracked |
| Q10 | Auth surface | None for V1 (local-first 127.0.0.1 binding) | locked |
| Q11 | Optional local LLM (Ollama) for fully-offline mode | Cloud Gemini only for V1. A provider abstraction allowing a local model (Ollama) fits the local-first pillar, but conflicts with the genai-rs design where Gemini's `previous_interaction_id` makes the server own chat history — Ollama would force us to manage history ourselves (the `HashMap<session, Vec<Content>>` §12-M2 audit forbids). Defer; revisit only with a concrete offline requirement. (Inspiration: interviewstreet/hiring-agent dual Ollama/Gemini backend.) | M6+ |

---

*Document version: 2.0 (Rust Edition)*
*Date: 2026-06-24*
*Authors: synthesized from `idea.md`, `CONTEXT.md`, `rust.md`, existing repo audit, and peer-state-of-practice scan (TexGuardian, PaperFit, scholia, ferroleaf, XandSuite, r/tauri 2026-03 thread, genai-rs 0.7 docs, tectonic docs).*
*Status: M1 prerequisite. M2–M5 sequential with audit gates. See §12.*
*Re-read this doc + `CONTEXT.md` + `idea.md` + `rust.md` at the start of every implementation session.*