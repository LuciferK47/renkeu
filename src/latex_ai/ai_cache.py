"""LaTeX AI cache logic for plan caching and content hashing."""

import hashlib
import json
import logging
from pathlib import Path
from typing import Optional

logger = logging.getLogger(__name__)


def hash_content(content: str) -> str:
    """Generate SHA256 hash of content.

    Args:
        content: Content to hash

    Returns:
        Hex digest of SHA256 hash
    """
    return hashlib.sha256(content.encode()).hexdigest()


def compute_plan_cache_key(file_hash: str, model_key: str, request: str) -> str:
    """Compute a cache key for an AI plan request.

    Args:
        file_hash: SHA256 hash of the file content
        model_key: Model name identifier
        request: User's natural language request

    Returns:
        Cache key string in format "file_hash:model_key:request_hash"
    """
    request_hash = hashlib.sha256(request.encode()).hexdigest()
    return f"{file_hash}:{model_key}:{request_hash}"


def _get_history_db():
    """Get HistoryDB instance (internal helper to avoid circular imports)."""
    from .history_db import HistoryDB

    db_path = Path.home() / ".latex-ai" / "history.db"
    return HistoryDB(db_path)


def get_cached_plan(cache_key: str) -> Optional[str]:
    """Retrieve a cached plan JSON string by cache key.

    Args:
        cache_key: Cache key from compute_plan_cache_key

    Returns:
        Cached plan JSON string or None if not found/invalid
    """
    db = _get_history_db()
    cached_json = db.get_ai_cache(cache_key)
    return cached_json


def store_cached_plan(cache_key: str, plan_json: str, ttl: int = 3600) -> None:
    """Store a plan in the persistent cache.

    Args:
        cache_key: Cache key from compute_plan_cache_key
        plan_json: Serialized plan JSON string
        ttl: Time-to-live in seconds (default 1 hour)
    """
    db = _get_history_db()
    db.set_ai_cache(cache_key, plan_json, ttl=ttl)


def invalidate_file_cache(file_path: str, file_hashes: dict, cache: dict) -> None:
    """Invalidate all cache entries for a file.

    Call this when a file is edited to clear stale cached plans.

    Args:
        file_path: Path to file that was edited
        file_hashes: Dict mapping file_path -> hash (class-level _file_hashes)
        cache: Dict of cached plans (class-level _cache)
    """
    old_hash = file_hashes.get(file_path)
    if old_hash is None:
        return

    # Remove all cache entries that used this file's old hash
    to_remove = []
    for cache_key in cache.keys():
        # cache_key format: "file_hash:model:request_hash"
        cache_file_hash = cache_key.split(':')[0]
        if cache_file_hash == old_hash:
            to_remove.append(cache_key)

    for key in to_remove:
        del cache[key]
        logger.debug(f"Invalidated cache for {file_path}")


def clear_cache(file_hashes: dict, cache: dict) -> None:
    """Clear all cached plans.

    Args:
        file_hashes: Dict mapping file_path -> hash (class-level _file_hashes)
        cache: Dict of cached plans (class-level _cache)
    """
    cache.clear()
    file_hashes.clear()
    logger.info("Cleared AI planner cache")
