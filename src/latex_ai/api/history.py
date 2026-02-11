"""History API endpoints for tracking file changes."""

import logging
from typing import Any, Dict, List

from fastapi import APIRouter, HTTPException
from fastapi.concurrency import run_in_threadpool

from latex_ai.api.dependencies import validate_path
from latex_ai.api.schemas import HistoryEntryResponse
from latex_ai.state import state

router = APIRouter()
logger = logging.getLogger(__name__)


@router.post("/api/history/batch")
async def get_history_batch(request: Dict[str, Any]) -> Dict[str, List[HistoryEntryResponse]]:
    """
    Fetch history for multiple files in a single request.
    Reduces N+1 query problem when loading history for many files.
    
    Request body:
    {
        "file_paths": ["path1.tex", "path2.tex"],
        "limit": 50,
        "offset": 0
    }
    
    Returns:
        Dictionary mapping file paths to their history entries
    """
    try:
        file_paths = request.get("file_paths", [])
        limit = request.get("limit", 50)
        offset = request.get("offset", 0)
        
        if not file_paths or not isinstance(file_paths, list):
            raise HTTPException(status_code=400, detail="file_paths must be a non-empty array")
        
        if len(file_paths) > 100:
            raise HTTPException(status_code=400, detail="Maximum 100 files per batch request")
        
        if not state.history_db:
            return {fp: [] for fp in file_paths}
        
        results = {}
        
        for file_path in file_paths:
            try:
                validated_path = validate_path(file_path)
                entries = await run_in_threadpool(
                    state.history_db.get_history, str(validated_path), limit=limit, offset=offset
                )
                results[file_path] = [
                    HistoryEntryResponse(
                        id=entry.id,
                        file_path=entry.file_path,
                        command=entry.command,
                        timestamp=entry.timestamp.isoformat(),
                        diff=entry.diff,
                        success=entry.success,
                    )
                    for entry in entries
                ]
            except Exception as exc:
                logger.warning(f"Error fetching history for {file_path}: {exc}")
                results[file_path] = []
        
        return results
    except HTTPException:
        raise
    except Exception as exc:
        logger.error(f"Batch history error: {exc}")
        raise HTTPException(status_code=400, detail=f"Error: {str(exc)}")
