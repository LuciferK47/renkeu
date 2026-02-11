# LaTeX-AI Frontend

Svelte + Vite web UI for AI-assisted LaTeX editing: Monaco editor, PDF preview, diffable AI plans, template inserts, and streaming chat.

## Setup

```bash
cd frontend
npm install
```

## Development

```bash
npm run dev
```

Opens at http://localhost:5173 with proxy to backend http://127.0.0.1:3000.

## Build

```bash
npm run build
```

Emits static assets to `src/latex_ai/static/` (served by FastAPI).

## Experience overview

- Layout: Overleaf-style (Editor | Right panel | optional Chat). Dividers are keyboard/mouse draggable buttons.
- Editor: Monaco, auto-save, dirty tracking, optional TexLab LSP (diagnostics → markers, hover, completions). Degrades if TexLab absent.
- Files: File tree (search, multi-select), quick open (pinned/recent/search), bulk mode driven by selections.
- Command palette: Fuzzy suggestions + recent/frequent. Enter → preview/execute, Shift+Enter → apply-focused preview, Ctrl+Enter → apply now. Dry-run toggle, AI plan option with model picker.
- Right panel: PDF preview, diff view, compile errors/warnings with explain/apply-fix, AI plan steps, suggestions, template library insert.
- Chat: Bottom panel streams AI responses via SSE.
- Accessibility: Combobox ARIA wiring, tree items announce selection, overlay backdrop button to close, dividers are buttons.

## Key components

- `App.svelte` — Layout, state, wiring of editor/preview/chat/command palette
- `Editor.svelte` — Monaco + LSP bridging (TexLab WebSocket), save/dirty events
- `PdfViewer.svelte` — Renders compiled PDF URL
- `DiffView.svelte` — Unified diff with apply-selected/apply-all flows
- `CommandPalette.svelte` — Command/AI input, shortcuts, dry-run, model select
- `ChatPanel.svelte` — Streaming AI chat (SSE)
- `RightPanel.svelte` — Tabs for preview/diff/errors/templates/suggestions
- `FileTree.svelte` — File browser with selection and search
- `FileOpener.svelte` — Quick open with pinned/recent/search

## Backend integration (high level)

- Files: list/search (`/api/files`), read/write current file
- Compile: enqueue/poll job → PDF URL + errors/warnings
- AI: chat/plan/review/suggestions/explain/apply-fix via SSE/REST
- Models: list available models and current default

## Running with backend

```bash
# Terminal 1: backend
cd ..
python -m latex_ai serve --host 0.0.0.0 --port 3000

# Terminal 2: frontend dev
cd frontend
npm run dev
```

Production: `npm run build` then `python -m latex_ai serve` (serves static build).

## Environment

- `VITE_API_BASE` — Override backend base URL (default http://127.0.0.1:3000)

## Debugging tips

- Network tab for API/SSE calls; console for component errors
- Backend logs: `python -m latex_ai serve --log-level debug`
