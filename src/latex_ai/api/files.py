"""File and configuration related API routes."""
from __future__ import annotations

try:
    import aiofiles
    import aiofiles.os
    HAS_AIOFILES = True
except ImportError:
    HAS_AIOFILES = False

import json
import os
from pathlib import Path
from typing import List, Optional

from fastapi import APIRouter, Header, HTTPException
from fastapi.concurrency import run_in_threadpool
from fastapi.responses import JSONResponse

from latex_ai.api.dependencies import (
    get_workspace_root,
    read_file_with_limit,
    validate_directory_path,
    validate_path,
)
from latex_ai.api.schemas import (
    ApplyRequest,
    ApplyResponse,
    BulkApplyRequest,
    BulkApplyResponse,
    ConfigResponse,
    HistoryResponse,
    ListFilesResponse,
    ModelsResponse,
    PaginatedHistoryResponse,
    PlanRequest,
    PreviewResponse,
    RestoreRequest,
    SaveFileRequest,
    SecurityInfoResponse,
)
from latex_ai.cache import get_cache
from latex_ai.config import get_config, get_config_manager
from latex_ai.engine import LatexEngine
from latex_ai.history_db import HistoryDB
from latex_ai.models_registry import AVAILABLE_MODELS, AVAILABLE_MODEL_IDS
from latex_ai.security import ALLOWED_EXTENSIONS, BLOCKED_EXTENSIONS
from latex_ai.services.files import (
    compute_etag,
    get_file_lock,
    save_with_etag_history_cache,
    with_file_lock,
    read_text,
    _aread_text,
    _astat,
    compute_file_size,
)
from latex_ai.state import state

router = APIRouter()


@router.post("/api/apply", response_model=ApplyResponse)
async def apply_endpoint(req: ApplyRequest) -> ApplyResponse:
    try:
        file_path = validate_path(req.file_path, access_type="write")
        exists = await run_in_threadpool(file_path.exists)
        if not exists:
            raise HTTPException(status_code=404, detail=f"File not found: {file_path}")

        lock = get_file_lock(file_path)
        async with lock:
            engine = await run_in_threadpool(LatexEngine, file_path)
            original_content = engine.text

            if req.if_match:
                current_etag = compute_etag(original_content)
                if current_etag != req.if_match:
                    raise HTTPException(
                        status_code=409,
                        detail={
                            "error": "ETag mismatch: file changed since baseline",
                            "current_etag": current_etag,
                            "expected_etag": req.if_match,
                            "current_content": original_content,
                        },
                    )

            cache = get_cache()
            content_hash = cache.get_content_hash(original_content)
            cached_result = await cache.get_cached_diff(content_hash, req.command)

            if cached_result and req.dry_run:
                return ApplyResponse(
                    success=True,
                    message=cached_result.get("message", "Applied from cache"),
                    changed=cached_result.get("changed", True),
                    diff=cached_result.get("diff", ""),
                )

            result = await run_in_threadpool(engine.apply_command, req.command, False)

            if not result.changed:
                return ApplyResponse(success=True, message=result.message, changed=False)

            diff = await run_in_threadpool(engine.compute_diff, result.new_text) if result.new_text else ""

            await cache.cache_diff(
                content_hash,
                req.command,
                {"message": result.message, "changed": True, "diff": diff},
            )

            if req.dry_run:
                return ApplyResponse(success=True, message=result.message, changed=True, diff=diff)

            engine.text = result.new_text
            backup_path = await run_in_threadpool(engine.save, True)

            history_id = None
            if state.history_db:
                history_id = await run_in_threadpool(
                    state.history_db.add_entry,
                    str(file_path),
                    req.command,
                    result.new_text,
                    diff,
                    edit_type="manual-apply",
                )

            return ApplyResponse(
                success=True,
                message=result.message,
                changed=True,
                diff=diff,
                backup_path=backup_path.name if backup_path else None,
                history_id=history_id,
            )

    except HTTPException:
        raise
    except Exception as exc:
        raise HTTPException(status_code=400, detail=f"Error: {str(exc)}")


@router.get("/api/models", response_model=ModelsResponse)
async def get_models_endpoint() -> ModelsResponse:
    cfg = get_config()
    return ModelsResponse(current=getattr(cfg, "ai_model_default", "gpt-5.1-codex-max"), available=AVAILABLE_MODELS)


@router.get("/api/security", response_model=SecurityInfoResponse)
async def get_security_info() -> SecurityInfoResponse:
    cfg = get_config()
    return SecurityInfoResponse(
        allowed_extensions=sorted(list(ALLOWED_EXTENSIONS)),
        blocked_extensions=sorted(list(BLOCKED_EXTENSIONS)),
        max_file_size_mb=cfg.max_file_size // (1024 * 1024),
    )


@router.get("/api/search")
async def search_content(query: str, file_pattern: str = "*.tex", case_sensitive: bool = False, limit: int = 50):
    import re

    try:
        workspace = get_workspace_root()

        def search_files_sync():
            file_results = []
            for file_path in workspace.rglob(file_pattern):
                if not file_path.is_file():
                    continue
                if any(part.startswith(".") or part == "__pycache__" for part in file_path.parts):
                    continue
                try:
                    with open(file_path, "r", encoding="utf-8") as handle:
                        content = handle.read()
                    matches = []
                    pattern = re.compile(query, flags=0 if case_sensitive else re.IGNORECASE)
                    for index, line in enumerate(content.splitlines(), 1):
                        if pattern.search(line):
                            matches.append({"line_number": index, "line_content": line.strip()[:200]})
                            if len(matches) >= 10:
                                break
                    if matches:
                        rel_path = file_path.relative_to(workspace)
                        file_results.append({
                            "file": str(rel_path).replace("\\", "/"),
                            "matches": matches,
                        })
                        if len(file_results) >= limit:
                            break
                except (OSError, UnicodeDecodeError, re.error):
                    continue
            return file_results

        results = await run_in_threadpool(search_files_sync)
        return {"query": query, "total_files": len(results), "results": results}
    except Exception as exc:
        raise HTTPException(status_code=400, detail=f"Search error: {str(exc)}")


@router.get("/api/preview", response_model=PreviewResponse)
async def preview_endpoint(file_path: str) -> PreviewResponse:
    try:
        file_path_obj = validate_path(file_path)
        exists = await run_in_threadpool(file_path_obj.exists)
        if not exists:
            raise HTTPException(status_code=404, detail=f"File not found: {file_path}")

        engine = await run_in_threadpool(LatexEngine, file_path_obj)
        info = await run_in_threadpool(engine.preview)

        packages: List[str] = []
        if hasattr(engine.document, "get_packages"):
            try:
                packages = engine.document.get_packages()
            except Exception:
                pass

        return PreviewResponse(
            file_path=file_path,
            documentclass=info.get("documentclass", ""),
            geometry=info.get("geometry", ""),
            packages=packages,
        )
    except HTTPException:
        raise
    except Exception as exc:
        raise HTTPException(status_code=400, detail=f"Error: {str(exc)}")


@router.get("/api/history", response_model=PaginatedHistoryResponse)
async def history_endpoint(path: str, limit: int = 50, offset: int = 0) -> PaginatedHistoryResponse:
    """Fetch paginated edit history for a file.
    
    Args:
        path: File path to get history for
        limit: Maximum entries per page (default: 50, max: 100)
        offset: Number of entries to skip (for pagination)
        
    Returns:
        PaginatedHistoryResponse with items and metadata
    """
    try:
        if not state.history_db:
            return PaginatedHistoryResponse(items=[], limit=limit, offset=offset, total=0, has_more=False)
        
        # Clamp limit to prevent excessive loading
        limit = min(limit, 100)
        limit = max(limit, 1)
        offset = max(offset, 0)
        
        entries = await run_in_threadpool(state.history_db.get_history, path, limit, offset)
        total = await run_in_threadpool(state.history_db.count_history, path)
        
        history_items = [
            HistoryResponse(id=entry.id, command=entry.command, timestamp=entry.timestamp.isoformat(), diff=entry.diff)
            for entry in entries
        ]
        
        has_more = (offset + len(entries)) < total
        
        return PaginatedHistoryResponse(
            items=history_items,
            limit=limit,
            offset=offset,
            total=total,
            has_more=has_more
        )
    except Exception as exc:
        raise HTTPException(status_code=400, detail=f"Error: {str(exc)}")


@router.get("/api/history/agent", response_model=List[HistoryResponse])
async def agent_history_endpoint(path: str, limit: int = 20) -> List[HistoryResponse]:
    """Return recent agent-applied history entries for a file.

    Filters to commands produced by agent application flows so the frontend
    can show redo candidates without manual filtering client-side.
    """
    try:
        if not state.history_db:
            return []
        entries = await run_in_threadpool(state.history_db.get_history, path, limit, 0)
        agent_commands = {"ai-agent-apply", "agent-apply"}
        filtered = [e for e in entries if e.command in agent_commands]
        return [
            HistoryResponse(id=entry.id, command=entry.command, timestamp=entry.timestamp.isoformat(), diff=entry.diff)
            for entry in filtered
        ]
    except Exception as exc:
        raise HTTPException(status_code=400, detail=f"Error: {str(exc)}")


@router.post("/api/restore")
async def restore_endpoint(req: RestoreRequest):
    try:
        if not state.history_db:
            raise HTTPException(status_code=400, detail="History database not initialized")

        entry = await run_in_threadpool(state.history_db.get_entry, req.history_id)
        if not entry:
            raise HTTPException(status_code=404, detail="History entry not found")

        file_path = validate_path(req.file_path, access_type="write")
        lock = get_file_lock(file_path)
        async with lock:
            from latex_ai.services.files import atomic_write
            await atomic_write(file_path, entry.new_text)
        return {"success": True, "message": "File restored"}
    except HTTPException:
        raise
    except Exception as exc:
        raise HTTPException(status_code=400, detail=f"Error: {str(exc)}")


@router.get("/api/file")
async def get_file_endpoint(path: str, if_none_match: Optional[str] = Header(None), range: Optional[str] = Header(None)):
    """
    Get file content with optional Range header support for progressive loading.
    
    Headers:
        Range: bytes=start-end  (optional, for partial content)
    
    Returns:
        File content or partial content if Range header is present
    """
    try:
        import asyncio
        file_path = validate_path(path, access_type="read")
        if HAS_AIOFILES:
            exists = await aiofiles.os.path.exists(str(file_path))
        else:
            exists = await asyncio.to_thread(file_path.exists)
        if not exists:
            raise HTTPException(status_code=404, detail=f"File not found: {path}")
        
        # Read content once and compute ETag
        content = await read_text(file_path)
        etag = compute_etag(content)
        
        # If client has matching ETag and no range requested, return 304
        if if_none_match and if_none_match == etag and not range:
            return JSONResponse(status_code=304, content=None, headers={"ETag": etag})
        
        # Handle Range header for progressive loading of large files
        if range:
            import re
            match = re.match(r"bytes=(\d+)-(\d*)", range)
            if not match:
                raise HTTPException(status_code=416, detail="Invalid Range header format")
            
            start = int(match.group(1))
            end_str = match.group(2)
            
            content_length = len(content)
            end = int(end_str) if end_str else content_length - 1
            
            if start >= content_length or start > end:
                raise HTTPException(status_code=416, detail="Range not satisfiable")
            
            partial_content = content[start:end + 1]
            
            return JSONResponse(
                content={
                    "path": path,
                    "content": partial_content,
                    "partial": True,
                    "start": start,
                    "end": end,
                    "total_size": content_length
                },
                headers={
                    "ETag": etag,
                    "Content-Range": f"bytes {start}-{end}/{content_length}"
                },
                status_code=206  # Partial Content
            )
        
        return JSONResponse(content={"path": path, "content": content}, headers={"ETag": etag})
    except HTTPException:
        raise
    except Exception as exc:
        raise HTTPException(status_code=400, detail=f"Error: {str(exc)}")


@router.post("/api/file")
async def save_file_endpoint(req: SaveFileRequest, if_match: Optional[str] = Header(None)):
    try:
        cfg = get_config()
        content_size, human_size = compute_file_size(req.content)
        max_size_bytes = cfg.max_file_size
        max_size_human = ""
        
        # Format max size for error message
        if max_size_bytes < 1024:
            max_size_human = f"{max_size_bytes} B"
        elif max_size_bytes < 1024 * 1024:
            max_size_human = f"{max_size_bytes / 1024:.1f} KB"
        else:
            max_size_human = f"{max_size_bytes / (1024 * 1024):.1f} MB"
        
        if content_size > max_size_bytes:
            raise HTTPException(
                status_code=413,
                detail=f"File too large: {human_size} (max {max_size_human})"
            )

        file_path = validate_path(req.path, access_type="write")
        result = await save_with_etag_history_cache(
            file_path,
            req.content,
            if_match=if_match,
            history_db=state.history_db,
            history_command="manual-save",
        )

        if not result["success"] and result.get("status") == 409:
            conflict = result.get("conflict", {})
            return JSONResponse(
                status_code=409,
                content={
                    "error": "Conflict detected",
                    "message": "File was modified externally",
                    "current_content": conflict.get("current_content", ""),
                    "current_etag": conflict.get("current_etag"),
                },
                headers={"ETag": conflict.get("current_etag") or ""} if conflict.get("current_etag") else {},
            )

        return JSONResponse(
            status_code=result.get("status", 200),
            content={"success": True, "message": "File saved"},
            headers={"ETag": result.get("etag", "")} if result.get("etag") else {},
        )
    except HTTPException:
        raise
    except Exception as exc:
        raise HTTPException(status_code=400, detail=f"Error: {str(exc)}")


@router.get("/api/files", response_model=ListFilesResponse)
async def list_files_endpoint(path: Optional[str] = None, search: Optional[str] = None, limit: int = 100, offset: int = 0) -> ListFilesResponse:
    try:
        base = get_workspace_root() if path is None else validate_path(path)
        if not base.exists():
            raise HTTPException(status_code=404, detail=f"Directory not found: {path or 'workspace root'}")

        cache = get_cache()
        cache_key = f"{base}:{search or ''}"
        cached = await cache.get_cached_file_list(cache_key, depth=0)
        if cached is not None:
            total = len(cached)
            paginated = cached[offset : offset + limit]
            has_more = offset + limit < total
            return ListFilesResponse(files=paginated, total=total, offset=offset, limit=limit, has_more=has_more)

        def _list_files_sync() -> list:
            from latex_ai.config import get_config as _get_config

            cfg = _get_config()
            ignore_dirs = set(cfg.editor.ignore_patterns) if cfg.editor.ignore_patterns else {
                ".git",
                "node_modules",
                ".venv",
                "dist",
                "build",
                "__pycache__",
                ".pytest_cache",
                "venv",
                ".mypy_cache",
            }
            files: list[dict] = []
            workspace = get_workspace_root()
            scan_limit = offset + limit + 1

            def scan_directory(dir_path: Path):
                if len(files) >= scan_limit:
                    return
                try:
                    with os.scandir(dir_path) as entries:
                        dirs_to_scan = []
                        for entry in entries:
                            if len(files) >= scan_limit:
                                break
                            if entry.is_dir(follow_symlinks=False):
                                if entry.name in ignore_dirs:
                                    continue
                                dirs_to_scan.append(entry.path)
                                continue
                            if entry.is_file(follow_symlinks=False) and entry.name.endswith(".tex"):
                                file_path = Path(entry.path)
                                try:
                                    rel_path = file_path.relative_to(workspace)
                                    stat = entry.stat()
                                    files.append(
                                        {
                                            "path": str(rel_path).replace("\\", "/"),
                                            "name": file_path.name,
                                            "dir": str(rel_path.parent).replace("\\", "/") if rel_path.parent != Path(".") else "root",
                                            "size": stat.st_size,
                                            "mtime": int(stat.st_mtime),
                                        }
                                    )
                                except (ValueError, OSError):
                                    pass
                        for subdir in dirs_to_scan:
                            if len(files) >= scan_limit:
                                break
                            scan_directory(Path(subdir))
                except PermissionError:
                    return

            scan_directory(base)
            if search:
                search_lower = search.lower()
                files[:] = [f for f in files if search_lower in f["name"].lower() or search_lower in f["path"].lower()]
            return files

        all_files = await run_in_threadpool(_list_files_sync)
        if len(all_files) < (offset + limit + 1):
            await cache.cache_file_list(cache_key, depth=0, files=all_files)

        total = len(all_files)
        paginated_files = all_files[offset : offset + limit]
        has_more = offset + limit < total
        return ListFilesResponse(files=paginated_files, total=total, offset=offset, limit=limit, has_more=has_more)
    except HTTPException:
        raise
    except Exception as exc:
        raise HTTPException(status_code=400, detail=f"Error: {str(exc)}")


@router.post("/api/bulk-apply", response_model=BulkApplyResponse)
async def bulk_apply_endpoint(req: BulkApplyRequest) -> BulkApplyResponse:
    """Apply a command to multiple files with comprehensive ETag conflict detection."""
    try:
        import asyncio
        
        # Phase 1: Pre-validate ALL files with ETag checks
        conflicts = []
        file_contents = {}  # Cache to avoid re-reading
        
        for file_path in req.files:
            try:
                file_path_obj = validate_path(file_path, access_type="read")
                
                if HAS_AIOFILES:
                    exists = await aiofiles.os.path.exists(str(file_path_obj))
                else:
                    exists = await asyncio.to_thread(file_path_obj.exists)
                
                if exists:
                    current_content = await read_text(file_path_obj)
                    current_etag = compute_etag(current_content)
                    file_contents[file_path] = current_content
                    
                    # Check ETag if provided
                    expected_etag = req.etags.get(file_path) if req.etags else None
                    if expected_etag and current_etag != expected_etag:
                        conflicts.append({
                            "file": file_path,
                            "expected_etag": expected_etag,
                            "current_etag": current_etag,
                            "current_content": current_content,
                            "message": "File was modified since last read"
                        })
            except Exception as e:
                conflicts.append({
                    "file": file_path,
                    "message": f"Validation error: {str(e)}"
                })
        
        # Abort entire operation if ANY conflicts detected
        if conflicts:
            return JSONResponse(
                status_code=409,
                content={
                    "success": False,
                    "message": "ETag conflicts detected - resolve before applying",
                    "results": [],
                    "conflicts": conflicts
                }
            )
        
        # Phase 2: Apply edits (all validations passed)
        async def _process_file(file_path: str):
            try:
                file_path_obj = validate_path(file_path, access_type="write")
                if not file_path_obj.exists():
                    return {"file": file_path, "changed": False, "message": "File not found", "success": False}

                lock = get_file_lock(file_path_obj)
                async with lock:
                    engine = await run_in_threadpool(LatexEngine, file_path_obj)
                    result = await run_in_threadpool(engine.apply_command, req.command, False)
                    if result.changed and result.new_text:
                        engine.text = result.new_text
                        await run_in_threadpool(engine.save, True)
                        if state.history_db:
                            diff = await run_in_threadpool(engine.compute_diff, result.new_text)
                            await run_in_threadpool(state.history_db.add_entry, file_path, req.command, result.new_text, diff)
                return {"file": file_path, "changed": result.changed, "message": result.message, "success": True}
            except Exception as exc_inner:
                return {"file": file_path, "changed": False, "message": str(exc_inner), "success": False}

        results = []
        for file_path in req.files:
            result = await _process_file(file_path)
            results.append(result)
        return BulkApplyResponse(results=results, conflicts=None)
    except Exception as exc:
        raise HTTPException(status_code=400, detail=f"Error: {str(exc)}")


@router.get("/api/config", response_model=ConfigResponse)
async def get_config_endpoint() -> ConfigResponse:
    try:
        config = get_config()
        return ConfigResponse(
            keyboard_shortcuts={
                "save": config.keyboard_shortcuts.save,
                "undo": config.keyboard_shortcuts.undo,
                "compile": config.keyboard_shortcuts.compile,
                "apply_command": config.keyboard_shortcuts.apply_command,
                "toggle_history": config.keyboard_shortcuts.toggle_history,
                "toggle_file_tree": config.keyboard_shortcuts.toggle_file_tree,
            },
            theme={
                "mode": config.theme.mode,
                "primary_color": config.theme.primary_color,
                "accent_color": config.theme.accent_color,
                "error_color": config.theme.error_color,
            },
            editor={
                "font_size": config.editor.font_size,
                "font_family": config.editor.font_family,
                "theme": config.editor.theme,
                "tab_size": config.editor.tab_size,
                "word_wrap": config.editor.word_wrap,
                "minimap_enabled": config.editor.minimap_enabled,
            },
            auto_save=config.auto_save,
            history_limit=config.history_limit,
            ai_model_default=getattr(config, "ai_model_default", None),
            available_models=AVAILABLE_MODELS,
        )
    except Exception as exc:
        raise HTTPException(status_code=400, detail=f"Error: {str(exc)}")


@router.post("/api/config")
async def update_config_endpoint(updates: dict) -> dict:
    try:
        manager = get_config_manager()
        current_config = manager.get_config()
        for key_path, value in updates.items():
            keys = key_path.split(".")
            if len(keys) == 1:
                if hasattr(current_config, key_path):
                    if key_path == "ai_model_default" and value not in AVAILABLE_MODEL_IDS:
                        raise HTTPException(
                            status_code=400,
                            detail={"error": f"Invalid model id '{value}'.", "valid_ids": sorted(list(AVAILABLE_MODEL_IDS))},
                        )
                    setattr(current_config, key_path, value)
            elif len(keys) == 2:
                parent_name, child_name = keys
                if hasattr(current_config, parent_name):
                    parent = getattr(current_config, parent_name)
                    if hasattr(parent, child_name):
                        setattr(parent, child_name, value)

        manager._config = current_config
        manager._save_config(current_config)

        return {
            "success": True,
            "message": "Configuration updated successfully",
            "config": {
                "keyboard_shortcuts": {
                    "save": current_config.keyboard_shortcuts.save,
                    "undo": current_config.keyboard_shortcuts.undo,
                    "compile": current_config.keyboard_shortcuts.compile,
                    "apply_command": current_config.keyboard_shortcuts.apply_command,
                    "toggle_history": current_config.keyboard_shortcuts.toggle_history,
                    "toggle_file_tree": current_config.keyboard_shortcuts.toggle_file_tree,
                },
                "theme": {
                    "mode": current_config.theme.mode,
                    "primary_color": current_config.theme.primary_color,
                    "accent_color": current_config.theme.accent_color,
                    "error_color": current_config.theme.error_color,
                },
                "editor": {
                    "font_size": current_config.editor.font_size,
                    "font_family": current_config.editor.font_family,
                    "theme": current_config.editor.theme,
                    "tab_size": current_config.editor.tab_size,
                    "word_wrap": current_config.editor.word_wrap,
                    "minimap_enabled": current_config.editor.minimap_enabled,
                },
                "auto_save": current_config.auto_save,
                "history_limit": current_config.history_limit,
                "ai_model_default": getattr(current_config, "ai_model_default", None),
            },
        }
    except Exception as exc:
        raise HTTPException(status_code=400, detail=f"Error updating configuration: {str(exc)}")
