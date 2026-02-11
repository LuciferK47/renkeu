# LaTeX-AI Interview Preparation Guide

> Complete codebase reference for corporate interviews. All sections covered with bullet points.

---

## Table of Contents

1. [Project Overview](#project-overview)
2. [Tech Stack Explained](#tech-stack-explained)
3. [Backend Architecture](#backend-architecture)
4. [Frontend Architecture](#frontend-architecture)
5. [API Endpoints Reference](#api-endpoints-reference)
6. [State Management](#state-management)
7. [Configuration System](#configuration-system)
8. [Security Implementation](#security-implementation)
9. [Testing Strategy](#testing-strategy)
10. [DevOps & Deployment](#devops--deployment)
11. [Common Interview Questions](#common-interview-questions)

---

## Project Overview

### What is LaTeX-AI?
- AI-assisted LaTeX document editor
- Web-based IDE similar to Overleaf
- Uses Google Gemini for intelligent suggestions
- Real-time PDF preview with SyncTeX support
- Command-line interface for scripted workflows

### Key Features
- Monaco editor (same as VS Code)
- Live compilation with error highlighting
- AI-powered document planning and fixes
- Multi-file project support
- Edit history with undo/redo
- Optional TexLab LSP integration

---

## Tech Stack Explained

### Backend Stack

| Technology | Version | Why Used |
|------------|---------|----------|
| **Python** | 3.9+ | Strong typing, async support, AI library ecosystem |
| **FastAPI** | 0.115.x | Async-first, automatic OpenAPI docs, high performance |
| **Uvicorn** | 0.24+ | ASGI server, production-grade |
| **Pydantic** | 2.x | Data validation, settings management |
| **Typer** | 0.12+ | CLI with rich help text |
| **google-generativeai** | 0.3+ | Official Gemini API SDK |

### Frontend Stack

| Technology | Version | Why Used |
|------------|---------|----------|
| **Svelte** | 5.x | Minimal bundle, reactive without virtual DOM |
| **Vite** | 6.x | Fast HMR, ESM-native bundling |
| **TailwindCSS** | 3.4+ | Utility-first, rapid styling |
| **Monaco Editor** | 0.45+ | VS Code's editor, full LSP support |
| **Axios** | 1.6+ | HTTP client with interceptors |

### External Dependencies

| Tool | Purpose |
|------|---------|
| **pdflatex** | LaTeX to PDF compilation |
| **TexLab** | Language Server Protocol for LaTeX |
| **SQLite** | Local history database |

---

## Backend Architecture

### Directory Structure
```
src/latex_ai/
├── __init__.py          # Package exports
├── cli.py               # CLI entry point
├── server.py            # FastAPI app setup
├── api/                 # Route handlers
├── services/            # Business logic
├── commands/            # Command registry
├── static/              # Built frontend
└── [core modules]       # Engine, planner, etc.
```

---

### Core Module: `cli.py`

**Location**: `src/latex_ai/cli.py` (~350 lines)

**Purpose**: Command-line interface

**Key Commands**:
- `preview` - Show document settings summary
- `apply` - Execute single formatting command
- `plan` - Generate AI editing plan
- `repl` - Interactive command loop
- `serve` - Start web server
- `start` - Production server startup
- `list_commands` - Show available commands

**Implementation Details**:
- Uses Typer library for CLI parsing
- Rich library for styled console output
- Supports `--dry-run` flag for previews
- Opens browser automatically on `serve`

**Interview Points**:
- "Why Typer over argparse?" → Type hints, auto-completion, rich formatting
- "How does REPL mode work?" → Loads file, executes commands in loop, reloads after each edit

---

### Core Module: `server.py`

**Location**: `src/latex_ai/server.py` (~180 lines)

**Purpose**: FastAPI application factory

**Lifecycle Management** (`lifespan` function):
- Sets up structured logging
- Initializes history database at `~/.latex-ai/history.db`
- Creates compilation semaphore for concurrency control
- Starts compilation queue
- Initializes cache
- Cleanup on shutdown (LSP clients, queue, AI executor)

**CORS Configuration**:
- Production: reads `ALLOWED_ORIGINS` env var
- Development: allows localhost:5173 (Vite dev server)

**Routers Registered**:
- `files` - File I/O operations
- `agent` - AI endpoints
- `compile` - PDF compilation
- `history` - Edit history
- `health` - Health checks
- `lsp` - Language server WebSocket
- `metrics` - Performance metrics

**Middleware**:
- Request ID tracing (X-Request-ID header)
- Cache headers for static assets

**Interview Points**:
- "Why asynccontextmanager for lifespan?" → Proper startup/shutdown in async context
- "How is CORS handled?" → Environment-based configuration for security

---

### Core Module: `engine.py`

**Location**: `src/latex_ai/engine.py` (~350 lines)

**Purpose**: LaTeX command execution

**Class**: `LatexEngine`

**Initialization**:
- Loads file from disk
- Stores original text
- Optional command registry integration

**Key Methods**:

| Method | Purpose |
|--------|---------|
| `reload()` | Re-read file from disk |
| `compute_diff(new_text)` | Generate unified diff |
| `apply_command(command, auto_save)` | Parse and execute command |
| `preview()` | Return document summary |
| `save(backup=True)` | Write to disk with backup |

**Editing Commands**:
- `_set_margins(size)` - Modify geometry package
- `_set_line_spacing(value)` - Set setspace package
- `_set_figure_width(width)` - Update includegraphics
- `_set_documentclass(class)` - Change document class
- `_add_package(pkg)` - Insert usepackage
- `_remove_package(pkg)` - Delete usepackage
- `_normalize_whitespace()` - Clean up spacing

**Validation Methods**:
- `_validate_size()` - Check LaTeX dimensions (1in, 2.5cm)
- `_validate_spacing()` - Range check [1.0, 3.0]
- `_validate_documentclass()` - Known class names

**Backup System**:
- Creates timestamped backups before saves
- Keeps max 10 backups per file
- Stored alongside original file

**Interview Points**:
- "How are commands parsed?" → Regex patterns, command registry lookup
- "Why create backups?" → Non-destructive editing, user safety

---

### Core Module: `document.py`

**Location**: `src/latex_ai/document.py` (~290 lines)

**Purpose**: High-level document abstraction

**Classes**:
- `DocumentPosition` - Line, column, offset
- `DocumentRange` - Start and end positions
- `Document` - Main abstraction

**Document Methods**:

| Method | Returns | Description |
|--------|---------|-------------|
| `load(path)` | Document | Async factory method |
| `reload()` | None | Refresh from disk |
| `find_documentclass()` | (class, options) | Get document class |
| `find_preamble_end()` | int | Offset before \begin{document} |
| `get_packages()` | List[(name, opts)] | All loaded packages |
| `has_package(name)` | bool | Check if loaded |
| `add_package(name, opts)` | str | Insert in preamble |
| `remove_package(name)` | str | Delete from preamble |
| `insert_preamble(content)` | str | Add after documentclass |
| `replace_documentclass(cls)` | str | Change class |

**Features**:
- Uses `aiofiles` for async I/O (optional)
- Lazy AST parsing via `LatexParser`
- Regex-based fallbacks for performance

**Interview Points**:
- "Why separate Document from Engine?" → Clean separation of concerns
- "How is async file loading handled?" → Optional aiofiles, sync fallback

---

### Core Module: `ai_planner.py`

**Location**: `src/latex_ai/ai_planner.py` (~1400 lines)

**Purpose**: AI-powered editing with Gemini

**Data Classes**:
- `EditStep` - Single edit: command, description, rationale
- `EditPlan` - Goal, steps[], summary, confidence score

**Main Class**: `AIPlanner`

**Initialization**:
- API key from param or `GEMINI_API_KEY` env
- Model selection (default: gemini-pro)
- Persistent SQLite cache
- Conversation context storage

**Key Methods**:

| Method | Streaming | Description |
|--------|-----------|-------------|
| `create_plan()` | No | Generate edit plan |
| `create_plan_streaming()` | Yes | SSE-compatible streaming |
| `add_to_conversation()` | - | Track conversation history |
| `get_conversation_context()` | - | Format history for prompts |
| `clear_conversation()` | - | Reset session |

**Smart Context Extraction** (`_prepare_smart_context`):
- Avoids sending entire document
- Extracts relevant sections based on request
- Respects token limits

**Caching Strategy**:
- Content hash for cache keys
- SQLite persistent storage
- API cost reduction

**Circuit Breaker Pattern**:
- `_check_circuit_breaker()` - Prevent cascading failures
- `_record_ai_success()` - Reset failure count
- `_record_ai_failure()` - Increment failure count

**Conversation Management**:
- Session-based tracking
- Max messages per session limit
- TTL-based session cleanup
- Bounded storage prevents memory leaks

**Interview Points**:
- "Why streaming for plans?" → Better UX, eliminates hanging
- "What is circuit breaker?" → Fail-fast on repeated API errors
- "How is context managed?" → Session-based with TTL cleanup

---

### Core Module: `pdf_compiler.py`

**Location**: `src/latex_ai/pdf_compiler.py` (~1100 lines)

**Purpose**: LaTeX to PDF compilation

**Data Classes**:
- `CompileError` - Line, message, file, error_type
- `CompileResult` - Success, pdf_path, errors, warnings, log, compile_time, page_count, synctex_path

**Main Class**: `PDFCompiler`

**Security Validations**:
```
Blocked Commands:
- \write18 (shell escape)
- \input| (piped input)
- \openin (file input)
- \openout (file output)
```

**Validation Methods**:
- `_validate_filename()` - No shell metacharacters, path traversal
- `_validate_latex_content()` - No dangerous commands

**Caching System**:
- `_compute_content_hash()` - SHA256 of content + config
- `_compute_bundle_hash()` - Hash all project files
- `_get_cached_pdf()` - Retrieve if valid
- `_save_cache_entry()` - Store new compilations
- `_enforce_cache_limit()` - Evict oldest entries

**Compilation Process**:
1. Validate filename and content
2. Check cache for existing PDF
3. Prepare temp directory with project files
4. Run pdflatex with multi-pass (for cross-refs)
5. Parse log for errors/warnings
6. Cache successful result
7. Return CompileResult

**Multi-file Support** (`_prepare_temp_dir`):
- Symlinks for large binaries (images, PDFs)
- Copies text files only
- Preserves project structure

**Interview Points**:
- "Why validate LaTeX content?" → Prevent shell injection via \write18
- "How does caching work?" → Content hash, bundle hash for invalidation
- "Why multi-pass compilation?" → Cross-references, citations need 2-3 passes

---

### Core Module: `config.py`

**Location**: `src/latex_ai/config.py` (~325 lines)

**Purpose**: Configuration management

**Configuration Classes**:

| Class | Fields |
|-------|--------|
| `KeyboardShortcuts` | save, undo, compile, apply_command, toggle_history, toggle_file_tree |
| `ThemeConfig` | mode, primary_color, accent_color, error_color |
| `EditorConfig` | font_size, font_family, theme, tab_size, word_wrap, minimap_enabled, ignore_patterns |
| `Config` | All of above + workspace_root, ai_model_default, max_concurrent_compilations, compilation_timeout, max_ai_context_size, max_websocket_connections |

**AppSettings (Pydantic-Settings)**:
- Loads from environment variables
- Prefix: `LATEX_AI_`
- Example: `LATEX_AI_MAX_FILE_SIZE=20971520`

**ConfigManager**:
- TOML file format
- Location: `~/.latex-ai/.latex-ai.toml`
- Methods: `get_config()`, `update_config()`, `get_keyboard_shortcut()`

**Global Accessors**:
- `get_config_manager()` - Singleton manager
- `get_config()` - Current config
- `get_app_settings()` - Environment-based settings

**Interview Points**:
- "Why TOML over JSON?" → Human-readable, comment support
- "How are env vars loaded?" → Pydantic-Settings with prefix

---

### Core Module: `compilation_queue.py`

**Location**: `src/latex_ai/compilation_queue.py` (~27KB)

**Purpose**: Async compilation job queue

**Features**:
- Background job execution
- Concurrency limiting via semaphore
- Status tracking (pending, running, complete, error)
- Progress streaming via SSE
- Job cancellation support
- TTL for completed jobs

**Queue Operations**:
- `start()` - Initialize queue
- `stop()` - Shutdown gracefully
- `submit(job)` - Add to queue
- `get_status(job_id)` - Check progress
- `cancel(job_id)` - Cancel pending/running

**Interview Points**:
- "Why queue compilation?" → Non-blocking, multiple users
- "How is concurrency controlled?" → asyncio.Semaphore

---

### Core Module: `history_db.py`

**Location**: `src/latex_ai/history_db.py` (~18KB)

**Purpose**: SQLite-based edit history

**Features**:
- Stores file snapshots with timestamps
- Tracks edit source (manual, ai, command)
- Enables undo/redo functionality
- Bounded storage (max entries per file)

**Schema**:
```sql
file_path TEXT
content TEXT
timestamp DATETIME
source TEXT (manual|ai|command)
description TEXT
```

**Operations**:
- `save_snapshot()` - Store current state
- `get_history()` - List entries for file
- `restore_snapshot()` - Revert to entry
- `prune()` - Remove old entries

**Interview Points**:
- "Why SQLite over file-based?" → ACID, efficient queries, atomic writes

---

### Core Module: `lsp_client.py`

**Location**: `src/latex_ai/lsp_client.py` (~16KB)

**Purpose**: TexLab Language Server client

**Features**:
- Connects to TexLab LSP server
- Sends didOpen/didChange notifications
- Receives diagnostics (errors/warnings)
- Hover info and completions
- Graceful degradation if unavailable

**Protocol Messages**:
- `initialize` - Handshake
- `textDocument/didOpen` - File opened
- `textDocument/didChange` - Content changed
- `textDocument/completion` - Get completions
- `textDocument/hover` - Get hover info

**Interview Points**:
- "What is LSP?" → Language Server Protocol, editor-agnostic tooling
- "Why optional?" → Not all users have TexLab installed

---

### Core Module: `parser.py`

**Location**: `src/latex_ai/parser.py` (~10KB)

**Purpose**: LaTeX AST parsing

**Features**:
- Tokenizes LaTeX source
- Builds abstract syntax tree
- Finds environments, commands, arguments
- Used by Document class

**Key Methods**:
- `parse()` - Build AST from text
- `find_command(name)` - Locate command usage
- `find_environment(name)` - Locate environment

---

### Core Module: `security.py`

**Location**: `src/latex_ai/security.py` (~9KB)

**Purpose**: Security utilities

**Path Validation**:
- `validate_path(path)` - Check for traversal attacks
- `is_within_workspace(path)` - Ensure file in allowed directory
- `sanitize_filename(name)` - Remove dangerous characters

**Content Validation**:
- `check_dangerous_content(content)` - Scan for injection
- `rate_limit_check(client_id)` - Prevent abuse

**Interview Points**:
- "How is path traversal prevented?" → Canonical path comparison
- "What injection attacks exist?" → Shell escape via \write18

---

### Core Module: `rate_limiter.py`

**Location**: `src/latex_ai/rate_limiter.py` (~5KB)

**Purpose**: API rate limiting

**Implementation**:
- Token bucket algorithm
- Session-based tracking
- Configurable limits

**Classes**:
- `RateLimiter` - Global limiter
- `SessionBasedRateLimiter` - Per-session limits

---

### Other Backend Modules

| Module | Lines | Purpose |
|--------|-------|---------|
| `cache.py` | ~6KB | In-memory LRU cache |
| `metrics.py` | ~10KB | Performance metrics |
| `streaming.py` | ~1KB | SSE event helpers |
| `streaming_helpers.py` | ~5KB | Async streaming utilities |
| `structured_logging.py` | ~7KB | JSON logging setup |
| `synctex_parser.py` | ~6KB | SyncTeX file parsing |
| `document_bundle.py` | ~16KB | Multi-file project handling |
| `resource_management.py` | ~13KB | Resource cleanup |
| `plugin_loader.py` | ~6KB | Plugin system |
| `models_registry.py` | ~1KB | AI model registry |
| `types.py` | ~1KB | Type definitions |
| `state.py` | ~0.5KB | Global state container |

---

## API Endpoints Reference

### Files API (`api/files.py`)

| Method | Endpoint | Purpose |
|--------|----------|---------|
| GET | `/api/files` | List workspace files |
| GET | `/api/files/read` | Read file content |
| POST | `/api/files/write` | Write file content |
| GET | `/api/files/search` | Search file contents |
| GET | `/api/config` | Get configuration |
| PUT | `/api/config` | Update configuration |
| GET | `/api/models` | List AI models |

---

### Agent API (`api/agent.py`)

| Method | Endpoint | Streaming | Purpose |
|--------|----------|-----------|---------|
| POST | `/api/agent` | No | Main edit protocol |
| GET | `/api/agent/plan/stream` | Yes | AI plan generation |
| POST | `/api/agent/patch` | No | Generate patch |
| POST | `/api/agent/apply-blocks` | No | Apply diff blocks |
| GET | `/api/agent/explain/stream` | Yes | Explain errors |
| GET | `/api/agent/suggest/stream` | Yes | Suggestions |
| GET | `/api/agent/review/stream` | Yes | Document review |
| POST | `/api/agent/loop` | Yes | Multi-step agent |
| POST | `/api/agent/undo` | No | Undo last edit |
| POST | `/api/agent/redo` | No | Redo edit |
| GET | `/api/agent/bundle` | No | Get file bundle |

---

### Compile API (`api/compile.py`)

| Method | Endpoint | Purpose |
|--------|----------|---------|
| POST | `/api/compile` | Submit compile job |
| GET | `/api/compile/{job_id}` | Get job status |
| GET | `/api/compile/{job_id}/stream` | Stream progress |
| GET | `/api/pdf/{filename}` | Serve PDF |
| GET | `/api/synctex` | Position mapping |

---

### Other APIs

| Module | Endpoints |
|--------|-----------|
| `health.py` | `GET /health`, `GET /api/health` |
| `lsp.py` | `WS /api/lsp` |
| `history.py` | `GET /api/history/{file}` |
| `metrics.py` | `GET /api/metrics` |

---

### API Schemas (`api/schemas.py`)

**Request Models**:
- `PlanRequest` - file_path, request_text, model
- `AgentRequest` - instruction, file_paths, mode
- `AgentLoopRequest` - goal, max_iterations
- `CompileRequest` - file_path, options

**Response Models**:
- `AgentResponse` - edits[], summary
- `AgentEdit` - file_path, diff, new_content
- `ConfigResponse` - all config fields
- `FileInfo` - path, size, modified

---

## Frontend Architecture

### Directory Structure
```
frontend/src/
├── main.js              # Entry point
├── App.svelte           # Main component
├── globals.css          # Global styles
├── api.js               # API functions
├── apiClient.js         # Axios setup
├── errors.js            # Error handling
├── components/          # UI components
├── stores/              # State stores
├── services/            # API services
├── layouts/             # Layout components
├── utils/               # Utilities
└── workers/             # Web workers
```

---

### Main Component: `App.svelte`

**Location**: `frontend/src/App.svelte` (~1700 lines)

**Responsibilities**:
- Layout management (split panes)
- State coordination between components
- File loading/saving
- Compilation workflow
- AI chat processing
- Keyboard shortcut handling
- Toast notifications

**State Variables**:
- `filePath`, `editorContent` - Current file
- `pdfUrl`, `pdfPages` - PDF preview
- `compiling`, `compileErrors` - Compile status
- `chatMessages`, `chatLoading` - Chat state
- `selectedFiles`, `bulkMode` - Multi-select
- `activeRightTab` - Right panel tab

**Key Functions**:
- `loadFile()` - Read file from backend
- `saveFile()` - Write file to backend
- `triggerCompile()` - Start PDF compilation
- `requestAIPlan()` - Get AI suggestions
- `handleChatMessage()` - Process chat input
- `applyBulkCommand()` - Multi-file operations

---

### Component: `Editor.svelte`

**Location**: `frontend/src/components/Editor.svelte` (~20KB)

**Features**:
- Monaco editor integration
- LaTeX syntax highlighting
- Auto-save with debounce
- LSP diagnostics display
- Cursor position tracking
- Content change events

**Monaco Configuration**:
- Language: latex
- Theme: configurable
- Word wrap: enabled
- Minimap: optional

**Events Emitted**:
- `change` - Content modified
- `save` - Save requested
- `cursorChange` - Position changed

---

### Component: `FileTree.svelte`

**Location**: `frontend/src/components/FileTree.svelte` (~11KB)

**Features**:
- Hierarchical file display
- Expand/collapse directories
- Single and multi-select
- Search/filter input
- Context menu (right-click)
- Drag-drop reordering (planned)

**Events Emitted**:
- `select` - File selected
- `multiSelect` - Multiple files selected
- `open` - File double-clicked

---

### Component: `CommandPalette.svelte`

**Location**: `frontend/src/components/CommandPalette.svelte` (~17KB)

**Features**:
- Fuzzy search using Fuse.js
- Recent/frequent command suggestions
- Keyboard navigation
- AI model picker dropdown
- Dry-run toggle checkbox

**Keyboard Shortcuts**:
- `Enter` - Preview command
- `Shift+Enter` - Apply focused preview
- `Ctrl+Enter` - Apply immediately
- `Escape` - Close palette

**Command Types**:
- Built-in commands (set margins, etc.)
- AI plan requests
- File operations
- Settings toggles

---

### Component: `RightPanel.svelte`

**Location**: `frontend/src/components/RightPanel.svelte` (~22KB)

**Tabs**:
- **PDF** - PDF preview
- **Diff** - Proposed changes
- **Errors** - Compile errors/warnings
- **AI** - Suggestions and review
- **Templates** - Snippet library

**Features**:
- Tab switching
- Error click-to-line
- Diff apply/reject
- Template insertion

---

### Component: `PdfViewer.svelte`

**Location**: `frontend/src/components/PdfViewer.svelte` (~8KB)

**Features**:
- PDF rendering
- Page navigation
- Zoom controls
- SyncTeX click-to-source

---

### Component: `DiffView.svelte`

**Location**: `frontend/src/components/DiffView.svelte` (~10KB)

**Features**:
- Unified diff display
- Line-by-line highlighting
- Apply/reject per hunk
- Syntax highlighting

---

### Component: `ChatPanel.svelte`

**Location**: `frontend/src/components/ChatPanel.svelte` (~10KB)

**Features**:
- Message input field
- Message history display
- Streaming response render
- Code block formatting
- Markdown rendering

---

### Other Components

| Component | Size | Purpose |
|-----------|------|---------|
| `AgentProposal.svelte` | ~11KB | AI edit proposal cards |
| `ChatWithProposal.svelte` | ~10KB | Chat + inline diffs |
| `FileOpener.svelte` | ~7KB | Quick file open dialog |
| `HistorySidebar.svelte` | ~4KB | Edit history timeline |
| `KeyboardShortcuts.svelte` | ~4KB | Shortcut help overlay |
| `SearchResults.svelte` | ~8KB | Search results list |
| `SettingsPanel.svelte` | ~20KB | Settings configuration |
| `StatusBar.svelte` | ~2KB | Bottom status bar |
| `ThemeToggle.svelte` | ~4KB | Light/dark toggle |
| `Toasts.svelte` | ~1KB | Toast notifications |

---

## State Management

### Stores (`frontend/src/stores/`)

**fileStore.js**:
```javascript
{
  filePath: string,
  content: string,
  isDirty: boolean,
  etag: string
}
```
- `setFile(path, content)` - Load file
- `markDirty()` - Content changed
- `markSaved()` - Content saved

**compileStore.js**:
```javascript
{
  jobId: string,
  status: 'idle'|'compiling'|'success'|'error',
  progress: number,
  pdfUrl: string,
  errors: [],
  warnings: []
}
```
- `setCompileJob(id)` - Start compile
- `updateProgress(pct)` - Update progress
- `setSuccess(url)` - Compilation done
- `setError(msg)` - Compilation failed

**aiStore.js**:
```javascript
{
  sessionId: string,
  messages: [],
  isStreaming: boolean,
  proposal: null|object,
  error: null|string
}
```
- `initSession()` - New AI session
- `addMessage(role, content)` - Add chat
- `startStreaming()` - Begin stream
- `setProposal(proposal)` - Set edit proposal

---

## Configuration System

### Backend Configuration

**File**: `~/.latex-ai/.latex-ai.toml`

```toml
[keyboard_shortcuts]
save = "ctrl+s"
compile = "ctrl+enter"

[theme]
mode = "dark"
primary_color = "#007bff"

[editor]
font_size = 14
tab_size = 2
word_wrap = true
```

**Environment Variables**:
- `GEMINI_API_KEY` - AI API key
- `LATEX_AI_MAX_FILE_SIZE` - File size limit
- `LATEX_AI_WORKSPACE_ROOT` - Project root
- `LOG_LEVEL` - Logging verbosity
- `LOG_JSON` - JSON log format

---

### Frontend Configuration

**vite.config.js**:
- Dev server proxy to backend
- Build output to `src/latex_ai/static/`
- Monaco editor worker handling

**tailwind.config.js**:
- Custom color palette
- Dark mode configuration
- Typography plugin

---

## Security Implementation

### Path Traversal Prevention
- Canonical path resolution
- Workspace boundary check
- Symlink following disabled

### Shell Injection Prevention
- Block `\write18` command
- Block `\input|` piped input
- Filename character validation

### Rate Limiting
- Token bucket per session
- Global request limits
- AI endpoint throttling

### Authentication (Note)
- Currently no auth implemented
- Designed for single-user local use
- Production would need auth layer

---

## Testing Strategy

### Backend Tests (`tests/`)

**Test Types**:
- Unit tests for core modules
- Integration tests for API endpoints
- Async tests using pytest-asyncio

**Running Tests**:
```bash
pytest tests/ -v
pytest tests/ --cov=latex_ai
```

**Key Test Files**:
- `test_engine.py` - Command execution
- `test_document.py` - Document operations
- `test_api_*.py` - API endpoints
- `test_compiler.py` - PDF compilation

---

### Frontend Tests (`frontend/tests/`)

**Framework**: Vitest + Testing Library

**Running Tests**:
```bash
npm run test
npm run test:coverage
```

---

## DevOps & Deployment

### Development Workflow

```bash
# Backend
python -m venv .venv
source .venv/bin/activate  # or .venv\Scripts\activate
pip install -e ".[dev]"
python -m latex_ai serve --port 3000

# Frontend
cd frontend
npm install
npm run dev  # Proxies to :3000
```

### Production Build

```bash
# Build frontend
cd frontend && npm run build
# Frontend served from src/latex_ai/static/

# Start production server
python -m latex_ai start --port 3000
```

### Docker Deployment

**Dockerfile**:
- Python 3.9 base image
- Installs TexLive for pdflatex
- Copies built frontend
- Exposes port 3000

**docker-compose.yml**:
- Single service: latex-ai
- Volume mount for workspace
- Environment variables

---

## Common Interview Questions

### Architecture Questions

**Q: Why FastAPI over Flask?**
- Async-first design
- Automatic OpenAPI documentation
- Type hints with Pydantic
- Better WebSocket support
- Higher performance

**Q: Why Svelte over React?**
- Smaller bundle size
- No virtual DOM overhead
- Less boilerplate code
- Build-time optimization
- Reactive by default

**Q: How does the AI integration work?**
- User sends request via API
- Backend streams to Gemini API
- Response streams back via SSE
- Frontend displays incrementally
- Conversation context maintained

**Q: How is compilation handled?**
- Job submitted to async queue
- PDFLatex runs in temp directory
- Results cached by content hash
- PDF served via static route
- Errors parsed from log file

---

### Code Questions

**Q: How is path traversal prevented?**
```python
def validate_path(path: str) -> bool:
    canonical = os.path.realpath(path)
    workspace = os.path.realpath(WORKSPACE_ROOT)
    return canonical.startswith(workspace)
```

**Q: How does streaming work?**
```python
async def event_generator():
    async for chunk in ai_planner.stream():
        yield {"event": "step", "data": json.dumps(chunk)}
    yield {"event": "done", "data": ""}
return EventSourceResponse(event_generator())
```

**Q: How is undo/redo implemented?**
- Every edit saved to SQLite history
- Snapshot includes content + metadata
- Undo restores previous snapshot
- Redo tracks undone snapshots

---

### Design Pattern Questions

**Q: What design patterns are used?**
- **Factory Pattern**: `Document.load()` class method
- **Singleton Pattern**: ConfigManager, AppSettings
- **Circuit Breaker**: AI failure handling
- **Observer Pattern**: Svelte stores
- **Repository Pattern**: HistoryDB

**Q: How is concurrency handled?**
- asyncio.Semaphore for compilation
- Thread pool for CPU-bound AI calls
- asyncio.Queue for job processing
- Rate limiting per session

---

### Performance Questions

**Q: How is compilation performance optimized?**
- Content hash caching avoids recompilation
- Symlinks for large binary assets
- Concurrent compilation limiting
- Incremental compilation (planned)

**Q: How is frontend performance optimized?**
- Lazy loading of heavy components
- Virtual list for large file trees
- Debounced auto-save
- Monaco editor web worker

---

## Quick Reference

### Key File Locations

| Purpose | Path |
|---------|------|
| CLI entry | `src/latex_ai/cli.py` |
| Server setup | `src/latex_ai/server.py` |
| AI logic | `src/latex_ai/ai_planner.py` |
| PDF compile | `src/latex_ai/pdf_compiler.py` |
| API routes | `src/latex_ai/api/` |
| Main UI | `frontend/src/App.svelte` |
| Editor | `frontend/src/components/Editor.svelte` |
| State | `frontend/src/stores/` |

### Key Commands

```bash
# Development
make install && make run
npm run dev  # in frontend/

# Testing
pytest tests/ -v
npm run test

# Production
make build && python -m latex_ai start
```

---

*Last updated: 2026-01-24*
