# LaTeX-AI

A writing app for LaTeX documents that **fixes its own mistakes using AI**, and
runs **entirely on your own computer**.

> **Status:** M2 complete. The engine compiles in-process, the API is up, and
> the AI self-healing loop works end to end. Click-the-PDF editing (M3),
> per-hunk polish (M4), and the packaged desktop app (M5) are next — see
> [`masterplan.md`](masterplan.md) §12.

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
opt into `auto_apply`, only after you approve them hunk by hunk.

- **Backend — Rust.** Does the compiling, talks to the AI, and exposes a small
  web API on `127.0.0.1` only.
- **Frontend — the browser.** Monaco (the editor inside VS Code) wrapped in
  Svelte. Kept in JavaScript on purpose — see [`masterplan.md`](masterplan.md) §10.1.
- **Live updates.** Progress streams back over SSE (the server pushing little
  updates instead of you refreshing).

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
curl -L -o /tmp/pdfium.tgz \
  https://github.com/bblanchon/pdfium-binaries/releases/latest/download/pdfium-linux-x64.tgz
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
| `ALLOWED_ORIGINS` | Vite dev origins | Comma-separated CORS allowlist |
| `LATEX_AI_HOME` | `~/.latex-ai` | Scratchpad root |
| `RUST_LOG` | `info` | Tracing filter |

### Frontend

```sh
cd frontend && npm ci && npm run dev   # :5173, proxies /api to :3000
```

## What's in the box

The code is split into focused **crates** (Rust's word for a self-contained
package):

| Crate | What it does |
|---|---|
| `crates/compiler` | Runs Tectonic to turn LaTeX into PDF bytes: global mutex, result cache, dedup/debounce queue, per-session scratchpads |
| `crates/latex_ai_core` | Use-case-agnostic services: AI planner, edit application, storage + history, path sandbox, prompt safety, fuzzy search |
| `crates/latex_ai_server` | The axum API + the `latex-ai` command-line tool |
| `crates/synctex` | `.synctex(.gz)` parser + backward search (PDF position → source line) |
| `crates/visual` | pdfium page rendering and bbox cropping |
| `crates/latex_ai_tauri` | Desktop shell (M5) — builds, not yet packaged |

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

Workspace lints are deny-level (`clippy::pedantic`, no `unwrap`/`expect`/`todo`
outside tests). CI additionally asserts that streaming endpoints only emit
events from the locked SSE taxonomy (masterplan §8.2).

## License

MIT OR Apache-2.0 — use it however you like.
