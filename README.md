# LaTeX-AI

An AI-assisted LaTeX workspace with a web UI, Monaco editor, PDF preview, diffable AI plans, and a CLI. It streams AI chat/plan responses, offers a command palette with keyboard-first workflows, and integrates optional TexLab LSP for diagnostics/completions.

## What you can do

- Edit LaTeX with Monaco, live PDF preview, and unified diffs before applying changes
- Run AI chat, AI plans, and command execution with apply/preview/apply-selected flows
- Browse files with tree, quick-open (pinned/recent/search), and bulk-select for multi-file commands
- Use a template library to insert ready-made LaTeX snippets
- Get compiler errors/warnings with explain/apply-fix hooks; see AI suggestions and code reviews
- Optional TexLab LSP: diagnostics → Monaco markers, hover, and completions (auto-detected if installed)

## Quick start (backend + frontend)

**Using Make (recommended):**

```bash
# Install dependencies
make install        # Install Python + frontend dependencies
make dev           # Install development dependencies

# Development
make run           # Start development server
make test          # Run test suite
make lint          # Run code linting
make format        # Format code with black

# Cleanup and packaging
make clean         # Remove build artifacts and caches
make pack          # Create clean distribution ZIP
make clean-all     # Nuclear clean (removes venv/node_modules)
```

**Manual setup:**

```pwsh
# Backend
python -m venv .venv
. .venv/Scripts/Activate.ps1
pip install -e ".[dev]"
python -m latex_ai serve --host 0.0.0.0 --port 3000

# Frontend (dev)
cd frontend
npm install
npm run dev  # proxies to http://127.0.0.1:3000
```

Production build (served by FastAPI):

```pwsh
cd frontend
npm run build
# static assets emitted to src/latex_ai/static/
python -m latex_ai serve --host 0.0.0.0 --port 3000
```

## How the app works

- Layout: Overleaf-style split (Editor | Right panel | optional Chat). Dividers are keyboard/mouse draggable buttons.
- Files: File tree (search, select, multi-select), quick open with pinned/recent/search, bulk selection feeds bulk commands.
- Editor: Monaco with auto-save, dirty state, optional TexLab LSP (didOpen/didChange, diagnostics → markers, hover/completions). Gracefully degrades if LSP unavailable.
- Command palette: Fuzzy suggestions + recent/frequent commands. Keyboard: Enter (preview/execute), Shift+Enter (apply-focused preview), Ctrl+Enter (apply immediately). Dry-run toggle and AI plan option with model picker.
- Right panel: PDF preview, diff view, compile errors/warnings with explain/apply-fix, AI plan steps, suggestions, template library insert.
- Chat: Bottom panel streams AI replies via SSE.
- Compilation: Backend queue produces PDF URL, errors/warnings; UI updates status and markers.
- Accessibility: Dividers as buttons, combobox ARIA wiring, tree items announce selection, overlay backdrop button for close.

## API surface (high level)

- Files: list/search (`/api/files`), read/write current file
- Compile: enqueue/poll job, return PDF URL + errors/warnings
- AI: chat/plan/review/suggestions/explain/apply-fix via SSE/REST
- Models: list available models and current default

## Backend architecture

- Routers: the backend is split into focused modules under `src/latex_ai/api/`:
	- `files.py`: file I/O, history, models, security, config endpoints
	- `agent.py`: AI plan, suggestions, reviews, patch/apply, agent loop, bundle
	- `compile.py`: compile queue, status streaming, SyncTeX, PDF, metrics
	- `health.py`: `/health` and `/api/health` status endpoints
	- `lsp.py`: TexLab LSP WebSocket (optional)
- App: `server.py` wires routers, lifecycle (history DB, cache, queue, CORS), and serves built frontend from `src/latex_ai/static/` when present.
- Compatibility: `server.py` re-exports commonly used symbols for tests and integrations:
	- `AgentEdit`, `AgentRequest`, `AgentResponse`, `ConfigResponse`
	- `validate_path`, `get_workspace_root`
	- `compute_etag`
- Deprecations: legacy `/api/agent/patch` and `/api/agent/apply` endpoints remain with warnings; prefer the structured `POST /api/agent` edit protocol.

## CLI (still supported)

You can run the original CLI for scripted flows:

```pwsh
latex-ai preview examples/sample.tex
latex-ai apply examples/sample.tex "set margins 1in"
latex-ai apply examples/sample.tex "set line spacing 1.5" --dry-run
latex-ai repl examples/sample.tex
```

## Tests

```pwsh
pytest tests/ -v
pytest tests/ --cov=latex_ai --cov-report=html
```

## Run the backend

```pwsh
python -m latex_ai serve --host 0.0.0.0 --port 3000
```

When the frontend is built (`npm run build`), assets are served from `src/latex_ai/static/`. In dev, run the Vite server and it will proxy API calls.

## Deployment & Architecture

**For production deployment:**
- See [DEPLOYMENT.md](DEPLOYMENT.md) for deployment options and best practices
- See [ARCHITECTURE.md](ARCHITECTURE.md) for architecture decisions (multi-worker support, static serving strategy)

**Key highlights:**
- Single-process deployment supported (development and small production)
- Multi-worker deployment (uvicorn --workers N) not yet supported; see ARCHITECTURE.md for roadmap
- **Packaging for distribution:** Use `make pack` or `python package.py latex-ai.zip` to create a clean distribution archive that excludes development artifacts (.venv, node_modules, .pytest_cache, etc.)
- **Clean builds:** Use `make clean` to remove build artifacts before packaging or testing

## Notes and tips

- TexLab LSP is optional; if installed, the editor auto-enables diagnostics/completions.
- AI features stream responses; you can apply individual hunks from diff view or whole plans.
- Template library inserts snippets at the editor cursor.
- Bulk mode uses selected files from the file tree for multi-file commands.
