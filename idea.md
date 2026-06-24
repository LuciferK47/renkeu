# The Autonomous LaTeX Engine — Idea Doc

> Source of truth for completing the LaTeX-AI codebase. Keep this doc minimal. Every line should drive a concrete engineering decision.

> **Governing law:** `CONTEXT.md` (repo root, loaded by opencode.json). Its four pillars — Modularity, Completeness, Auditability, Intentionality — are binding on every line below. Any conflict between this doc and `CONTEXT.md` resolves in favor of `CONTEXT.md`; refactor this doc, not the constraints.

---

## 0. Four-Pillar Compliance (binds every milestone below)

### 0.1 Modularity — Document Pipeline as platform, LaTeX as adapter
Treat the codebase as a **Document Pipeline Engine** with LaTeX as one adapter. Core infrastructure must remain business/use-case agnostic:

- `pdf_compiler.py`, `document_bundle.py`, `synctex_parser.py`, `compilation_queue.py` — generic compiler/dependency-graph/source-map/queue primitives. No LaTeX-specific business rules beyond "drive pdflatex with these flags" (which itself should be config, see §6).
- `services/ai.py`, `ai_planner.py` — generic Edit Planner abstraction. Gemini is one provider behind a `Provider` interface (strat pluggable per M5+); model id collated by `models_registry.py`, not hard-coded inside routers.
- `api/agent.py` — orchestrates an `EditPlanner` over `EditBatch` requests. Must not reference LaTeX semantics (e.g., no `\begin{document}` parsing here).
- Where LaTeX *is* the domain (preamble parsing, `set margins`, etc.) it lives in `commands/` and `engine.py` — those are the swappable strategy layer. Keep them out of `services/`.
- Schema discipline: `history` table is generic `(file_path, command, new_text, diff, edit_type)`. Adding LaTeX-specific columns violates this pillar; instead use `edit_type` discriminator + a side-table joined by FK.
- Naming: `EditStep`, `EditPlan`, `AgentEdit` — domain-neutral. Do not introduce `LatexStep`, `TexFix`, `CompileHealStep` etc.

### 0.2 Completeness — Stop Rule overrides milestones
No milestone below ships until each item of the CONTEXT.md §3 Completion Checklist passes verifiably. A milestone that cannot satisfy all of them is **reverted** and re-queued, not "marked done with caveats". This means:
- No `apply_*` endpoint that lacks a 409 conflict path test.
- No SSE endpoint without a disconnect-cancellation test.
- No pdflatex call without a timeout-failure test.
- No AI edit path without an ETag-mismatch test.

### 0.3 Auditability — Each milestone emits an Audit Record
Each milestone section below contains a **Audit Gate** block. Before marked done, the implementer must execute the four audits from CONTEXT.md §4 (Data Integrity, Logic Correctness, Integration, Security) and record findings inline. Empty findings = "no findings". No milestone is done without an Audit Gate entry.

### 0.4 Intentionality — Pre-implementation questions
Before any new file/function in this codebase, answer (in the commit message or PR description):
1. Why? (specific requirement from this doc or a tracking issue)
2. What calls it / what it calls / what it reads-writes?
3. What breaks if removed?
4. Simplest viable form?

Speculative scaffolding prohibited. The plugin loader (currently unused) and `resource_management.py` (broken, unreferenced) are **deletion candidates** unless M5 proves a demonstrated need.

---

## 1. Executive Summary

Writing LaTeX by hand is high-friction. LLMs can draft LaTeX but are **spatial-blind** and **syntax-fragile**: they routinely emit uncompilable code, misaligned tables, and broken environments. The current user loop is wasteful: prompt → copy → compile → fail → paste errors back → repeat.

LaTeX-AI shifts the model from **passive editor + chatbox** to **autonomous document engine**. Three pillars:

1. **Self-healing compile loop** — agent writes a patch into a scratchpad, compiles in background, reads the log, fixes its own errors, then surfaces only the *compilable* result.
2. **Visual grounding via reverse SyncTeX** — user draws a bounding box on the PDF; the engine maps it to the exact source line range and feeds {crop, snippet} to a Vision-Language Model (VLM) for targeted layout repair.
3. **Deterministic diff-driven control** — changes flow as structured patches (SEARCH/REPLACE or unified diff). User previews, accepts/rejects per hunk before commit.

Everything runs **local-first**, single-process, against a workspace sandbox.

---

## 2. Problems We Solve

| # | Problem | Symptom | Our angle |
|---|---|---|---|
| I | Prompt-Compile-Fail loop | Single char / `\end{table}` mismatch kills compile; user plays debugger | Agent intercepts log, self-corrects, returns only compilable output |
| II | LLM spatial blindness | Can't "see" overflow page/figure/table; manipulates code blind | VLM gets the rendered crop + localized snippet via SyncTeX |
| III | Mass rewrite damage | Fixing one sentence rewrites paragraphs, breaks `\label`/`\ref` | Strict `replace_range` / SEARCH-REPLACE patches with bounded hunk scope |

---

## 3. Solution — Three Pillars

### Pillar 1: Autonomous Self-Healing Compiler

**User intent** → "Insert a 3-column table comparing these hardware specs."
**Agent loop**:

1. Generate `AgentEdit[]` (typed: `replace_range` | `insert` | `rewrite_file` | `apply_dmp_patch`).
2. Apply into **isolated scratchpad** (temp dir, never user's file first).
3. Trigger `pdflatex` / `xelatex` via the compile queue.
4. On non-zero exit or log errors → parse log (`CompileError{line,file,message}`), feed error summary back into the chat as next turn.
5. Repeat up to `max_iterations` (default 3).
6. Only on success (or final iteration) surface the diff to the user for review.

**Engineering contract:**
- Use Gemini `client.aio.chats.create(model=...).send_message_stream(...)` for multi-turn agent state. Do **not** roll your own conversation dict (AFC is moving to `chats` only).
- Stream `{thinking, proposal, applying, compiled, done, error}` as SSE events via FastAPI `EventSourceResponse` (from `fastapi.sse`, not `sse-starlette`).
- Always `request.is_disconnected()` guard; on disconnect, **cancel** the running pdflatex via `PDFCompiler.cancel_compilation(job_id)` — never leave orphan processes.
- Compile errors feed back as **structured `CompileError` JSON**, not raw log text. Saves tokens + parsing.

### Pillar 2: Visual Grounding via Reverse SyncTeX

**Interaction**: user drags a bounding box over a misaligned image / table on the rendered PDF.
**Engine**:

1. PDF coordinates `{page, x, y, w, h}` → `SyncTeXParser.backward_search(page, x, y)` → `{file, line, column}`.
2. Expand to a ±N-line window around the hit (default 30 lines).
3. Render a tight **PDF crop** of just that bounding box (PNG, ≤512px on the long edge for token budget).
4. Build a VLM prompt: image crop + source snippet + intent. Ask Gemex `gemini-2.5-flash-image` (or equivalent) for a *bounded* patch.
5. Apply patch through the same healed compile loop in Pillar 1.

**Engineering contract:**
- VLM call signature: `generate_content(contents=[text_part, image_part], config=GenerateContentConfig(temperature=0.2, response_mime_type="application/json"))` — force JSON, parse strictly.
- Patch must be `replace_range` with explicit `start_line`/`end_line` (no `rewrite_file` from VLM path — VLM tends to over-emit).
- Cache the PDF→source map per compile (`synctex.gz` path gated on compile success).

### Pillar 3: Deterministic Diff-Driven Controls

**Patch protocol** (already in `schemas.AgentEdit`, keep it):
- 1-based line numbers, inclusive end.
- `op` ∈ `{rewrite_file, replace_range, insert, apply_dmp_patch, apply_unified_diff}`.
- `expected_etag` for optimistic concurrency; backend returns 409 with `current_content` on mismatch.

**Engineering contract:**
- Diff viewing = unified diff via Python `difflib.unified_diff`; never reflow or reformat user content server-side.
- Frontend hunk semantics: each `@@ -a,b +c,d @@` block is independently toggle-able; backend `applyAgentEdits` must accept a *selected subset* of hunks (handled today by `App.svelte#applyHunks` logic — incumbent, preserve it).
- Reject DMP/unified dual-path ambiguity (current bug): pick **one** canonical format. Decision: **SEARCH/REPLACE blocks only** for VLM/agent output; **unified diff** for human review display. Drop `apply_dmp_patch` op entirely.

---

## 4. Engineering Roadmap (ordered)

Phase gates are strict — no pillar work begins until prior gate's acceptance is green.

### M1 — Heal the engine (BLOCKER)
Fix the broken checkout before any feature work.
- Remove orphan block in `src/latex_ai/document_bundle.py` lines 120–138.
- Fix try/except in `src/latex_ai/api/health.py` lines 34–39.
- Repair `src/latex_ai/api/compile.py`: import `Dict`, fix `parser.get_source` → `backward_search`, import `JobStatus`.
- Fix `src/latex_ai/resource_management.py` bracket-token NameErrors (or delete the file — currently unused).
- Create `frontend/src/lib/api/client.js` exposing `api`, `APIClient`, `APIError` with the methods imported by services (`request, getFile, saveFile, getCompileStatus, compile, checkCompileCache, buildUrl, getConfig, setConfig, syncTex, getPdfUrl`). Reuse `axios` for HTTP.

**Acceptance**: `pytest tests/ -v` collects green; `cd frontend && npm run build` succeeds; `latex-ai serve` boots and serves `/health` = healthy.

**Audit Gate (M1) — recorded in PR:**
- Data integrity: confirm `document_bundle` import sphere no longer raises; confirm no orphan column references added.
- Logic correctness: walk every repaired branch (`health.py:34`, `compile.py:62/48/262`, `document_bundle.py:120`).
- Integration: `frontend/src/lib/api/client.js` exports match every name imported by `services/*` (compiler-checked via `npm run build`).
- Security: deleted/emptied `resource_management.py` (if chosen) leaves no orphan import; grep `from .resource_management` returns zero.

### M2 — Stable agent loop (Pillar 1 backstop)
- Rewrite `api/agent.py#agent_loop_stream` on top of `client.aio.chats.create(model=...).send_message_stream(...)`. Track chat per `session_id`. Use `get_history(curated=True)` for context compaction.
- Add `tools=[mcp_session]`-style **automatic function calling** only if we adopt `latex_compile` / `read_file` tool surface; otherwise stay manual. Decision: **manual dispatch** for M2 (MCP tool surface is an M4 exploration).
- Cancel pdflatex on client disconnect + finalize `compile_file` cancellable subprocess id.
- Concurrency guard: queue already dedups by bundle hash; keep it. Add per-session mutex on agent state.

**Acceptance**: Send intent "make table 3-col"; receive SSE stream showing `thinking → proposal → applying → compiled → done`. Induce a deliberately broken latent edit (e.g. `\begin{tabular}` w/o `\end`); agent must self-correct in ≤3 iterations and emit `done` with a compilable diff.

**Audit Gate (M2) — recorded in PR:**
- Data integrity: chat history lives in `client.aio.chats` keyed by `session_id`; no parallel `conversation_context` dict resurrected at the router layer.
- Logic correctness: state machine is total — `queued → compiling → compiled(done) | compiled(failed, retry<max) | error`; verify no path silently swallows last error.
- Integration: SSE event taxonomy consistent — `{thinking, proposal, applying, compiled, done, error}` only; frontend parser handles each.
- Security: prompt-injection sanitizer applied to `req.instruction` before it enters the chat; compile subprocess cancelled on disconnect (write a regression test).

### M3 — Reverse SyncTeX + VLM (Pillar 2)
- New endpoint `POST /api/visual-fix`: body `{file_path, page, x, y, w, h, intent}`.
  - Validate bounds, ensure synctex file exists for the *current* compile.
  - `parser.backward_search(page, x, y)` → source line.
  - Render PDF crop via `pdftoppm` (or `pdfium-render`) but only if we add libpdfium; otherwise proxy to a tiny Pillow+pdf2image helper — pick `pypdfium2` (lightweight, pure-ish, MIT).
- VLM call with `response_mime_type="application/json"` enforced; schema: `{ "edits": AgentEdit[] }`.
- Edits flow back through the same `apply_agent_edits` + healed compile loop.

**Acceptance**: Drag box over an overflowing figure on a demo PDF; intent "shrink this image to fit column"; get a compilable diff touching only the `\includegraphics` line range.

**Audit Gate (M3) — recorded in PR:**
- Data integrity: `backward_search(page,x,y)` return shape `{file,line,column}` always present; null only when synctex miss (and endpoint returns 404, not empty 200).
- Logic correctness: VLM JSON parse uses strict schema validator; malformed JSON → 422 with the raw response logged at WARN (never swallowed).
- Integration: `POST /api/visual-fix` reuses `apply_agent_edits` — no parallel edit pipeline; SyncTeX cache invalidates when source mtime > last synctex mtime.
- Security: PDF crop bytes capped (≤512px long edge); bounds validated `0 ≤ x ≤ page_width`, no negative; workspace path on returned `file` re-validated.

### M4 — Diff UX polish (Pillar 3)
- Frontend `DiffView` exposes per-hunk accept/reject (already scaffolded in `App.svelte`).
- Backend new endpoint `POST /api/agent/apply-selected`: body `{edits: AgentEdit[], selected_hunk_ids: str[]}`. Server applies only selected hunks atomically (single per-file lock, single ETag check, single history entry tagged `apply-selected`).
- `generate_edits` VLM path returns hunks with stable `hunk_id` (hash of search text).

**Acceptance**: Reject-all → file unchanged; accept one of three hunks → only that range modified, others untouched, history shows exactly one entry.

**Audit Gate (M4) — recorded in PR:**
- Data integrity: `hunk_id` is deterministic hash of search text; identical hunks across two prompts collapse to same id; reject stale `hunk_id` with 409 when underlying `expected_etag` drifts.
- Logic correctness: applying a strict subset of hunks is atomic per-file lock — aborts on ANY selection failure, never partial.
- Integration: `/api/agent/apply-selected` produces exactly ONE `history` entry tagged `apply-selected` regardless of hunk count.
- Security: per-file ETag check precedes hunk application; `AgentEdit` schema validates `start_line ≤ end_line`.

### M5 — Observability + hardening
- Wire `metrics.record_request` / `record_agent_request` / `record_compilation` as FastAPI dependencies (currently inert).
- Persist compile errors to `history_db.errors` table per (history_id, line, message) for post-hoc review.
- Circuit breaker already exists (`services/ai.py`); add per-session LLM spend counter + soft cap in `rate_limiter`.
- Tighten CORS: dev mode `localhost:5173`; prod mode `ENV=production` + `ALLOWED_ORIGINS`. Remove dev permissive wildcards in prod path (today's bug).
- Fix Dockerfile `--workers 4` contradiction: drop to `--workers 1` OR add external queue backend (Redis). Decision for now: **single worker**, document as known limitation.

**Audit Gate (M5) — recorded in PR:**
- Data integrity: new `errors` table joins to `history.id` via FK; migrate existing rows; reverse migration tested.
- Logic correctness: every router path instruments exactly one `metrics.record_*`; counter regression test asserts non-zero after a fixture request.
- Integration: spend counter increments before the LLM call returns, not after (so disconnects still count).
- Security: dev vs prod CORS branch covered by a config-driven test; `ENV=production` without `ALLOWED_ORIGINS` rejects cross-origin.

> **Stop Rule reminder (CONTEXT.md §3):** if any milestone cannot satisfy all four audit gates above + the completion checklist, revert to the last green state. A partial milestone committed to `main` violates this doc and CONTEXT.md alike.

---

## 5. Non-Goals (do not build)

These are *intentional* non-goals (Modularity §2 + Intentionality §5: complexity must be earned):

- Multi-user / multi-tenant. Workspace-bound, single-user.
- Multi-worker horizontal scale. In-memory queue + singletons are fine; documenting as limit is cheaper than Redis.
- Generic IDE features (find/replace across files, regex search UI). `api/files.py#/api/search` exists; leave it.
- Real-time collaborative editing.
- Cloud-hosted deployments as primary path. Local-first is the product.

---

## 6. Library & API Choices (state-of-the-art, locked) — **RUST NATIVE**

> **2026-06-24 PIVOT**: we abandoned Python and rewrote in Rust. The Rust USP is `tectonic` embedded as an in-process LaTeX engine — no subprocess, no TeX Live install requirement on host. This puts us years ahead of every peer project (latex-engineer, agentic-latex-writer, TexGuardian — all Python shell-out to `latexmk`). See `masterplan.md` §2 for the full stack and `rust.md` for coding guidelines + port spec.

| Concern | Rust pick | Why (state-of-the-art 2026) |
|---|---|---|
| **LaTeX engine** | **`tectonic`** ≥ 0.16 ([docs.rs/tectonic](https://docs.rs/tectonic)) — `tectonic::latex_to_pdf(&str) -> Result<Vec<u8>>` | **Killer USP.** XeTeX-based, no subprocess, self-contained bundle auto-fetched server-side and content-address cached on disk. We get full LaTeX + Unicode + modern fonts with a single `cargo add tectonic`. Global mutex is fine — our `CompilationQueue` serializes compiles anyway. |
| **Web framework** | **`axum`** 0.8 + `tokio` 1 + `tower` 0.5 | Standard Rust web stack; macro-free routing, type-safe extractors, integrated SSE via `axum::response::sse::{Event, Sse, KeepAlive}`. |
| **Desktop shell** | **`tauri`** 2 + `tauri-plugin-axum` | Same Rust server embedded inside Tauri webview; one binary serves both as standalone daemon (axum-on-port) and desktop app (`axum://localhost/` custom protocol). Reduces to a single codebase, ≤20 MB binary (per `r/tauri` 2026-03 thread reporting 114-route axum+Tauri shipped at 18 MB). |
| **SSE** | `axum::response::sse::Sse<Stream<Item = Result<Event, _>>>` with `KeepAlive` | Native; no extra dep. Frontend SSE parser unchanged. |
| **LLM/VLM orchestration** | **`genai-rs`** ≥ 0.7 ([docs.rs/genai-rs](https://docs.rs/genai-rs)) | Uses Gemini Interactions API natively. `previous_interaction_id` means **Gemini server owns chat history, we don't manage it at all** — eliminates the bespoke `conversation_context` dict problem entirely. `with_response_format(JSON)` for strict JSON, `#[tool]` macro for AFC, `StreamChunk::Delta` typed streaming. |
| **VLM model** | `gemini-2.5-flash-image` (or current Gemini image-capable model), pinned at M3 start | Forced JSON via `with_response_format`. |
| **Patch format** | SEARCH/REPLACE blocks for AI output; unified diff (`similar` crate) for display | Drop `apply_dmp_patch` op entirely per Pillar 3. Use `similar` for diff display (state-of-the-art Rust diff crate). |
| **PDF rasterize (crop render)** | **`pdfium-render`** + `tectonic` PDF output | Rust binding to Google's pdfium (the same lib used by Chromium's PDF viewer and ferroleaf). `PdfRender::render_pdf_page_to_bitmap` with clip rect. AGPL→MIT-friendly (pdfium itself is BSD-3). Alternative if Tauri env rejects pdfium: keep PyMuPDF FFI fallback deferred to M3 milestone gate. |
| **SyncTeX** | **NEW `latex-ai-synctex` crate** (we publish it; no mature Rust crate exists) | Port the regex-over-`.synctex.gz` parser using `flate2` + `nom` parser combinators (~600 LOC). We publish under MIT; becomes our open-source contribution. See `rust.md` §II.4. |
| **LSP** | **`tower-lsp-server`** + `lsp-types` community-maintained fork | We **become** the LaTeX LSP server in-binary — no texlab subprocess dependency. Invention: in-binary diagnostics completion as a fallback when texlab is absent. |
| **Database** | **`sqlx`** ≥ 0.8 with **`sqlx-cli`** migrations, SQLite runtime | Compile-time-checked queries (`query!` macro); eliminates whole classes of SQL bugs at compile time. Stays SQLite single-user local-first. |
| **Cache** | **`moka`** ≥ 0.12 (`future` + `sync` features; `Expiry` trait for TTL) | State-of-the-art Rust concurrent cache. Lightweight TinyLFU + LRU + size-aware. Default `moka::future::Cache` with `expire_after` based on per-key policy. |
| **Rate limiting** | **`tower-governor`** + `governor` (Generic Cell Rate Algorithm) | Per-key `StorageKey` extractor; works as a Tower layer for axum. Sessions keyed on `X-Session-ID` header. |
| **Hashing (ETag)** | **`blake3`** ≥ 1.5 — `blake3::hash(content.as_bytes())` | Order of magnitude faster than SHA-256; SIMD parallel; same key length as SHA-256 (`Hash` Display gives 64 hex chars). ETag uses 16-char prefix. |
| **Errors** | **`thiserror`** for libraries, **`anyhow`** for app layer; `.context()` at every trust boundary | Production consensus 2026 ([lucaberton](https://lucaberton.com/blog/rust-error-handling-production-2026/), [andrewodendaal](https://andrewodendaal.com/rust-error-handling-patterns-production/)). Every `?` at a boundary gets `.context()`. Never `unwrap()` in non-test code. |
| **Validation** | `serde` + `serde_json` + `garde` (validator-style checks) | `garde` is the 2026 successor to `validator`: async-capable, multi-language error messages, derive `Validate` macro. Strict-mode `#[derive(Validate)]` on every payload. |
| **Async cancellation** | `tokio-util::sync::CancellationToken` — propagated into every spawned task | `tokio::select!` against the token inside long-running loops. Pattern enforces "cancellation works in O(1) checks at await points, never hangs on disconnect". |
| **Tracing / logging** | `tracing` + `tracing-subscriber` (`EnvFilter`, `JsonLayer`/`fmt::Layer`) + optional `tracing-opentelemetry` | Replaces structlog. Baggage propagation via `tonic`/`opentelemetry-otlp` exporter only at M5+. Structured spans, not string interpolation. `info!`/`warn!`/`error!` with fields, never `println!`. |
| **Config** | `~/.latex-ai/config.toml` parsed via `serde` + `toml` crate (or `config` crate for layered env/file/args) | Strong typing via `#[derive(Deserialize, Clone, Default)]` with field_doc comments — automatically generates `.latex-ai.toml.example` via `figment`/`config-derive` macros. No env-vs-toml split-brain. |
| **Frontend** | Svelte 5 + Vite 8 + Monaco + PDF.js (kept from Python iteration — Svelte is JS, not Python, so the pivot is wall-effective on the frontend) | Existing frontend stays. Tauri shell + `tauri-plugin-axum` embeds axum routes inside the webview via `axum://localhost/` for true desktop-distributed single-binary UX without any Python residue. |
| **Security** | `std::fs` + `std::path::Path` canonicalization (resolve) + workspace containment check + `walkdir` traversal + symlink-aware via `std::fs::symlink_metadata` | Rust's `Path` API makes "no path traversal possible" a compile-time-ish fact + zero-cost abstraction. Allowed-extensions via `const ALLOWED: &[&str] = &[\".tex\", \".bib\", \".sty\", \".cls\", \".tikz\"];`. |
| **Binary size** | `cargo` with `opt-level = "z"` + `lto = "fat"` + `codegen-units = 1` + `strip = true` + `panic = "abort"` | Tauri+axum in single binary ≤ 25 MB confirmed by peer reports. |
| **CI/cleanliness** | `cargo fmt --check`, `cargo clippy -- -D warnings -D clippy::pedantic`, `cargo deny check`, `cargo audit`, `cargo machete`, `cargo nextest run` | All gates mandatory on every PR. No `unsafe` without SAFETY.md justification. No `unwrap` outside `#[cfg(test)]`. |
| **Workspace** | Cargo workspace, members: `core/`, `server/`, `compiler/`, `lsp/`, `visual/`, `synctex/`, `tauri-shell/` | Each crate is a general-purpose building block (Modularity §0.1) — `compiler` doesn't know about `lsp`, `lsp` doesn't know about `server`. |

---

## 7. Contracts the Codebase Must Honor (Rust edition)

When extending the engine, every PR must respect:

1. **All file writes** go through `core::storage::save_with_etag_history`. No direct `tokio::fs::write` outside `core::storage`. Write order: temp file → `tokio::fs::rename` (atomic on POSIX + Windows) → history commit.
2. **All user-supplied paths** pass through `core::security::validate_path(workspace, path, AccessKind::Read|Write)` (`Path::canonicalize` + workspace containment + extension allow-list). No raw `Path::new(req.path)`.
3. **All LLM multi-turn state** lives inside `genai_rs::Client` via `previous_interaction_id`. We do NOT store chat history anywhere — Gemini's Interactions API stores server-side and hands us content-addressed IDs. Re-introducing a HashMap<session_id, Vec<Content>> is a violation of CONTEXT.md §5 Intentionality.
4. **All SSE handlers** are `Sse<impl Stream<Item = Result<Event, anyhow::Error>>>` returning a `tokio::sync::mpsc::Receiver` or `tokio_stream`-wrapping generator. The stream must consult a `CancellationToken` on every `yield` and cancel child tasks (compile, AI stream) on disconnect. `axum` does not auto-cancel; we wire `cancellation::CancelOnDisconnect` tower middleware (also new — invention).
5. **All AI edits** are `AgentEdit` serde-strict types (`#[serde(deny_unknown_fields)]`), never raw strings. No `serde_json::Value` in any handler signature.
6. **All line numbers** in `AgentEdit::start_line`/`end_line`/`line` are **1-based, inclusive** — runtime-enforced via `garde::Validate` rules.
7. **All compile jobs** go through `compiler::queue::CompilationQueue::submit` (dedup via blake3 bundle hash, debounce 250 ms, concurrency cap via `tokio::sync::Semaphore`). Direct `tectonic::latex_to_pdf` is a violation — only `compiler::engine` uses Tectonic directly, behind the queue.
8. **No `unsafe`** outside `core/src/safety/` (which is empty by default and requires SAFETY.md entry per `unsafe-block` lint).
9. **No `unwrap()` / `expect()` in non-test code.** Add a `#![deny(clippy::unwrap_used, clippy::expect_used)]` crate attribute at the root of every workspace member.
10. **No `println!`/`eprintln!` in non-test code.** Use `tracing::info!`/`warn!`/`error!`. CI grep fails any patch that adds it.
11. **Tectonic global mutex serialization** — only one Tectonic engine runs at a time per process. `compiler::engine::COMPILER_LOCK: OnceLock<Mutex<()>>` enforces this. The `CompilationQueue` runs serially inside this lock; concurrency comes from queueing + downstream PDF render not from parallel compiles.
12. **No multi-process primitives** (Redis, channel.backends, std::sync::mpsc): single-process only. State is `tokio::sync` + `OnceLock` + `Arc<RwLock<_>>`.
13. **Backups**: every save writes a `.bak.<blake3_16hex>` sibling capped at 10 per file via `moka::sync::Cache<PathBuf, VecDeque<PathBuf>>` LRU.
14. **For every `Arc<Mutex<T>>`** annotation in a PR, explain in PR description why `Arc<RwLock<T>>` or `arc-swap` doesn't fit. Lock contention is a P0 perf audit.
15. **For every public `async fn`** answer in PR description: is the future `Send`? Cancelling via drop should be safe — does it cleanup partial state? (`CancellationToken` propagation, not `select!` panic-on-drop).
16. **`#[must_use]` on every public `Result`-returning fn.** Compiler-enforced; tables of ignored errors vanish.
17. **All SQL via `sqlx::query!` or `sqlx::query_as!` macros** (compile-time checked against schema + migrations). No `query()` runtime strings.
18. **Single workspace, single lockfile** — don't introduce sub-`Cargo.toml`s that aren't workspace members; we share `Cargo.lock` (CONTEXT.md §3: integration surface clean).

---

## 8. Definition of Done (per milestone)

Each milestone is done when **every item below is satisfied** (CONTEXT.md §3 — no partial shipping):

- All acceptance criteria pass with real commands typed into the running server (real Tectonic embed, not mocked).
- `cargo fmt --check` green; `cargo clippy -- -D warnings -D clippy::pedantic -D clippy::unwrap_used -D clippy::expect_used` green; `cargo nextest run` green; `cd frontend && npm run build` green — all in one invocation.
- `cargo deny check` (advisories + licenses + bans) green. No unreviewed CVEs in `Cargo.lock`.
- `cargo audit` clean. `cargo machete` clean (no unused deps).
- A new test exists that exercises the happy path end-to-end (genai-rs Plex integration mock via `wiremock-rs`).
- All edge cases called out in the milestone's bullet list have a corresponding test (failure-mode tests required, not optional). Coverage gate: `cargo tarpaulin --workspace --fail-under-80`}.
- Audit Gate block for the milestone is filled in the PR description with real findings (or "no findings" per audit).
- All tunable parameters introduced are externalized via `~/.latex-ai/config.toml` (default + example entry in `.latex-ai.toml.example`-style auto-generated output via `figment`); no magic numbers in code.
- All new endpoints documented in `README.md` "API surface" section with request/response shape.
- Pre-implementation questions (§0.4) answered in the PR description for each new crate/file/function.
- `idea.md` unchanged mid-milestone — if scope drifts, amend this doc first, then implement.

---

## 9. Open questions (resolve before milestone start)

1. **VLM model id** — pin to current Gemini image-capable model at M3 start; update `core::models_registry` accordingly.
2. **xelatex via Tectonic** — Tectonic is XeTeX-based by default, so this is automatic — no `pdflatex` vs `xelatex` choice; we always get XeLaTeX/Unicode. LuaLaTeX support may come via a future Tectonic feature; out of scope for V1.
3. **Scratchpad location** — `tempfile::TempDir` (auto-cleaned on drop) vs persisted at `~/.latex-ai/scratch/<session_id>/`. Lean persisted for debugging; finalize at M2.
4. **Hunk identity** — blake3 of search text (16-char prefix); identical hunks across replans collapse; resistant to ordering. Lock at M4.
5. **Tectonic bundle strategy** — default `cosmic` bundle is large (~1 GB cache, fetched on demand). For Tauri deployment, consider bundling a minimal format-file + selective packages via `Tectonic.toml`. Decide at M5.
6. **Tauri custom protocol vs axum TCP** — `tauri-plugin-axum` provides `axum://localhost/` custom protocol (no streaming support per Tauri issue #1404) vs TCP loopback (works for SSE). Use TCP loopback on `127.0.0.1:<port>` for SSE routes, custom protocol for non-streaming handlers. Lock at M2.
7. **rust-synctex crate name availability** — `synctex`/`synctex_parser` may already exist as empty crates.io squats; final crate name `latex-ai-synctex` lock at M3.