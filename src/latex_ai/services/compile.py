"""Compilation service: queue, compile, and error analysis.

Provides both async job submission and synchronous on-demand compilation
for agent loop error recovery, with content-based caching to avoid redundant compilations.
"""
from __future__ import annotations

import asyncio
import hashlib
from pathlib import Path
from typing import Optional, Dict

from ..compilation_queue import get_compilation_queue
from ..pdf_compiler import compile_file, CompileResult


# In-memory cache for compilation results (content hash -> CompileResult)
_compilation_cache: Dict[str, CompileResult] = {}


def _compute_content_hash(content: str) -> str:
    """Compute SHA-256 hash of file content."""
    return hashlib.sha256(content.encode('utf-8')).hexdigest()


def _get_cached_result(content_hash: str) -> Optional[CompileResult]:
    """Get cached compilation result if available."""
    return _compilation_cache.get(content_hash)


def _cache_result(content_hash: str, result: CompileResult):
    """Cache compilation result. Evict old entries if cache grows too large."""
    _compilation_cache[content_hash] = result
    
    # Simple LRU: keep only 100 most recent
    if len(_compilation_cache) > 100:
        # Remove oldest (first key)
        first_key = next(iter(_compilation_cache))
        del _compilation_cache[first_key]


async def submit_compile(file_path: Path, *, output_dir: Optional[str] = None) -> str:
    """Submit a compile job and return the job id."""
    queue = get_compilation_queue()
    job = await queue.submit(file_path=file_path, options={"output_dir": output_dir})
    return job.job_id


def compile_now(tex_file: str, *, output_dir: Optional[str] = None, use_cache: bool = True) -> CompileResult:
    """Run a synchronous compile for immediate feedback with optional caching.
    
    Used by agent loop to test patches before returning to user.
    
    Args:
        tex_file: Path to .tex file
        output_dir: Optional output directory
        use_cache: Whether to use cached results (default True)
        
    Returns:
        CompileResult (possibly from cache)
    """
    # Check cache if enabled
    if use_cache:
        try:
            with open(tex_file, 'r', encoding='utf-8') as f:
                content = f.read()
            content_hash = _compute_content_hash(content)
            cached = _get_cached_result(content_hash)
            if cached:
                return cached
        except Exception:
            pass  # Fall through to actual compilation
    
    # Perform actual compilation
    result = compile_file(tex_file, output_dir)
    
    # Cache successful results
    if use_cache and result.success:
        try:
            with open(tex_file, 'r', encoding='utf-8') as f:
                content = f.read()
            content_hash = _compute_content_hash(content)
            _cache_result(content_hash, result)
        except Exception:
            pass  # Don't fail on cache errors
    
    return result


async def compile_async(tex_file: str, *, output_dir: Optional[str] = None, use_cache: bool = True) -> CompileResult:
    """Run compilation in a thread pool with optional caching.
    
    Args:
        tex_file: Path to .tex file
        output_dir: Optional output directory
        use_cache: Whether to use cached results (default True)
        
    Returns:
        CompileResult (possibly from cache)
    """
    loop = asyncio.get_event_loop()
    return await loop.run_in_executor(
        None,
        lambda: compile_now(tex_file, output_dir=output_dir, use_cache=use_cache)
    )


def extract_error_summary(result: CompileResult) -> str:
    """Extract human-readable error summary from compile result.
    
    Returns:
        Multi-line string suitable for feeding back to AI
    """
    lines = []
    
    if result.errors:
        lines.append("ERRORS:")
        for err in result.errors[:10]:  # First 10 only
            lines.append(f"  Line {err.line}: {err.message}")
    
    if result.warnings:
        lines.append("WARNINGS:")
        for warn in result.warnings[:5]:  # First 5 only
            lines.append(f"  Line {warn.line}: {warn.message}")
    
    if not lines:
        lines.append("(No errors or warnings)")
    
    return "\n".join(lines)
