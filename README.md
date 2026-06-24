# LaTeX-AI

Local-first, single-binary **Autonomous Document Engine**. Rust backend with an
in-process LaTeX engine (Tectonic — no TeX Live install), an `axum` HTTP server,
and a self-healing AI compile loop. See `idea.md` (pitch), `masterplan.md`
(architecture), `rust.md` (coding rules), `CONTEXT.md` (the four pillars).

> **Status:** M1 — backend skeleton. Tectonic compiles in-process; `axum` serves
> `/health`. Agent loop, SyncTeX, visual-fix, LSP, Tauri shell land in M2–M5
> (see `masterplan.md` §12).

## Build prerequisites

Rust **1.87+** and the C libraries Tectonic links at build time:

```sh
# Debian/Ubuntu
sudo apt-get install -y pkg-config libfontconfig1-dev libgraphite2-dev \
    libharfbuzz-dev libicu-dev libfreetype6-dev libssl-dev zlib1g-dev
```

## Build & run

```sh
cargo build --workspace

# Compile a .tex to PDF in-process
cargo run -p latex-ai-server -- compile examples/sample.tex -o out.pdf

# Run the server (binds 127.0.0.1:3000)
cargo run -p latex-ai-server -- serve --port 3000
curl http://127.0.0.1:3000/health    # {"status":"ok"}
```

First compile downloads the Tectonic bundle to `~/.cache/Tectonic/`.

## Workspace

| Crate | Role |
|---|---|
| `crates/compiler` | Tectonic wrapper: global mutex, cancellation, blake3 cache |
| `crates/latex_ai_server` | `axum` app + `latex-ai` CLI (`serve`, `compile`) |
| `crates/latex_ai_core` | use-case-agnostic services (M2) |
| `crates/synctex` | `.synctex.gz` parser (M3) |
| `crates/visual` | PDF crop/rasterize (M3) |
| `crates/lsp` | in-binary LaTeX LSP (M5) |

## CI gates

`cargo fmt --check` · `cargo clippy --workspace --all-targets` ·
`cargo test --workspace` · `cargo deny check`.

License: MIT OR Apache-2.0.
