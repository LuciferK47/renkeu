# LaTeX-AI

A writing app for LaTeX documents that **fixes its own mistakes using AI**, and
runs **entirely on your own computer**.

> **Status:** early days (milestone M1). The engine compiles documents and the
> server is up. The AI auto-fix loop, click-the-PDF editing, and the desktop app
> arrive in M2–M5 — see [`masterplan.md`](masterplan.md) §12.

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

- **Backend — Rust.** Rust is a programming language prized for being fast *and*
  hard to crash. It does the compiling, talks to the AI, and exposes a small web
  API.
- **Frontend — the browser.** The editor is Monaco (the same one inside VS Code),
  wrapped in a lightweight UI framework (Svelte). Kept in JavaScript on purpose
  — see [`masterplan.md`](masterplan.md) §10.1 for the why.
- **Live updates.** Progress streams back as it happens (a web technique called
  SSE — basically the server pushing little updates instead of you refreshing).
- **Click-to-edit (coming in M3).** A behind-the-scenes map called *SyncTeX*
  links each spot on the PDF to the exact line of source that produced it, so
  clicking the PDF knows what to change.

## Build & run

You'll need **Rust 1.87+** and a handful of system libraries the engine relies
on at build time:

```sh
# Debian / Ubuntu
sudo apt-get install -y pkg-config libfontconfig1-dev libgraphite2-dev \
    libharfbuzz-dev libicu-dev libfreetype6-dev libssl-dev zlib1g-dev
```

Then:

```sh
cargo build --workspace

# Turn a .tex file into a PDF, right from the terminal
cargo run -p latex-ai-server -- compile examples/sample.tex -o out.pdf

# Or start the server (listens on your machine only, port 3000)
cargo run -p latex-ai-server -- serve --port 3000
curl http://127.0.0.1:3000/health     # -> {"status":"ok"}
```

The very first compile downloads the LaTeX support files into
`~/.cache/Tectonic/`; after that it's offline.

## What's in the box

The code is split into focused **crates** (Rust's word for a self-contained
package):

| Crate | What it does |
|---|---|
| `crates/compiler` | Runs Tectonic to turn LaTeX into PDF bytes, safely and one-at-a-time |
| `crates/latex_ai_core` | Shared building blocks (e.g. fuzzy file search) |
| `crates/latex_ai_server` | The web API + the `latex-ai` command-line tool |

Crates for click-to-edit, PDF rendering, and the desktop app are added when we
get to those milestones — we don't create empty folders ahead of time.

## Quality bar

Every change has to pass: code formatting (`cargo fmt`), the linter
(`cargo clippy`), the tests (`cargo test`), a license/security check
(`cargo deny`), and an unused-dependency check (`cargo machete`).

## License

MIT OR Apache-2.0 — use it however you like.
