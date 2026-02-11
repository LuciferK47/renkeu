"""File service utilities: locking, atomic writes, ETag, history, cache.

This module centralizes common file operations to ensure consistent behavior
and avoids circular imports with the API layer.
"""
from __future__ import annotations

try:
    import aiofiles
    import aiofiles.os
    HAS_AIOFILES = True
except ImportError:
    HAS_AIOFILES = False

import asyncio
import hashlib
import os
from pathlib import Path
from typing import Optional, Callable

from ..cache import get_cache


# Async helpers with aiofiles fallback
async def _aread_text(path: Path) -> str:
    """Read file asynchronously, with fallback to asyncio.to_thread."""
    if HAS_AIOFILES:
        async with aiofiles.open(path, mode="r", encoding="utf-8") as f:
            return await f.read()
    else:
        return await asyncio.to_thread(path.read_text, encoding="utf-8")


async def _awrite_text(path: Path, content: str) -> None:
    """Write file asynchronously, with fallback to asyncio.to_thread."""
    if HAS_AIOFILES:
        async with aiofiles.open(path, mode="w", encoding="utf-8") as f:
            await f.write(content)
    else:
        await asyncio.to_thread(path.write_text, content, encoding="utf-8")


async def _amkdirs(path: str) -> None:
    """Create directories asynchronously, with fallback to asyncio.to_thread."""
    if HAS_AIOFILES:
        await aiofiles.os.makedirs(path, exist_ok=True)
    else:
        await asyncio.to_thread(os.makedirs, path, exist_ok=True)


async def _areplace(src: str, dst: str) -> None:
    """Replace file asynchronously, with fallback to asyncio.to_thread."""
    if HAS_AIOFILES:
        await aiofiles.os.replace(src, dst)
    else:
        await asyncio.to_thread(os.replace, src, dst)


async def _aexists(path: str) -> bool:
    """Check file exists asynchronously, with fallback to asyncio.to_thread."""
    if HAS_AIOFILES:
        return await aiofiles.os.path.exists(path)
    else:
        return await asyncio.to_thread(os.path.exists, path)


async def _astat(path: Path):
    """Get file stat asynchronously.
    
    Returns an object with at least st_size and st_mtime attributes.
    """
    if HAS_AIOFILES:
        # aiofiles exposes os.stat via aiofiles.os.stat
        return await aiofiles.os.stat(str(path))
    else:
        return await asyncio.to_thread(path.stat)


# Per-file asyncio locks shared across server operations
_file_locks: dict[str, asyncio.Lock] = {}


def get_file_lock(file_path: Path) -> asyncio.Lock:
    """Get or create asyncio lock for a file path."""
    key = str(file_path)
    if key not in _file_locks:
        _file_locks[key] = asyncio.Lock()
    return _file_locks[key]


def compute_etag(content: str) -> str:
    """Compute a short SHA-256-based ETag for content."""
    return hashlib.sha256(content.encode("utf-8")).hexdigest()[:16]


def compute_file_size(content: str) -> tuple[int, str]:
    """Compute file size in bytes and human-readable format.
    
    Args:
        content: File content string
        
    Returns:
        Tuple of (size_bytes, human_readable_size)
    """
    size_bytes = len(content.encode("utf-8"))
    if size_bytes < 1024:
        human_size = f"{size_bytes} B"
    elif size_bytes < 1024 * 1024:
        human_size = f"{size_bytes / 1024:.1f} KB"
    else:
        human_size = f"{size_bytes / (1024 * 1024):.1f} MB"
    return size_bytes, human_size


async def read_text(path: Path) -> str:
    return await _aread_text(path)


async def atomic_write(path: Path, content: str) -> None:
    """Atomic write via temp rename to avoid corruption on Windows/POSIX."""
    tmp_path = path.with_suffix(path.suffix + ".tmp")
    await _amkdirs(str(path.parent))
    await _awrite_text(tmp_path, content)
    await _areplace(str(tmp_path), str(path))


async def with_file_lock(path: Path, coro: Callable[[], object]):
    """Run a coroutine while holding the per-file asyncio lock."""
    lock = get_file_lock(path)
    async with lock:
        return await coro()


async def save_with_etag_history_cache(
    path: Path,
    new_content: str,
    *,
    if_match: Optional[str] = None,
    history_db=None,
    history_command: str = "manual-save",
    assume_locked: bool = False,
) -> dict:
    """Save a file under a lock with ETag conflict detection and history.

    Returns a dict with keys: success(bool), status(int), etag(str),
    conflict(optional dict when 409 occurs)
    """
    async def _op():
        # Read current
        old_content = ""
        exists = await _aexists(str(path))
        if exists:
            old_content = await _aread_text(path)
        current_etag = compute_etag(old_content) if exists else None

        # ETag conflict
        if if_match and current_etag != if_match:
            return {
                "success": False,
                "status": 409,
                "conflict": {
                    "current_content": old_content,
                    "current_etag": current_etag,
                },
            }

        # Write atomically
        await atomic_write(path, new_content)

        # Invalidate file list cache for directory
        cache = get_cache()
        await cache.invalidate_file_list(str(path.parent))

        # History (best-effort)
        if history_db and old_content != new_content:
            try:
                from difflib import unified_diff
                diff = "".join(
                    unified_diff(
                        old_content.splitlines(keepends=True),
                        new_content.splitlines(keepends=True),
                        fromfile=str(path),
                        tofile=str(path),
                    )
                )
                loop = asyncio.get_running_loop()
                await loop.run_in_executor(
                    None,
                    lambda: history_db.add_entry(str(path), history_command, new_content, diff),
                )
            except Exception:
                pass

        return {
            "success": True,
            "status": 200,
            "etag": compute_etag(new_content),
        }

    # Avoid nested lock deadlocks: if caller already holds the lock, run directly
    if assume_locked:
        return await _op()
    return await with_file_lock(path, _op)
