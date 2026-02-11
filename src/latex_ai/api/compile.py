"""Compilation, SyncTeX, and PDF-serving routes."""
from __future__ import annotations

import asyncio
import json
import logging
from pathlib import Path
from typing import Optional

from fastapi import APIRouter, HTTPException, Request
from fastapi.concurrency import run_in_threadpool
from fastapi.responses import FileResponse, JSONResponse, StreamingResponse
from starlette.requests import Request

try:
    from sse_starlette.sse import EventSourceResponse
    HAS_SSE_STARLETTE = True
except ImportError:
    HAS_SSE_STARLETTE = False
    EventSourceResponse = None

from latex_ai.api.dependencies import validate_directory_path, validate_path
from latex_ai.api.schemas import CompileRequest, CompileResponse, CompileStatusResponse, SyncTeXBackwardRequest, SyncTeXForwardRequest, SyncTeXResponse
from latex_ai.compilation_queue import get_compilation_queue
from latex_ai.config import get_config
from latex_ai.metrics import get_metrics
from latex_ai.pdf_compiler import compile_file
from latex_ai.rate_limiter import compile_limiter
from latex_ai.state import state
from latex_ai.synctex_parser import parse_synctex
from latex_ai.streaming import stream_sse_events

router = APIRouter()


@router.post("/api/synctex")
async def synctex_endpoint(file_path: str, page: int, h: float, v: float):
    try:
        file_path_obj = validate_path(file_path)
        synctex_file = file_path_obj.with_suffix(".synctex.gz")
        if not synctex_file.exists():
            raise HTTPException(status_code=404, detail="SyncTeX file not generated. Try compiling first.")

        def _parse():
            from latex_ai.synctex_parser import SyncTeXParser

            parser = SyncTeXParser(synctex_file)
            return parser.get_source(page=page, h=h, v=v)

        mapping = await run_in_threadpool(_parse)
        if not mapping:
            raise HTTPException(status_code=404, detail=f"No mapping found for page {page} at ({h}, {v})")
        return {"file": mapping.file, "line": mapping.line, "column": mapping.column}
    except HTTPException:
        raise
    except Exception as exc:
        logging.error(f"SyncTeX error: {exc}")
        raise HTTPException(status_code=400, detail=f"Error: {str(exc)}")


@router.post("/api/compile/check-cache")
async def check_compile_cache(req: CompileRequest) -> Dict:
    """
    Check if compilation result is cached.
    Returns cached PDF path if available, avoiding unnecessary compilation.
    """
    try:
        file_path = validate_path(req.file_path)
        exists = await run_in_threadpool(file_path.exists)
        if not exists:
            raise HTTPException(status_code=404, detail=f"File not found: {req.file_path}")
        
        # Read file content and compute semantic hash
        content = await run_in_threadpool(file_path.read_text, encoding="utf-8")
        compiler = get_pdf_compiler()
        
        # Compute hash with compile config
        config = {
            "max_iterations": getattr(req, "max_iterations", None),
            "output_dir": req.output_dir
        }
        content_hash = await run_in_threadpool(compiler._compute_content_hash, content, config)
        
        # Check cache
        cached_pdf = await run_in_threadpool(compiler._get_cached_pdf, content_hash)
        
        if cached_pdf:
            return {
                "cached": True,
                "pdf_path": str(cached_pdf),
                "content_hash": content_hash
            }
        
        return {"cached": False, "content_hash": content_hash}
    except HTTPException:
        raise
    except Exception as exc:
        raise HTTPException(status_code=400, detail=f"Cache check error: {str(exc)}")


@router.post("/api/compile")
async def compile_endpoint(req: CompileRequest, request: Request):
    try:
        # Support both IP-based and session-based rate limiting
        session_id = request.cookies.get("session_id")
        client_ip = request.client.host if request.client else "unknown"
        x_forwarded_for = request.headers.get("X-Forwarded-For")
        if x_forwarded_for:
            client_ip = x_forwarded_for.split(",")[0].strip()
        
        # Use session ID if available, otherwise fall back to IP
        client_identifier = session_id if session_id else client_ip

        if not await compile_limiter.check_limit(client_identifier):
            remaining = await compile_limiter.get_remaining(client_identifier)
            raise HTTPException(status_code=429, detail=f"Too many compilation requests. Try again later. Remaining: {remaining}")

        if state.compilation_semaphore is None:
            raise HTTPException(status_code=503, detail="Server not ready")

        async with state.compilation_semaphore:
            file_path = validate_path(req.file_path)
            main_file_path = validate_path(req.main_file) if req.main_file else file_path
            output_dir_path = validate_directory_path(req.output_dir) if req.output_dir else None

            exists = await run_in_threadpool(file_path.exists)
            if not exists:
                raise HTTPException(status_code=404, detail=f"File not found: {req.file_path}")

            queue = get_compilation_queue()
            job = await queue.submit(
                file_path=main_file_path,
                options={
                    "output_dir": str(output_dir_path) if output_dir_path else str(main_file_path.parent),
                    "source_file": str(file_path),
                },
            )
            return {"job_id": job.job_id, "status_url": f"/api/compile/status/{job.job_id}", "status": job.status.value}
    except HTTPException:
        raise
    except Exception as exc:
        raise HTTPException(status_code=400, detail=f"Error: {str(exc)}")


@router.get("/api/compile/status/{job_id}", response_model=CompileStatusResponse)
async def get_compile_status(job_id: str) -> CompileStatusResponse:
    try:
        queue = get_compilation_queue()
        job = await queue.get_job(job_id)
        if not job:
            raise HTTPException(status_code=404, detail="Job not found")
        pdf_url = None
        if job.status.value == "completed" and job.pdf_url:
            from urllib.parse import quote

            pdf_url = f"/api/pdf?path={quote(job.pdf_url)}"
        return CompileStatusResponse(
            job_id=job.job_id,
            status=job.status.value,
            progress=job.progress,
            pdf_url=pdf_url,
            diagnostics=job.diagnostics,
            error=job.error,
            duration=(job.completed_at - job.started_at) if job.completed_at and job.started_at else None,
        )
    except HTTPException:
        raise
    except Exception as exc:
        raise HTTPException(status_code=400, detail=f"Error: {str(exc)}")


@router.delete("/api/compile/{job_id}")
async def cancel_compile_job(job_id: str):
    try:
        queue = get_compilation_queue()
        cancelled = await queue.cancel_job(job_id)
        if not cancelled:
            raise HTTPException(status_code=404, detail="Job not found or already completed")
        return {"success": True, "message": "Job cancelled"}
    except HTTPException:
        raise
    except Exception as exc:
        raise HTTPException(status_code=400, detail=f"Error: {str(exc)}")


@router.post("/api/compile-sync", response_model=CompileResponse)
async def compile_sync_endpoint(req: CompileRequest) -> CompileResponse:
    try:
        file_path = validate_path(req.file_path)
        output_dir_path = validate_directory_path(req.output_dir) if req.output_dir else None
        exists = await run_in_threadpool(file_path.exists)
        if not exists:
            raise HTTPException(status_code=404, detail=f"File not found: {req.file_path}")

        result = await run_in_threadpool(
            compile_file,
            str(file_path),
            str(output_dir_path) if output_dir_path else str(file_path.parent),
        )

        pdf_url = None
        if result.success and result.pdf_path:
            from urllib.parse import quote

            pdf_url = f"/api/pdf?path={quote(result.pdf_path)}"

        errors = [
            {"line": e.line, "file": getattr(e, "file", "") or "", "message": getattr(e, "message", str(e)), "type": e.error_type}
            for e in result.errors
        ]
        warnings = [
            {"line": w.line, "file": getattr(w, "file", "") or "", "message": getattr(w, "message", str(w)), "type": w.error_type}
            for w in result.warnings
        ]

        return CompileResponse(
            success=result.success,
            pdf_path=result.pdf_path,
            pdf_url=pdf_url,
            synctex_path=result.synctex_path,
            page_count=result.page_count,
            compile_time=result.compile_time,
            errors=errors,
            warnings=warnings,
        )
    except HTTPException:
        raise
    except Exception as exc:
        raise HTTPException(status_code=400, detail=f"Compilation error: {str(exc)}")


@router.get("/api/compile/stream/{job_id}")
async def stream_compile_status(job_id: str, request: Request):
    queue = get_compilation_queue()
    job = await queue.get_job(job_id)
    if not job:
        raise HTTPException(status_code=404, detail="Job not found")

    async def event_generator():
        event_queue = await job.subscribe()
        try:
            yield {"event": "progress", "data": json.dumps({"status": job.status.value, "progress": job.progress, "step": "Initializing"})}
            while True:
                # Check if client disconnected to avoid wasted work
                if await request.is_disconnected():
                    import logging
                    logging.info(f"Client disconnected from compile stream for job {job_id}, stopping generation")
                    break
                
                try:
                    event = await asyncio.wait_for(event_queue.get(), timeout=30.0)
                    yield {"event": "progress", "data": json.dumps(event)}
                    if event.get("status") in ("completed", "failed"):
                        break
                except asyncio.TimeoutError:
                    yield {"event": "keepalive", "data": json.dumps({"status": job.status.value})}
        finally:
            # Cleanup: unsubscribe from job events
            await job.unsubscribe(event_queue)
            
            # Auto-cancel: if this was the last subscriber and job is still queued, cancel it
            if not job.has_subscribers() and job.status == JobStatus.QUEUED:
                import logging
                logging.info(f"Last subscriber disconnected from queued job {job_id}, auto-canceling")
                await queue.cancel_job(job_id)

    if HAS_SSE_STARLETTE:
        return EventSourceResponse(event_generator())
    else:
        # Fallback to StreamingResponse with SSE format
        return StreamingResponse(
            stream_sse_events(event_generator()),
            media_type="text/event-stream"
        )


@router.get("/api/pdf")
async def pdf_endpoint(path: str):
    try:
        pdf_path = validate_path(path)
        if not pdf_path.exists():
            raise HTTPException(status_code=404, detail=f"PDF not found: {path}")
        if not pdf_path.suffix.lower() == ".pdf":
            raise HTTPException(status_code=400, detail="File is not a PDF")
        return FileResponse(pdf_path, media_type="application/pdf", filename=pdf_path.name)
    except HTTPException:
        raise
    except Exception as exc:
        raise HTTPException(status_code=400, detail=f"Error: {str(exc)}")


@router.post("/api/synctex/forward", response_model=SyncTeXResponse)
async def synctex_forward(req: SyncTeXForwardRequest) -> SyncTeXResponse:
    try:
        file_path = validate_path(req.file_path, access_type="read")
        if req.synctex_file:
            synctex_path = Path(req.synctex_file)
        else:
            synctex_path = file_path.parent / "document.synctex.gz"
        if not synctex_path.exists():
            return SyncTeXResponse(success=False, error=f"SyncTeX file not found: {synctex_path}")

        parser = await run_in_threadpool(parse_synctex, synctex_path)
        result = await run_in_threadpool(parser.forward_search, str(file_path), req.line)
        if result:
            return SyncTeXResponse(
                success=True,
                file=result["file"],
                line=result["line"],
                page=result["page"],
                x=result["x"],
                y=result["y"],
                max_file_size_mb=get_config().max_file_size // (1024 * 1024),
            )
        return SyncTeXResponse(success=False, error="No matching location found in PDF")
    except Exception as exc:
        logging.error(f"SyncTeX forward sync error: {exc}")
        return SyncTeXResponse(success=False, error=str(exc))


@router.post("/api/synctex/backward", response_model=SyncTeXResponse)
async def synctex_backward(req: SyncTeXBackwardRequest) -> SyncTeXResponse:
    try:
        synctex_path = Path(req.synctex_file)
        if not synctex_path.exists():
            return SyncTeXResponse(success=False, error=f"SyncTeX file not found: {synctex_path}")

        parser = await run_in_threadpool(parse_synctex, synctex_path)
        result = await run_in_threadpool(parser.backward_search, req.page, req.x, req.y)
        if result:
            return SyncTeXResponse(
                success=True,
                file=result["file"],
                line=result["line"],
                column=result.get("column", 0),
                page=result["page"],
                max_file_size_mb=get_config().max_file_size // (1024 * 1024),
            )
        return SyncTeXResponse(success=False, error="No matching source location found")
    except Exception as exc:
        logging.error(f"SyncTeX backward sync error: {exc}")
        return SyncTeXResponse(success=False, error=str(exc))


@router.get("/api/metrics", response_class=JSONResponse)
async def metrics_text():
    from fastapi.responses import PlainTextResponse

    metrics = get_metrics()
    return PlainTextResponse(content=metrics.get_prometheus_text(), media_type="text/plain")


@router.get("/api/metrics/json")
async def metrics_json():
    metrics = get_metrics()
    return metrics.get_summary()
