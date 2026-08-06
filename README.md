# LaTeX-AI

A writing app for LaTeX documents that **fixes its own mistakes using AI**, and
runs **entirely on your own computer**.

> **Status:** all five milestones (M1–M5) implemented. The engine compiles
> in-process, the AI self-healing loop works end to end, drawing a box on the
> PDF maps back to the source line that produced it, changes are accepted hunk
> by hunk, and the desktop shell bundles the lot. See
> [`masterplan.md`](masterplan.md) §12 for the per-milestone record and §15.1
> for where the implementation knowingly differs from the plan.

## What is this, in plain words?

**LaTeX** is a way of writing documents where you type plain text with little
commands (like `\section{Intro}`) and a program turns it into a polished PDF —
the kind of typesetting you see in research papers and textbooks. It looks
great, but it's finicky: one wrong symbol and the whole thing refuses to build,
usually with an error message only a robot could love.

LaTeX-AI is an editor that takes the pain out of that:

- it turns your LaTeX into a PDF;
- when the build breaks, an **AI reads the error, edits your source, and tries
  again** — looping until it works. We call this the *self-healing* loop;
- you can point at a spot on the finished PDF and say "fix this," and it figures
  out which line of source you meant.

Nothing leaves your machine except the snippets actually sent to the AI.

## The one trick worth knowing

Normally, turning LaTeX into a PDF means installing a giant toolkit (TeX Live —
several gigabytes) and running it as a separate program your app has to babysit.

We skip all that. We **build the LaTeX engine directly into our program** (a Rust
library called [Tectonic](https://tectonic-typesetting.github.io/)). Think of it
like the difference between phoning a print shop every time versus having the
printer built into your desk. The payoff: no multi-gigabyte install, faster
builds, and far fewer moving parts to break. Missing fonts or packages are
fetched automatically the first time you need them.

## How it works (the quick tour)

```
  You type LaTeX  ─►  Backend compiles it with Tectonic  ─►  PDF
        ▲                          │
        │                    build failed?
        │                          ▼
        └──── AI edits the source ◄── AI reads the error
                 (repeat until it compiles)
```

The AI never writes to your file directly. Edits are applied to a **scratchpad**
copy, compiled there, and only saved once they actually build — and, unless you
opt into `auto_apply`, only after you approve them hunk by hunk. Each hunk is
previewed by the server using the same code that performs the write, so what
you review is exactly what you get; a hunk that no longer matches the file is
shown as stale rather than silently applied.

- **Backend — Rust.** Does the compiling, talks to the AI, and exposes a small
  web API on `127.0.0.1` only.
- **Frontend — the browser.** Monaco (the editor inside VS Code) wrapped in
  Svelte. Kept in JavaScript on purpose — see [`masterplan.md`](masterplan.md) §10.1.
- **Live updates.** Progress streams back over SSE (the server pushing little
  updates instead of you refreshing).
- **Click-to-edit.** Drawing a box on the PDF resolves, through the SyncTeX
  source map, to the exact line that produced that spot. The crop plus the
  surrounding source go to the model, which proposes a bounded edit you review
  before it is applied.

> **Vision caveat.** Analysing the crop needs a vision-capable model. Groq's
> current lineup is text-only, so with Groq alone visual fix still finds the
> right source line and proposes a fix — but from the source text, not the
> picture. Point `LATEX_AI_VISION_MODEL` at a vision model to get the full
> behaviour.

## Build & run

You'll need **Rust 1.87+**, **Node 20+**, and a handful of system libraries the
engine relies on at build time:

```sh
# Debian / Ubuntu  (on Ubuntu 24.04+ the freetype package is `libfreetype-dev`)
sudo apt-get install -y pkg-config libfontconfig1-dev libgraphite2-dev \
    libharfbuzz-dev libicu-dev libfreetype-dev libssl-dev zlib1g-dev
```

PDF preview additionally needs **libpdfium** on the loader path (no distro
package exists; grab a prebuilt binary):

```sh
# Pinned to the same build CI uses; `latest` moves and has broken this before.
curl -L -o /tmp/pdfium.tgz \
  https://github.com/bblanchon/pdfium-binaries/releases/download/chromium/7961/pdfium-linux-x64.tgz
mkdir -p /tmp/pdfium && tar -xzf /tmp/pdfium.tgz -C /tmp/pdfium
sudo cp /tmp/pdfium/lib/libpdfium.so /usr/local/lib/ && sudo ldconfig
```

Then:

```sh
cargo build --workspace --exclude latex-ai-tauri

# Turn a .tex file into a PDF, right from the terminal
cargo run -p latex-ai-server -- compile examples/sample.tex -o out.pdf

# Or start the server (listens on your machine only, port 3000)
cargo run -p latex-ai-server -- serve --port 3000
curl http://127.0.0.1:3000/health     # -> {"status":"ok"}
```

The very first compile downloads the LaTeX support files into
`~/.cache/Tectonic/`; after that it's offline.

**The directory you launch from is the workspace** — every API path is sandboxed
to it, and the history database lands there as `.latex-ai.db`.

### AI configuration

The planner talks to **Groq** through the [`genai`](https://docs.rs/genai) crate.
Put your key in a gitignored `.env` and source it:

```sh
echo 'export GROQ_API_KEY="gsk_..."' > .env && chmod 600 .env
source .env
```

| Variable | Default | Purpose |
|---|---|---|
| `GROQ_API_KEY` | *(required for AI routes)* | Groq API key |
| `LATEX_AI_MODEL` | `llama-3.3-70b-versatile` | Planner model. Must be a name `genai` recognizes, or it silently falls back to Ollama. |
| `LATEX_AI_VISION_MODEL` | *(unset)* | Model used for visual fix. **Groq currently serves no vision model**, so leave it unset for Groq and visual-fix degrades to a source-only fix (see below). Set it to any vision model `genai` can route — e.g. an OpenAI or Gemini one, with that provider's key exported — to have the PDF crop actually analysed. |
| `ALLOWED_ORIGINS` | Vite dev origins | Comma-separated CORS allowlist |
| `LATEX_AI_HOME` | `~/.latex-ai` | Scratchpad root |
| `ENV` | *(unset)* | Set to `production` to require an explicit `ALLOWED_ORIGINS` |
| `LATEX_AI_HOST` | `127.0.0.1` | Bind address (`--host`). The daemon has no authentication, so leave it on loopback unless something in front restricts access; the container sets `0.0.0.0` (see below) |
| `RUST_LOG` | `info` | Tracing filter |

### Frontend

```sh
cd frontend && npm ci && npm run dev   # :5173, proxies /api to :3000
```

### Desktop app

The desktop shell bundles the same server and the built SPA into one binary.

```sh
cd frontend && npm ci && npm run build && cd ..
cargo tauri build --config crates/latex_ai_tauri/tauri.conf.json --bundles deb
# -> target/release/bundle/deb/*.deb   (also: appimage; dmg/msi need macOS/Windows)
sudo dpkg -i target/release/bundle/deb/*.deb && latex-ai-tauri
```

The bundle is **6.5 MB** (15.4 MB binary) — the whole LaTeX engine is in there;
no TeX Live install.

> **Note on `external-harfbuzz`.** The workspace enables Tectonic's
> `external-harfbuzz` feature so it links the *system* harfbuzz. Without it the
> desktop shell segfaults: webkit2gtk links the system copy dynamically, and
> two copies of harfbuzz in one process let the linker resolve symbols across
> implementations. This is why `libharfbuzz-dev` is a required build
> dependency.

It binds the API to an **ephemeral loopback port** and injects that address
into the page before any script runs — the SPA is served from
`tauri://localhost`, so a relative `/api` would never reach the server.

Both debug and release builds load the bundled SPA, so
`cargo run -p latex-ai-tauri` works without a Vite server running. Build the
frontend first (`npm run build`) — the shell embeds `frontend/dist`.

Building `.dmg` and `.msi` requires macOS and Windows respectively; Tauri does
not cross-compile bundles.

### Production deployment

Set `ENV=production` and the server **refuses to start** without an explicit
`ALLOWED_ORIGINS` — silently falling back to the development origins would
leave a deployment trusting localhost.

```sh
ENV=production ALLOWED_ORIGINS=https://latex.example.com \
  latex-ai serve --port 3000
```

`GET /api/metrics` returns counters as JSON, or Prometheus text when the
request carries `Accept: text/plain`.

### Docker

```sh
ALLOWED_ORIGINS=http://localhost:3000 docker compose up -d
curl http://127.0.0.1:3000/health     # -> {"status":"ok"}
```

The image is the daemon only — the SPA is served by Vite in development and
bundled into the desktop app. Compose publishes to **host loopback**
(`127.0.0.1:3000:3000`) and mounts `./examples` as the sandboxed workspace,
with named volumes for the Tectonic bundle and the history database.

It is built `FROM scratch`: **62.8 MB unpacked, 24.9 MB to pull, 55 files**.
The runtime holds the binary and exactly the shared objects `ldd` reports for
it — the library set is derived from the binary at build time rather than
hand-listed, so a new transitive dependency cannot be missed. Roughly half the
remaining size is `libicudata.so`, which XeTeX needs for bidi, normalisation
and line breaking.

Two consequences worth knowing:

- **There is no shell**, so `docker compose exec … sh` will not work. Debug by
  reading logs, or run the same binary outside the container.
- **`HEALTHCHECK` runs `latex-ai healthcheck`**, the binary probing its own
  `/health`. That is why the image needs no curl — carrying the curl
  dependency tree just to run one health probe was most of what a
  `debian-slim` runtime was for.

> **Reading image sizes.** `docker images` on the containerd store sums the
> unpacked layers *and* the compressed blob, so it reports 89 MB for this
> image. `docker history` totals the real unpacked size; `docker image
> inspect --format '{{.Size}}'` gives the compressed one.

Inside the container the server binds `0.0.0.0`, because Docker forwards a
published port to the container's interface address — a loopback-only bind
would be unreachable from the host while the in-container healthcheck still
reported healthy. The host-side mapping is what limits access.

## What's in the box

The code is split into focused **crates** (Rust's word for a self-contained
package):

| Crate | What it does |
|---|---|
| `crates/compiler` | Runs Tectonic to turn LaTeX into PDF bytes: global mutex, result cache, dedup/debounce queue, per-session scratchpads |
| `crates/latex_ai_core` | Use-case-agnostic services: AI planner, edit application, storage + history + durable jobs, path sandbox, prompt safety, fuzzy search, metrics |
| `crates/latex_ai_server` | The axum API + the `latex-ai` command-line tool |
| `crates/synctex` | `.synctex(.gz)` parser, scaled-point→PDF-point conversion, backward *and* forward search |
| `crates/visual` | pdfium page rendering and bbox cropping (one process-wide binding) |
| `crates/latex_ai_tauri` | Desktop shell: embeds the server on an ephemeral loopback port and bundles the SPA (`.deb` / `.appimage`) |

## Quality bar

Every change has to pass, and CI enforces all of it:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --exclude latex-ai-tauri --all-targets -- -D warnings
cargo nextest run --workspace --exclude latex-ai-tauri
cargo deny check          # advisories + licenses + sources
cargo machete             # no unused dependencies
cd frontend && npx vitest run
```

### Resource limits

Several tests start a real Tectonic engine, which loads the font/bundle cache
and costs hundreds of MB each. Left unbounded, `nextest` runs one test process
per core and `cargo` one rustc per core — enough to exhaust a 16 GB machine.
Two checked-in files cap that:

| File | Limit | Why |
|---|---|---|
| `.config/nextest.toml` | 4 test threads; LaTeX-compiling tests in a `max-threads = 1` group | Only one Tectonic in flight at a time — it mirrors the engine's own global mutex, so it costs almost no wall-clock |
| `.cargo/config.toml` | `jobs = 6` | rustc peaks at 1-2 GB per crate on this dependency tree |

Raise both if you have RAM to spare, or override per run with
`cargo build -j <n>` / `cargo nextest run --test-threads <n>`.

Those two files cannot bound a tool that spawns its own build — `cargo
tarpaulin` rebuilds the workspace instrumented *and* re-runs every test under
ptrace, and has taken this machine down. For anything heavy, use the wrapper:

```sh
scripts/capped cargo tarpaulin --workspace --exclude latex-ai-tauri
CAP=8G scripts/capped cargo build --release
```

It runs the command in its own systemd scope with `MemoryMax`, so an overrun
kills that cgroup rather than your shell, editor, or desktop session.

Workspace lints are deny-level (`clippy::pedantic`, no `unwrap`/`expect`/`todo`
outside tests). CI additionally asserts two things that are invisible to the
compiler: that streaming endpoints only emit events from the locked SSE
taxonomy (masterplan §8.2), and that Tectonic keeps its `external-harfbuzz`
feature — dropping it reintroduces a desktop segfault that only appears when
the bundle is actually run.

## License

MIT OR Apache-2.0 — use it however you like.
