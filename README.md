# Renkeu

> Renkeu was called LaTeX-AI. The product name changed; the crates, the `latex-ai`
> command, `LATEX_AI_*` settings and `~/.latex-ai` keep their names for now.

A writing app for LaTeX documents that **fixes its own mistakes using AI**, and
runs **entirely on your own computer**.

> **Status:** feature-complete (M1–M17). The engine compiles in its own
> killable worker process, the AI works on a copy of your project and checks its
> edits compile before showing them, every change is reviewed hunk by hunk and
> can be undone from the history, and the desktop app bundles the lot. See
> [`masterplan.md`](masterplan.md) §0 for how it is built, §15.1 for where the
> build knowingly differs from the original plan.

## What is this, in plain words?

**LaTeX** is a way of writing documents where you type plain text with little
commands (like `\section{Intro}`) and a program turns it into a polished PDF —
the kind of typesetting you see in research papers and textbooks. It looks
great, but it's finicky: one wrong symbol and the whole thing refuses to build,
usually with an error message only a robot could love.

Renkeu is an editor that takes the pain out of that:

- it turns your LaTeX into a PDF;
- when the build breaks, an **AI reads the error, edits your source, and tries
  again** — looping until it works. We call this the *self-healing* loop;
- you can point at a spot on the finished PDF and say "fix this," and it figures
  out which line of source you meant.

Nothing leaves your machine except the snippets actually sent to the AI.

## What you can do with it

- **Edit a whole project**, not one file: a file tree (create, rename, delete,
  drag files in), tabs that keep their own undo history, autosave, and a choice
  of main file.
- **Write LaTeX comfortably:** syntax highlighting, completion for citations,
  labels, files, packages and environments, an outline, spell check, hover and
  Ctrl+click on `\ref`, `\cite` and `\input`, and compile errors drawn on the line.
- **See the PDF as you go:** continuous scrolling and zoom, **Ctrl+Alt+J** shows
  where the cursor line landed, and double-clicking the PDF jumps to its source.
- **Ask the AI for changes:** it tries them on a copy, checks the document still
  compiles, then shows the edits for you to accept, per hunk and per file.
- **Undo anything:** the history keeps what every save and AI change replaced.

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

PDF preview additionally needs **libpdfium** (no distro package exists; grab a
prebuilt binary). The Docker image and the desktop bundle carry their own; for a
source build either install it system-wide:

```sh
# Pinned to the same build CI uses; `latest` moves and has broken this before.
curl -L -o /tmp/pdfium.tgz \
  https://github.com/bblanchon/pdfium-binaries/releases/download/chromium/7961/pdfium-linux-x64.tgz
mkdir -p /tmp/pdfium && tar -xzf /tmp/pdfium.tgz -C /tmp/pdfium
sudo cp /tmp/pdfium/lib/libpdfium.so /usr/local/lib/ && sudo ldconfig
```

or keep it anywhere and point at the folder with `LATEX_AI_PDFIUM_DIR=/path/to/dir`
(`scripts/fetch-pdfium.sh DIR` downloads and verifies it for you). Check it with:

```sh
cargo run -p latex-ai-server -- doctor    # -> pdfium: ok (<where it was found>)
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
to it. App data (settings, API key, edit history) is kept under `~/.latex-ai`
(`$LATEX_AI_HOME`), not inside your project: history is in
`~/.latex-ai/projects/<id>/history.db`. A `.latex-ai.db` left in the workspace by
an older version is imported automatically on first start and otherwise ignored;
you can delete it afterwards.

### AI configuration

The planner talks to **Groq** through the [`genai`](https://docs.rs/genai) crate.
The easiest way to set it up is **Settings → AI** in the app: paste your key
there and it takes effect immediately, no restart. It is stored in
`~/.latex-ai/secrets.toml` (mode `0600`) and is never shown again. Model and
timeout are saved to `~/.latex-ai/config.toml`; see
[`config.example.toml`](config.example.toml).

Or use the environment instead. Environment variables win over the files, and
anything they set is shown as locked in the Settings dialog. For example, put
your key in a gitignored `.env` and source it:

```sh
echo 'export GROQ_API_KEY="gsk_..."' > .env && chmod 600 .env
source .env
```

| Variable | Default | Purpose |
|---|---|---|
| `GROQ_API_KEY` | *(or set it in Settings)* | Groq API key. Without any key the AI routes answer 503 and the chat says AI is off. |
| `LATEX_AI_MODEL` | `llama-3.3-70b-versatile` | Planner model. Must be a name `genai` recognizes, or it silently falls back to Ollama. |
| `LATEX_AI_VISION_MODEL` | *(unset)* | Model used for visual fix. **Groq currently serves no vision model**, so leave it unset for Groq and visual-fix degrades to a source-only fix (see below). Set it to any vision model `genai` can route — e.g. an OpenAI or Gemini one, with that provider's key exported — to have the PDF crop actually analysed. |
| `ALLOWED_ORIGINS` | Vite dev origins | Comma-separated CORS allowlist |
| `LATEX_AI_HOME` | `~/.latex-ai` | App data: `config.toml`, `secrets.toml`, per-project history, scratchpad |
| `ENV` | *(unset)* | Set to `production` to require an explicit `ALLOWED_ORIGINS` |
| `LATEX_AI_HOST` | `127.0.0.1` | Bind address (`--host`). The daemon has no authentication, so leave it on loopback unless something in front restricts access; the container sets `0.0.0.0` (see below) |
| `RUST_LOG` | `info` | Tracing filter |

### Frontend

```sh
cd frontend && npm ci && npm run dev   # :5173, proxies /api to :3000
```

A first visit opens the **landing page** (`#/landing`); "Open editor" goes to
the editor (`#/editor`), and the app remembers that you have been there. The
desktop app always opens the editor.

#### The landing page

The page is a pixel-art 3D tower ("Night Foundry") that the visitor scrolls
down floor by floor: street, repair loop, workspace, engine bench, vault, dock.
All the words are ordinary page content (`src/landing/copy.js`, checked by a
test against what the code does); the scene is a backdrop.

- **How it is drawn:** three.js renders procedural voxels into a small target,
  magnifies it without smoothing, and dithers the colours (`src/landing/scene/`).
  The characters (the courier crab, the repair bot) come from the same pixel
  maps as the editor's 2D sprites (`src/lib/sprites/`).
- **Fallbacks:** with no WebGL, reduced motion or data saver, it shows still
  frames of each floor (`public/landing/poster-*.jpg`) instead. `?poster`
  forces that, `?tier=lite` forces the light version. A pause button stops the
  animation.
- **Two states:** if a Renkeu server answers, the main button is "Open
  editor" and an engine chip shows the delay; if not (a public copy), it is
  "Get Renkeu".
- **The public copy:** `BASE_PATH=/Latex-AI/ npm run build:site` builds only the
  landing page into `frontend/dist-site/`, for any static host.
- **Regenerating the pictures:** with the dev server (and, for the editor shot,
  a `latex-ai serve` in a project folder) running,
  `node scripts/capture-landing.mjs http://127.0.0.1:5173 --editor`.

### Desktop app

The desktop shell bundles the same server and the built SPA into one binary.

```sh
cd frontend && npm ci && cd ..
scripts/fetch-pdfium.sh      # libpdfium for the PDF preview, verified, into the bundle's resources
cargo tauri build --config crates/latex_ai_tauri/tauri.conf.json --bundles deb
# -> target/release/bundle/deb/*.deb   (also: appimage; dmg/msi need macOS/Windows)
sudo dpkg -i target/release/bundle/deb/*.deb && latex-ai-tauri
```

The whole LaTeX engine is in the binary (no TeX Live install) and the package
carries its own libpdfium under `usr/lib/Latex-AI/pdfium/`. Without the
`fetch-pdfium.sh` step the app still builds, but PDF preview then needs a
system libpdfium.

To check a package really works, `scripts/smoke-desktop.sh PACKAGE.deb` unpacks
it, starts the unpacked app with no other libpdfium available, compiles a
document and renders a page (it needs a display; CI runs it under `xvfb-run`).

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
| `crates/compiler` | Compiles a project in a killable worker process (this binary, re-executed): main-file detection, project fingerprint, content-addressed build store, structured diagnostics from the TeX log |
| `crates/latex_ai_core` | Use-case-agnostic services: config and secrets, AI planner, edit engine and TeX lint, storage + history, path sandbox, prompt safety, fuzzy search, metrics |
| `crates/latex_ai_server` | The axum API + the `latex-ai` command-line tool |
| `crates/synctex` | `.synctex(.gz)` parser, scaled-point→PDF-point conversion, backward *and* forward search |
| `crates/visual` | pdfium page rendering and bbox cropping (one process-wide binding) |
| `crates/latex_ai_tauri` | Desktop shell: embeds the server on an ephemeral loopback port and bundles the SPA (`.deb` / `.appimage`) |

## Quality bar

Every change has to pass, and CI enforces all of it:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings      # includes the desktop crate
cargo nextest run --workspace --exclude latex-ai-tauri
cargo deny check          # advisories + licenses + sources
cargo machete             # no unused dependencies

cd frontend
npx eslint . && npm run check && npm run knip   # lint, svelte-check (warnings fail), dead code
npx vitest run                                  # unit and component tests
npm run build && npm run e2e                    # Playwright against the real server
```

The end-to-end tests start `latex-ai serve` and `vite preview` themselves; they
need the server binary built (`cargo build -p latex-ai-server`, or set
`E2E_SERVER_BIN`), libpdfium (on the loader path or `E2E_PDFIUM_DIR`), and a
browser (`npx playwright install chromium firefox webkit`, or set `E2E_CHROME` for Chromium; `--project=firefox` picks one engine).

To drive the assistant against a real model instead of a scripted stream
(slow and not deterministic, so never part of CI), name one `genai` can reach —
for example a local Ollama model — and run the live spec:

```sh
E2E_LLM_MODEL=qwen2.5:7b npm run e2e -- --project=chromium assistant-live
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
