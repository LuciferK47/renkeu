# LaTeX-AI Frontend

Svelte 5 + Vite SPA for the LaTeX-AI engine: Monaco editor, server-rendered PDF
preview, streaming AI agent loop, and per-hunk diff review.

The backend is the Rust `latex-ai` daemon in this repo — see the root
[README](../README.md).

## Setup

```bash
npm ci
npm run dev     # http://localhost:5173, proxies /api and /ws to :3000
```

Start the backend in another terminal (from the repo root, which becomes the
workspace the API is sandboxed to):

```bash
source .env                                     # GROQ_API_KEY
cargo run -p latex-ai-server -- serve --port 3000
```

## Test & build

```bash
npx vitest run
npm run build   # -> dist/ (also what the Tauri shell bundles)
```

## Layout

| Path | Role |
|---|---|
| `src/lib/api/client.js` | The only HTTP layer. Native `fetch`; one method per backend route. |
| `src/services/_stream.js` | The single SSE parser. Every stream in the app goes through it. |
| `src/services/` | Thin wrappers over `client.js` per domain (file, compile, ai, config). |
| `src/controllers/` | Orchestration: services in, store updates out. Components stay dumb. |
| `src/stores/` | Svelte stores: `fileStore`, `compileStore`, `aiStore`, `uiStore`. |
| `src/layouts/` | `IdeShell` → `WorkspaceLayout` / `PanelManager`. |
| `src/components/` | Editor, PdfViewer, DiffView, ChatPanel, CommandPalette, … |

## Backend contract

Everything the UI calls exists in `crates/latex_ai_server`:

- **Files** — `GET /api/files`, `GET /api/file?path=`, `POST /api/file`
  (send `If-Match` with the ETag; a **409** carries `current_etag`).
- **Search** — `GET /api/search?q=` (workspace files) and `POST /api/search`
  (client-supplied candidates). Fuzzy matching is server-side via `nucleo`.
- **Compile** — `POST /api/compile` → `{ job_id }`, then
  `GET /api/compile/stream/{job_id}` (SSE). Pages come from
  `GET /api/pdf/page?file=&n=&dpi=`.
- **AI** — `POST /api/plan/stream` and `POST /api/agent/stream` (SSE),
  `POST /api/agent/apply-selected`, `POST /api/visual-fix`.
- **Other** — `GET /api/config`, `GET /api/history`.

### SSE rules

Payloads are **always JSON objects**, and event names come only from the locked
taxonomy (masterplan §8.2), enforced in CI:

`progress` · `step` · `proposal` · `applying` · `compiled` · `thinking` ·
`done` · `error` · `awaiting_approval` · `keepalive`

Consume them as `{ event, data }` from `streamSSE` — never reach for a `type`
field inside the payload.

## Notes

- **No axios, no fuse.js, no PDF.js.** Native `fetch`, backend `nucleo` search,
  and backend pdfium rendering respectively (masterplan §10.1).
- **LSP is off.** There is no in-binary LaTeX language server until M5; set
  `VITE_ENABLE_LSP=1` only once `crates/lsp` exists.
- `VITE_API_BASE` overrides the API base URL (default `/api`, via the dev proxy).
