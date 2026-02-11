"""Shared FastAPI dependencies and helpers for routers."""
try:
    import aiofiles
    import aiofiles.os
    HAS_AIOFILES = True
except ImportError:
    HAS_AIOFILES = False

import asyncio
import os
from pathlib import Path
from fastapi import HTTPException

from latex_ai.config import get_config
from latex_ai.security import (
    validate_file_access,
    validate_workspace_path,
    safe_resolve_path,
    check_symlink_safety,
    SecurityError,
)


def get_workspace_root() -> Path:
    """Return workspace root from config, defaulting to cwd if missing."""
    env_root = os.getenv("WORKSPACE_ROOT")
    if env_root:
        return validate_workspace_path(env_root)

    cfg = get_config()
    root = cfg.workspace_root or str(Path.cwd())
    return validate_workspace_path(root)


def validate_directory_path(path: str) -> Path:
    """Validate a directory path within the workspace with symlink checks."""
    workspace = get_workspace_root()
    resolved = safe_resolve_path(workspace, path)
    check_symlink_safety(resolved, workspace)
    if not resolved.exists():
        raise HTTPException(status_code=404, detail=f"Directory not found: {resolved}")
    if not resolved.is_dir():
        raise HTTPException(status_code=400, detail=f"Not a directory: {resolved}")
    return resolved


def validate_path(path: str, access_type: str = "read") -> Path:
    """Safely validate and resolve a file path with comprehensive security checks."""
    workspace = get_workspace_root()
    try:
        return validate_file_access(workspace, path, access_type=access_type)
    except SecurityError as exc:
        raise HTTPException(status_code=403, detail=str(exc))
    except Exception as exc:  # pragma: no cover - defensive
        raise HTTPException(status_code=400, detail=f"Invalid path: {str(exc)}")


async def read_file_with_limit(path: Path, max_size: int | None = None) -> str:
    """Read file with size limit to prevent memory exhaustion."""
    cfg = get_config()
    max_allowed = max_size if max_size is not None else cfg.max_file_size

    try:
        if HAS_AIOFILES:
            stat = await aiofiles.os.stat(path)
            if stat.st_size > max_allowed:
                size_mb = stat.st_size / (1024 * 1024)
                limit_mb = max_allowed / (1024 * 1024)
                raise HTTPException(
                    status_code=413,
                    detail=f"File too large: {size_mb:.1f}MB exceeds limit of {limit_mb:.1f}MB",
                )
            async with aiofiles.open(path, "r", encoding="utf-8") as handle:
                return await handle.read()
        else:
            # Fallback to asyncio.to_thread
            stat = await asyncio.to_thread(os.stat, path)
            if stat.st_size > max_allowed:
                size_mb = stat.st_size / (1024 * 1024)
                limit_mb = max_allowed / (1024 * 1024)
                raise HTTPException(
                    status_code=413,
                    detail=f"File too large: {size_mb:.1f}MB exceeds limit of {limit_mb:.1f}MB",
                )
            return await asyncio.to_thread(path.read_text, encoding="utf-8")
    except HTTPException:
        raise
    except Exception as exc:  # pragma: no cover - defensive
        raise HTTPException(status_code=500, detail=f"Error reading file: {str(exc)}")
