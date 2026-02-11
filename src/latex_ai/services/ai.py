"""AI service: agent loop, patch generation, and error recovery.

Encapsulates AI planner integration, prompt engineering for SEARCH/REPLACE patches,
and the agent loop orchestration for autonomous error fixing.
"""
from __future__ import annotations

import json
import re
import asyncio
import time
from pathlib import Path
from typing import Optional, AsyncGenerator
from dataclasses import dataclass
from concurrent.futures import ThreadPoolExecutor

from ..ai_planner import get_ai_planner, AIPlanner
from ..engine import LatexEngine
from ..document_bundle import DocumentBundle
from .files import with_file_lock, compute_etag, read_text


# Bounded executor for AI requests to prevent threadpool saturation
# Size 3: allows a few concurrent AI calls without overwhelming system
_ai_executor: Optional[ThreadPoolExecutor] = None
_ai_executor_max_workers = 3


# Circuit breaker state
_circuit_breaker_open = False
_circuit_breaker_failures = 0
_circuit_breaker_last_failure = 0.0
_circuit_breaker_threshold = 5  # Open after 5 consecutive failures
_circuit_breaker_timeout = 60.0  # Stay open for 60 seconds


class CircuitBreakerOpen(Exception):
    """Raised when circuit breaker is open (too many AI failures)."""
    pass


def _check_circuit_breaker():
    """Check if circuit breaker allows requests.
    
    Raises:
        CircuitBreakerOpen: If circuit is open due to consecutive failures
    """
    global _circuit_breaker_open, _circuit_breaker_failures, _circuit_breaker_last_failure
    
    current_time = time.time()
    
    # If circuit is open, check if timeout has passed
    if _circuit_breaker_open:
        if current_time - _circuit_breaker_last_failure >= _circuit_breaker_timeout:
            # Reset circuit breaker
            _circuit_breaker_open = False
            _circuit_breaker_failures = 0
        else:
            raise CircuitBreakerOpen(
                f"AI circuit breaker is open. Too many failures. "
                f"Retry after {int(_circuit_breaker_timeout - (current_time - _circuit_breaker_last_failure))}s"
            )


def _record_ai_success():
    """Record successful AI request."""
    global _circuit_breaker_failures, _circuit_breaker_open
    _circuit_breaker_failures = 0
    _circuit_breaker_open = False


def _record_ai_failure():
    """Record failed AI request and potentially open circuit."""
    global _circuit_breaker_failures, _circuit_breaker_open, _circuit_breaker_last_failure
    
    _circuit_breaker_failures += 1
    _circuit_breaker_last_failure = time.time()
    
    if _circuit_breaker_failures >= _circuit_breaker_threshold:
        _circuit_breaker_open = True


def get_ai_executor() -> ThreadPoolExecutor:
    """Get or create bounded thread pool executor for AI requests."""
    global _ai_executor
    if _ai_executor is None:
        _ai_executor = ThreadPoolExecutor(
            max_workers=_ai_executor_max_workers,
            thread_name_prefix="ai_worker"
        )
    return _ai_executor


async def shutdown_ai_executor():
    """Shutdown AI executor gracefully."""
    global _ai_executor
    if _ai_executor is not None:
        _ai_executor.shutdown(wait=True, cancel_futures=True)
        _ai_executor = None


@dataclass
class PatchBlock:
    """A SEARCH/REPLACE patch block."""
    search: str
    replace: str
    
    @staticmethod
    def parse_blocks(text: str) -> list[PatchBlock]:
        """Parse SEARCH/REPLACE blocks from LLM response.
        
        Expected format:
        <<<<<<< SEARCH
        original text here
        can be multiline
        =======
        replacement text here
        can be multiline
        >>>>>>> REPLACE
        """
        blocks = []
        pattern = r"<<<<<<< SEARCH\n(.*?)\n=======\n(.*?)\n>>>>>>> REPLACE"
        for match in re.finditer(pattern, text, re.DOTALL):
            search_text = match.group(1)
            replace_text = match.group(2)
            blocks.append(PatchBlock(search=search_text, replace=replace_text))
        return blocks


def resolve_planner(model: Optional[str] = None) -> AIPlanner:
    """Get an AI planner instance with optional model override."""
    return get_ai_planner(model=model)


async def get_agent_context(file_path: Path, task_type: str = "full", error_info: Optional[dict] = None) -> dict:
    """Load bundle context with AI context pruning for efficiency.
    
    Args:
        file_path: Path to main LaTeX file
        task_type: Type of task - "full", "error_fix", or "suggest"
        error_info: Optional error information with 'line' key for error_fix tasks
        
    Returns:
        Dict with context data optimized for the task type
    """
    lock = get_file_lock(file_path)
    async with lock:
        try:
            bundle = DocumentBundle(file_path)
            await bundle.load_async()
            
            # AI context pruning: send minimal context for error fixes
            if task_type == "error_fix" and error_info and "line" in error_info:
                main_content = bundle.files.get(str(file_path), "")
                error_line = error_info["line"]
                
                # Extract ±50 lines around error
                lines = main_content.split('\n')
                start = max(0, error_line - 50)
                end = min(len(lines), error_line + 50)
                context_lines = '\n'.join(lines[start:end])
                
                pruned_content = f"""Error at line {error_line} (showing lines {start+1}-{end}):
{context_lines}

Dependencies: {', '.join(bundle.dependencies.keys())}"""
                
                return {
                    "main_file": str(file_path),
                    "files": bundle.files,
                    "dependencies": bundle.dependencies,
                    "combined_text": pruned_content,
                }
            
            elif task_type == "suggest":
                # Only send main file + dependency list for suggestions
                main_content = bundle.files.get(str(file_path), "")
                dep_summary = f"Dependencies: {list(bundle.dependencies.keys())}"
                
                return {
                    "main_file": str(file_path),
                    "files": bundle.files,
                    "dependencies": bundle.dependencies,
                    "combined_text": f"Main file:\n{main_content}\n\n{dep_summary}",
                }
            
            # Full bundle for comprehensive tasks
            return {
                "main_file": str(file_path),
                "files": bundle.files,
                "dependencies": bundle.dependencies,
                "combined_text": bundle.get_combined_text(),
            }
        except Exception:
            # Fallback: just read the main file
            try:
                content = await read_text(file_path)
                return {
                    "main_file": str(file_path),
                    "files": {str(file_path): content},
                    "dependencies": [],
                    "combined_text": content,
                }
            except Exception:
                return {}


async def generate_patch(
    planner: AIPlanner,
    file_path: Path,
    instruction: str,
    error_context: Optional[str] = None,
    max_retries: int = 3,
    retry_delay: float = 1.0,
    timeout: float = 60.0,
) -> str:
    """Generate a SEARCH/REPLACE patch from the AI with retry logic.
    
    Args:
        planner: AIPlanner instance
        file_path: Path to file to edit
        instruction: User instruction (e.g., "Fix the abstract overflow")
        error_context: Optional compilation error details
        max_retries: Maximum number of retry attempts
        retry_delay: Initial delay between retries (exponential backoff)
        timeout: Timeout per AI request in seconds
        
    Returns:
        Raw LLM response containing SEARCH/REPLACE blocks
        
    Raises:
        CircuitBreakerOpen: If AI service has too many failures
        Exception: After exhausting all retries
    """
    # Check circuit breaker
    _check_circuit_breaker()
    
    # Get full context
    context_data = await get_agent_context(file_path)
    combined_text = context_data.get("combined_text", "")
    
    # Build prompt for patch generation
    prompt = _create_patch_prompt(combined_text, instruction, error_context)
    
    # Retry logic with exponential backoff
    last_error = None
    executor = get_ai_executor()
    
    for attempt in range(max_retries):
        try:
            # Get LLM response via planner with bounded executor and timeout
            response = await asyncio.wait_for(
                asyncio.get_event_loop().run_in_executor(
                    executor,
                    lambda: planner.stream_generate(prompt)
                ),
                timeout=timeout
            )
            # Success - reset circuit breaker
            _record_ai_success()
            return response
        except asyncio.TimeoutError:
            last_error = TimeoutError(f"AI request timed out after {timeout}s")
            _record_ai_failure()
            if attempt < max_retries - 1:
                wait_time = retry_delay * (2 ** attempt)
                await asyncio.sleep(wait_time)
            else:
                raise Exception(f"AI request failed after {max_retries} attempts: {last_error}") from last_error
        except Exception as e:
            last_error = e
            _record_ai_failure()
            if attempt < max_retries - 1:
                wait_time = retry_delay * (2 ** attempt)  # Exponential backoff
                await asyncio.sleep(wait_time)
            else:
                # Last attempt failed, raise the error
                raise Exception(f"AI request failed after {max_retries} attempts: {last_error}") from last_error


async def generate_patch_stream(
    planner: AIPlanner,
    file_path: Path,
    instruction: str,
    error_context: Optional[str] = None,
) -> AsyncGenerator[str, None]:
    """Stream patch generation tokens as they arrive.
    
    Yields:
        Individual token strings from the LLM
    """
    # Get full context
    context_data = await get_agent_context(file_path)
    combined_text = context_data.get("combined_text", "")
    
    # Build prompt for patch generation
    prompt = _create_patch_prompt(combined_text, instruction, error_context)
    
    # Stream tokens from planner
    loop = asyncio.get_event_loop()
    async for token in planner.stream_tokens(prompt):
        yield token


async def apply_patch_blocks(
    file_path: Path,
    blocks: list[PatchBlock],
) -> tuple[bool, str]:
    """Apply SEARCH/REPLACE blocks to a file.
    
    Args:
        file_path: File to patch
        blocks: List of PatchBlock to apply
        
    Returns:
        (success: bool, updated_content: str)
    """
    async def _apply():
        content = await read_text(file_path)
        
        for block in blocks:
            if block.search not in content:
                return False, content  # Search text not found
            content = content.replace(block.search, block.replace, 1)
        
        return True, content
    
    return await with_file_lock(file_path, _apply)


# Shared agent edit application and proposal helpers
from typing import List, Dict, Optional
import difflib

try:
    import aiofiles.os
    HAS_AIOFILES = True
except ImportError:
    HAS_AIOFILES = False

from latex_ai.api.schemas import AgentEdit
from latex_ai.api.dependencies import validate_path
from latex_ai.services.files import get_file_lock, save_with_etag_history_cache


async def _apply_single_edit(old_content: str, edit: AgentEdit) -> str:
    """Apply a single edit operation to content, supporting typed fields and legacy payload."""
    lines = old_content.splitlines(keepends=True)

    if edit.op == "rewrite_file":
        return edit.payload or (edit.text or "")

    if edit.op == "replace_range":
        if edit.start_line is not None and edit.end_line is not None:
            start_idx = max(0, int(edit.start_line) - 1)
            end_idx = max(start_idx, int(edit.end_line))
            new_text = edit.text if edit.text is not None else ""
        else:
            # Legacy payload format: "start,end:new_text"
            try:
                range_part, new_text = edit.payload.split(":", 1)
                start_line, end_line = map(int, range_part.split(","))
                start_idx = max(0, start_line - 1)
                end_idx = max(start_idx, end_line)
            except Exception:
                return old_content
        replacement = (new_text + ("\n" if not new_text.endswith("\n") else "")).splitlines(keepends=True)
        lines[start_idx:end_idx] = replacement
        return "".join(lines)

    if edit.op == "insert":
        if edit.line is not None:
            insert_idx = max(0, int(edit.line) - 1)
            insert_text = edit.insert_text if edit.insert_text is not None else (edit.text or "")
        else:
            try:
                line_num_str, insert_text = edit.payload.split(":", 1)
                insert_idx = max(0, int(line_num_str) - 1)
            except Exception:
                return old_content
        lines.insert(insert_idx, insert_text + ("\n" if not insert_text.endswith("\n") else ""))
        return "".join(lines)

    if edit.op in ("apply_dmp_patch", "apply_unified_diff"):
        from .patch_utils import apply_semantic_patch
        return apply_semantic_patch(old_content, edit.payload)

    return old_content


async def apply_agent_edits(
    edits: List[AgentEdit],
    *,
    history_db,
    auto_compile: bool = False,
    main_file: Optional[Path] = None,
) -> Dict:
    """Apply a list of AgentEdit operations with ETag safety and diffs.

    Returns dict with keys: success, applied, diffs, conflicts, compile_result.
    """
    diffs: Dict[str, str] = {}
    applied = 0
    conflicts: List[Dict] = []

    for edit in edits:
        edit_path = validate_path(edit.file_path, access_type="write")
        lock = get_file_lock(edit_path)
        async with lock:
            if HAS_AIOFILES:
                exists = await aiofiles.os.path.exists(str(edit_path))
            else:
                exists = await asyncio.to_thread(edit_path.exists)
            old_content = await read_text(edit_path) if exists else ""

            current_etag = compute_etag(old_content) if exists else None
            if edit.expected_etag and current_etag != edit.expected_etag:
                conflicts.append({
                    "file": edit.file_path,
                    "expected_etag": edit.expected_etag,
                    "current_etag": current_etag,
                })
                continue

            new_content = await _apply_single_edit(old_content, edit)
            if new_content == old_content:
                continue

            save_result = await save_with_etag_history_cache(
                edit_path,
                new_content,
                if_match=None,
                history_db=history_db,
                history_command="ai-agent-apply",
                assume_locked=True,
            )

            if not save_result.get("success", False):
                conflicts.append({
                    "file": edit.file_path,
                    "error": save_result.get("error", "unknown"),
                    "conflict": save_result.get("conflict"),
                })
                continue

            diff_text = "\n".join(difflib.unified_diff(
                old_content.splitlines(),
                new_content.splitlines(),
                fromfile=validate_path(edit.file_path, "read").name,
                tofile=validate_path(edit.file_path, "read").name,
                lineterm="",
            ))
            diffs[edit.file_path] = diff_text
            applied += 1

    compile_result = None
    if auto_compile and main_file is not None:
        try:
            from .compile import compile_async, extract_error_summary
            cr = await compile_async(str(main_file))
            compile_result = {
                "success": cr.success,
                "page_count": cr.page_count if cr.success else 0,
                "error_count": len(cr.errors),
                "warning_count": len(cr.warnings),
                "errors": [
                    {"line": e.line, "file": e.file, "message": e.message}
                    for e in (cr.errors or [])
                ] if not cr.success else None,
                "warnings": [
                    {"line": w.line, "file": w.file, "message": w.message}
                    for w in (cr.warnings or [])
                ] if cr.warnings else None,
                "summary": extract_error_summary(cr) if not cr.success else None,
            }
        except Exception as e:
            compile_result = {"error": str(e)}

    return {
        "success": len(conflicts) == 0,
        "applied": applied,
        "diffs": diffs,
        "conflicts": conflicts,
        "compile_result": compile_result,
    }


async def compute_proposal_diffs(
    bundle: DocumentBundle,
    edits: List[AgentEdit],
) -> Dict[str, Dict[str, str]]:
    """Compute before/after/diff for proposed edits without applying them."""
    diffs: Dict[str, Dict[str, str]] = {}
    for edit in edits:
        try:
            path = validate_path(edit.file_path, access_type="read")
            if HAS_AIOFILES:
                exists = await aiofiles.os.path.exists(str(path))
            else:
                exists = await asyncio.to_thread(path.exists)
            old_content = await read_text(path) if exists else ""
            new_content = await _apply_single_edit(old_content, edit)
            diff_lines = list(difflib.unified_diff(
                old_content.splitlines(keepends=True),
                new_content.splitlines(keepends=True),
                fromfile=f"a/{path.name}",
                tofile=f"b/{path.name}",
                lineterm="",
            ))
            diffs[edit.file_path] = {
                "before": old_content,
                "after": new_content,
                "diff": "\n".join(diff_lines),
            }
        except Exception as e:
            diffs[edit.file_path] = {"error": str(e)}
    return diffs


async def capture_etags(edits: List[AgentEdit]) -> Dict[str, str]:
    """Capture current ETags for files referenced by edits."""
    etags: Dict[str, str] = {}
    for edit in edits:
        try:
            path = validate_path(edit.file_path, access_type="read")
            if HAS_AIOFILES:
                exists = await aiofiles.os.path.exists(str(path))
            else:
                exists = await asyncio.to_thread(path.exists)
            if exists:
                content = await read_text(path)
                etag = compute_etag(content)
                etags[edit.file_path] = etag
        except Exception:
            continue
    return etags


def generate_fix_instruction(errors: List[Dict]) -> str:
    """Generate a high-level instruction string from compile errors."""
    if not errors:
        return "Compilation succeeded"
    lines = []
    for i, err in enumerate(errors[:5], 1):
        line = err.get("line", "?")
        msg = err.get("message", "unknown")
        lines.append(f"{i}. Line {line}: {msg}")
    if len(errors) > 5:
        lines.append(f"... and {len(errors) - 5} more errors")
    return "Fix the following LaTeX compilation errors:\n" + "\n".join(lines)


def _create_patch_prompt(
    context: str,
    instruction: str,
    error_context: Optional[str] = None,
) -> str:
    """Create a prompt that asks for SEARCH/REPLACE blocks instead of commands."""
    error_section = ""
    if error_context:
        error_section = f"""
COMPILATION ERRORS TO FIX:
{error_context}
"""
    
    return f"""You are an expert LaTeX editor. Your task is to generate precise SEARCH/REPLACE patches.

{context}

USER INSTRUCTION: {instruction}
{error_section}

Generate SEARCH/REPLACE blocks in this exact format:

<<<<<<< SEARCH
[exact original text, multiline if needed]
=======
[exact replacement text, multiline if needed]
>>>>>>> REPLACE

<<<<<<< SEARCH
[next block original text]
=======
[next block replacement text]
>>>>>>> REPLACE

Important:
- The SEARCH text must match EXACTLY what's in the file (including whitespace)
- Generate 1-5 blocks max
- If you can't generate patches, output: NO PATCHES

Now generate the patches:"""

