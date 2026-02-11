"""FastAPI backend server for LaTeX-AI web interface."""
from __future__ import annotations

import asyncio
import logging
import os
import time
import uuid
from contextlib import asynccontextmanager
from pathlib import Path

import fastapi.applications
from fastapi import FastAPI, Request
from fastapi.middleware.cors import CORSMiddleware
from fastapi.staticfiles import StaticFiles

from latex_ai.api import agent, compile as compile_routes, files, health, history, lsp, metrics
from latex_ai.api.agent import agent_edit_protocol_endpoint
from latex_ai.api.dependencies import get_workspace_root, validate_path
from latex_ai.api.schemas import AgentEdit, AgentRequest, AgentResponse, ConfigResponse
from latex_ai.services.files import compute_etag
from latex_ai.cache import get_cache
from latex_ai.compilation_queue import get_compilation_queue
from latex_ai.config import get_config
from latex_ai.history_db import HistoryDB
from latex_ai.state import state
from latex_ai.structured_logging import get_logger, setup_structured_logging


@asynccontextmanager
async def lifespan(app: FastAPI):
    """Manage application lifecycle."""
    log_level = os.getenv("LOG_LEVEL", "INFO")
    json_output = os.getenv("LOG_JSON", "false").lower() == "true"
    setup_structured_logging(log_level=log_level, json_output=json_output)

    logger = get_logger("server")
    logger.info("startup", version="0.2.0", env=os.getenv("ENV", "development"))

    state.startup_time = time.time()

    db_path = Path.home() / ".latex-ai" / "history.db"
    state.history_db = HistoryDB(db_path)

    cfg = get_config()
    state.compilation_semaphore = asyncio.Semaphore(cfg.max_concurrent_compilations)

    queue = get_compilation_queue()
    await queue.start()

    get_cache()

    yield

    if state.history_db:
        state.history_db.close()

    for client in state.lsp_clients.values():
        try:
            await client.stop()
        except Exception as exc:  # pragma: no cover - best effort
            logging.error(f"Error stopping LSP client: {exc}")
    state.lsp_clients.clear()

    queue = get_compilation_queue()
    await queue.stop()
    
    # Shutdown AI executor
    from .services.ai import shutdown_ai_executor
    await shutdown_ai_executor()



# Note: Previously patched FastAPI.build_middleware_stack for starlette 0.50+ compatibility.
# Now removed in favor of pinned compatible versions in pyproject.toml to avoid
# fragile monkeypatching that breaks on upgrades.


app = FastAPI(title="LaTeX-AI", lifespan=lifespan)


# CORS configuration
if os.getenv("ENV") == "production":
    allowed_origins = os.getenv("ALLOWED_ORIGINS", "").split(",")
    if allowed_origins and allowed_origins[0]:
        app.add_middleware(
            CORSMiddleware,
            allow_origins=[origin.strip() for origin in allowed_origins],
            allow_credentials=True,
            allow_methods=["GET", "POST", "PUT", "DELETE"],
            allow_headers=["Content-Type", "Authorization"],
        )
else:
    app.add_middleware(
        CORSMiddleware,
        allow_origins=["http://localhost:5173", "http://127.0.0.1:5173"],
        allow_credentials=True,
        allow_methods=["*"],
        allow_headers=["*"],
    )


# Register routers
app.include_router(files.router)
app.include_router(agent.router)
app.include_router(compile_routes.router)
app.include_router(history.router)
app.include_router(health.router)
app.include_router(lsp.router)
app.include_router(metrics.router)


# Serve static files from frontend/dist if built
static_dir = Path(__file__).parent / "static"
if static_dir.exists():
    app.mount("/", StaticFiles(directory=static_dir, html=True), name="static")
else:
    @app.get("/")
    async def root():
        return {
            "message": "LaTeX-AI Server v0.2.0",
            "status": "running",
            "frontend": "not built (run: npm run build in frontend/)",
        }


# Add request ID tracing middleware
@app.middleware("http")
async def add_request_id(request: Request, call_next):
    """Add unique request ID for distributed tracing."""
    request_id = request.headers.get("X-Request-ID", str(uuid.uuid4()))
    # Store in request state for access in route handlers
    request.state.request_id = request_id
    
    # Log request with ID
    logger = get_logger("server")
    logger.info(
        "request_started",
        request_id=request_id,
        method=request.method,
        path=str(request.url.path),
        client=request.client.host if request.client else "unknown"
    )
    
    start_time = time.time()
    response = await call_next(request)
    duration = time.time() - start_time
    
    # Add request ID to response headers
    response.headers["X-Request-ID"] = request_id
    
    # Log response with duration
    logger.info(
        "request_completed",
        request_id=request_id,
        status_code=response.status_code,
        duration_ms=round(duration * 1000, 2)
    )
    
    return response


# Add caching headers for static assets to improve frontend performance
@app.middleware("http")
async def add_cache_headers(request, call_next):
    response = await call_next(request)
    path = request.url.path or ""
    try:
        # Cache hashed assets aggressively
        if "/assets/" in path and any(path.endswith(ext) for ext in (".js", ".css", ".svg", ".png", ".jpg", ".woff", ".woff2")):
            response.headers["Cache-Control"] = "public, max-age=31536000, immutable"
        # Avoid caching HTML shell
        if path.endswith("index.html") or path == "/":
            response.headers["Cache-Control"] = "no-cache"
    except Exception:
        pass
    return response
