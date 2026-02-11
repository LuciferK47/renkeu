"""Health endpoints for API monitoring."""
from __future__ import annotations

import logging
import time
import shutil
from pathlib import Path

from fastapi import APIRouter

from latex_ai.ai_planner import get_ai_planner
from latex_ai.state import state
from latex_ai.api.dependencies import get_workspace_root
from latex_ai.compilation_queue import get_compilation_queue

router = APIRouter()


@router.get("/health")
@router.get("/api/health")
async def health_check():
    """Unified health endpoint (accessible at /health and /api/health)."""
    uptime = time.time() - state.startup_time
    db_healthy = False
    try:
        if state.history_db:
            state.history_db.get_recent_entries(limit=1)
            db_healthy = True
    except Exception as exc:
        logging.error(f"Database health check failed: {exc}")

    ai_healthy = False
    circuit_breaker_open = False
    try:
        planner = get_ai_planner()
        ai_healthy = planner is not None
        
        # Check circuit breaker status
    circuit_breaker_state = {}
    try:
        from latex_ai.services.ai import (
            _circuit_breaker_open,
            _circuit_breaker_failures,
            _circuit_breaker_last_failure
        )
        circuit_breaker_open = _circuit_breaker_open
        circuit_breaker_state = {
            "open": _circuit_breaker_open,
            "failures": _circuit_breaker_failures,
            "last_failure_time": _circuit_breaker_last_failure
        }
    except ImportError:
        pass
    except Exception as exc:
        logging.error(f"AI health check failed: {exc}")
    
    # Check disk space
    disk_healthy = True
    disk_free_gb = 0
    try:
        root = get_workspace_root()
        disk_usage = shutil.disk_usage(root)
        disk_free_gb = disk_usage.free / (1024**3)  # Convert to GB
        disk_healthy = disk_free_gb > 1.0  # At least 1GB free
    except Exception:
        disk_healthy = False

    try:
        from latex_ai.api.lsp import LSP_AVAILABLE
    except Exception:
        LSP_AVAILABLE = False

    status = "healthy" if (db_healthy and ai_healthy and disk_healthy and not circuit_breaker_open) else "degraded"
    return {
        "status": status,
        "version": "0.2.0",
        "uptime_seconds": uptime,
        "checks": {
            "database": "ok" if db_healthy else "error",
            "ai_service": "ok" if ai_healthy else "error",
            "circuit_breaker": circuit_breaker_state if circuit_breaker_state else (
                "open" if circuit_breaker_open else "closed"
            ),
            "disk_space_gb": round(disk_free_gb, 2),
            "disk_space_ok": disk_healthy,
            "lsp_available": bool(LSP_AVAILABLE),
        },
        "timestamp": time.time(),
    }


@router.get("/api/health/deep")
async def deep_health_check():
    """Deeper health check verifying filesystem write, compilation queue, and DB access."""
    uptime = time.time() - state.startup_time
    checks = {
        "database": "error",
        "filesystem": "error",
        "compile_queue": "error",
        "ai_service": "error",
    }
    # DB check
    try:
        if state.history_db:
            state.history_db.get_recent_entries(limit=1)
            checks["database"] = "ok"
    except Exception:
        checks["database"] = "error"
    # Filesystem write check in workspace root
    try:
        root = get_workspace_root()
        probe = root / ".health_probe.tmp"
        probe.write_text("ok", encoding="utf-8")
        probe.unlink(missing_ok=True)
        checks["filesystem"] = "ok"
    except Exception:
        checks["filesystem"] = "error"
    # Compilation queue check
    try:
        queue = get_compilation_queue()
        # Basic signal: ensure queue is initialized and worker started
        checks["compile_queue"] = "ok" if queue is not None else "error"
    except Exception:
        checks["compile_queue"] = "error"
    # AI planner available
    try:
        planner = get_ai_planner()
        checks["ai_service"] = "ok" if planner is not None else "error"
    except Exception:
        checks["ai_service"] = "error"
    status = "healthy" if all(v == "ok" for v in checks.values()) else "degraded"
    return {
        "status": status,
        "version": "0.2.0",
        "uptime_seconds": uptime,
        "checks": checks,
        "timestamp": time.time(),
    }
