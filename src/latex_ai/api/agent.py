"""Agent and AI-related API routes."""
from __future__ import annotations

import asyncio
import json
import logging
import warnings
from difflib import unified_diff
from pathlib import Path
from typing import List, Optional

from fastapi import APIRouter, Header, HTTPException, Request
from fastapi.concurrency import run_in_threadpool
from fastapi.responses import JSONResponse, StreamingResponse

try:
    from sse_starlette.sse import EventSourceResponse
    HAS_SSE_STARLETTE = True
except ImportError:
    HAS_SSE_STARLETTE = False
    EventSourceResponse = None

from latex_ai.ai_planner import get_ai_planner
from latex_ai.config import get_config
from latex_ai.rate_limiter import SessionBasedRateLimiter
from latex_ai.api.dependencies import get_workspace_root, read_file_with_limit, validate_path
from latex_ai.api.schemas import (
    AgentEdit,
    AgentEditResult,
    AgentLoopRequest,
    AgentRequest,
    AgentResponse,
    PlanRequest,
)
from latex_ai.cache import get_cache
from latex_ai.document_bundle import DocumentBundle
from latex_ai.engine import LatexEngine
from latex_ai.pdf_compiler import compile_file
from latex_ai.services.files import compute_etag, get_file_lock, read_text, save_with_etag_history_cache, with_file_lock
from latex_ai.services.ai import apply_agent_edits, compute_proposal_diffs, capture_etags, generate_fix_instruction
from latex_ai.state import state
from latex_ai.streaming import stream_sse_events

router = APIRouter()

# Session-based rate limiter for AI calls
_session_limiter = SessionBasedRateLimiter()


def _get_streaming_response(event_generator_func):
    """Return appropriate streaming response (EventSourceResponse or StreamingResponse fallback)."""
    if HAS_SSE_STARLETTE:
        return EventSourceResponse(event_generator_func())
    else:
        # Fallback: StreamingResponse with SSE-formatted output
        return StreamingResponse(
            stream_sse_events(event_generator_func()),
            media_type="text/event-stream"
        )


@router.post("/api/plan")
async def plan_endpoint(req: PlanRequest, session_id: Optional[str] = Header(None, alias="X-Session-ID")):
    # Rate limit AI calls per session
    client_id = session_id or "default"
    if not await _session_limiter.check_ai_call_limit(client_id):
        raise HTTPException(status_code=429, detail="AI rate limit exceeded. Please try again later.")
    
    try:
        file_path_obj = validate_path(req.file_path)
        exists = await run_in_threadpool(file_path_obj.exists)
        if not exists:
            raise HTTPException(status_code=404, detail=f"File not found: {req.file_path}")

        async def _plan_op():
            engine = await run_in_threadpool(LatexEngine, file_path_obj)
            planner = get_ai_planner(model=req.model)
            if not planner:
                raise HTTPException(status_code=503, detail="AI planner not available")
            plan = await run_in_threadpool(planner.create_plan, engine.document, req.request_text)
            if not plan:
                raise HTTPException(status_code=400, detail="Failed to generate plan")
            return plan.to_dict()

        lock = get_file_lock(file_path_obj)
        async with lock:
            return await _plan_op()
    except HTTPException:
        raise
    except Exception as exc:
        raise HTTPException(status_code=400, detail=f"Error: {str(exc)}")


@router.post("/api/plan/stream")
async def plan_stream_endpoint(
    file_path: str,
    request_text: str,
    model: Optional[str] = None,
    context: Optional[list] = None,
    session_id: Optional[str] = Header(None, alias="X-Session-ID"),
    request: Request = None,  # Add request to check disconnect
):
    """Stream AI planning response using Server-Sent Events.
    
    Uses direct async streaming with the new SSE helper for proper event loop handling.
    Cancels work if client disconnects to save resources.
    """
    # Rate limit AI calls per session
    client_id = session_id or "default"
    if not await _session_limiter.check_ai_call_limit(client_id):
        raise HTTPException(status_code=429, detail="AI rate limit exceeded. Please try again later.")
    
    from latex_ai.streaming_helpers import sse_stream
    
    async def event_generator():
        stream_completed = False  # Track if stream completed successfully
        try:
            file_path_obj = validate_path(file_path)
            exists = await run_in_threadpool(file_path_obj.exists)
            if not exists:
                yield {"type": "error", "message": f"File not found: {file_path}"}
                return

            engine = await run_in_threadpool(LatexEngine, file_path_obj)
            planner = get_ai_planner(model=model)
            if not planner:
                yield {"type": "error", "message": "AI planner not available"}
                return

            # SIDE-EFFECT SAFETY: Only add to conversation if stream completes
            # Don't modify conversation state on disconnect

            # Stream planning events directly
            async for event in planner.create_plan_streaming(
                engine.document, 
                request_text, 
                session_id=session_id, 
                conversation_context=context
            ):
                # Check if client disconnected to avoid wasted work
                if request and await request.is_disconnected():
                    logging.info(f"Client disconnected during AI streaming, stopping generation (skip side effects)")
                    stream_completed = False
                    break
                    
                # Normalize event format for SSE helper
                yield event
            else:
                # Stream completed successfully without disconnect
                stream_completed = True
            
            # SIDE-EFFECT SAFETY: Only write conversation history if stream completed
            if stream_completed and session_id:
                planner.add_to_conversation(session_id, "user", request_text)
                
        except Exception as exc:
            logging.error(f"Streaming error: {exc}")
            yield {"type": "error", "message": str(exc)}
            stream_completed = False

    from starlette.responses import StreamingResponse
    return StreamingResponse(
        sse_stream(event_generator()),
        media_type="text/event-stream"
    )

@router.post("/api/agent/patch")
async def agent_patch_endpoint(file_path: str, instruction: str, error_context: Optional[str] = None, model: Optional[str] = None, session_id: Optional[str] = Header(None, alias="X-Session-ID")):
    # Rate limit AI calls per session
    client_id = session_id or "default"
    if not await _session_limiter.check_ai_call_limit(client_id):
        raise HTTPException(status_code=429, detail="AI rate limit exceeded. Please try again later.")
    
    warnings.warn(
        "The /api/agent/patch endpoint is deprecated. Use /api/agent with structured edits instead.",
        DeprecationWarning,
        stacklevel=2,
    )
    logging.warning("DEPRECATED: /api/agent/patch called. Use /api/agent instead.")

    try:
        file_path_obj = validate_path(file_path)
        exists = await run_in_threadpool(file_path_obj.exists)
        if not exists:
            raise HTTPException(status_code=404, detail=f"File not found: {file_path}")

        from latex_ai.services.ai import PatchBlock, generate_patch

        lock = get_file_lock(file_path_obj)
        async with lock:
            planner = get_ai_planner(model=model)
            if not planner:
                raise HTTPException(status_code=503, detail="AI planner not available")

            cfg = get_config()
            current_content = await read_text(file_path_obj)
            current_etag = compute_etag(current_content)

            patch_text = await generate_patch(
                planner,
                file_path_obj,
                instruction,
                error_context=error_context or "",
                max_retries=getattr(cfg, "ai_max_retries", 3),
                retry_delay=getattr(cfg, "ai_retry_delay", 1.0),
            )

        blocks = PatchBlock.parse_blocks(patch_text)
        return {
            "patch_text": patch_text,
            "blocks": [{"search": block.search, "replace": block.replace} for block in blocks],
            "etag": current_etag,
        }
    except HTTPException:
        raise
    except Exception as exc:
        logging.error(f"Patch generation error: {exc}")
        raise HTTPException(status_code=400, detail=f"Error: {str(exc)}")


@router.post("/api/agent/apply")
async def agent_apply_blocks_endpoint(
    file_path: str,
    blocks: list[dict],
    auto_compile: bool = True,
    if_match: Optional[str] = Header(None),
    expected_etag: Optional[str] = None,
):
    warnings.warn(
        "The /api/agent/apply endpoint is deprecated. Use /api/agent instead.",
        DeprecationWarning,
        stacklevel=2,
    )
    logging.warning("DEPRECATED: /api/agent/apply called. Use /api/agent instead.")

    try:
        file_path_obj = validate_path(file_path)
        exists = await run_in_threadpool(file_path_obj.exists)
        if not exists:
            raise HTTPException(status_code=404, detail=f"File not found: {file_path}")

        from latex_ai.services.ai import PatchBlock

        patch_blocks = [PatchBlock(search=block["search"], replace=block["replace"]) for block in blocks]
        lock = get_file_lock(file_path_obj)

        async def _apply():
            content = await read_text(file_path_obj)
            if if_match or expected_etag:
                current_etag = compute_etag(content)
                expected = if_match or expected_etag
                if current_etag != expected:
                    raise HTTPException(
                        status_code=409,
                        detail={
                            "error": "ETag conflict - file has been modified",
                            "current_etag": current_etag,
                            "current_content": content,
                        },
                    )

            applied_count = 0
            for block in patch_blocks:
                if block.search in content:
                    content = content.replace(block.search, block.replace, 1)
                    applied_count += 1

            result = await save_with_etag_history_cache(
                file_path_obj,
                content,
                if_match=if_match,
                history_db=state.history_db,
                history_command="agent-apply",
                assume_locked=True,
            )
            return applied_count, content, result

        applied_count, new_content, save_result = await with_file_lock(file_path_obj, _apply)
        result = {"success": True, "applied_count": applied_count, "etag": save_result.get("etag")}

        if auto_compile:
            from latex_ai.services.compile import compile_async, extract_error_summary

            compile_result = await compile_async(str(file_path_obj))
            result["compile_result"] = {
                "success": compile_result.success,
                "page_count": compile_result.page_count if compile_result.success else 0,
                "error_count": len(compile_result.errors),
                "warning_count": len(compile_result.warnings),
                "errors": extract_error_summary(compile_result) if not compile_result.success else None,
            }
        return result
    except HTTPException:
        raise
    except Exception as exc:
        logging.error(f"Patch apply error: {exc}")
        raise HTTPException(status_code=400, detail=f"Error: {str(exc)}")


@router.post("/api/errors/explain/stream")
async def explain_errors_stream(
    file_path: Optional[str] = None,
    diagnostics: Optional[List[dict]] = None,
    session_id: Optional[str] = Header(None, alias="X-Session-ID"),
    context: Optional[list] = None,
    model: Optional[str] = None,
    request: Request = None,
):
    # Rate limit AI calls per session
    client_id = session_id or "default"
    if not await _session_limiter.check_ai_call_limit(client_id):
        raise HTTPException(status_code=429, detail="AI rate limit exceeded. Please try again later.")
    
    async def event_generator():
        stream_completed = False
        try:
            if not diagnostics:
                yield {"event": "error", "data": json.dumps({"message": "No diagnostics provided"})}
                return

            document = None
            if file_path:
                file_path_obj = validate_path(file_path)
                exists = await run_in_threadpool(file_path_obj.exists)
                if not exists:
                    yield {"event": "error", "data": json.dumps({"message": f"File not found: {file_path}"})}
                    return
                engine = await run_in_threadpool(LatexEngine, file_path_obj)
                document = engine.document

            planner = get_ai_planner(model=model)
            if not planner:
                yield {"event": "error", "data": json.dumps({"message": "AI planner not available"})}
                return

            async for evt in planner.explain_errors_streaming(
                document=document, diagnostics=diagnostics or [], session_id=session_id, conversation_context=context
            ):
                # Check disconnect to avoid wasted work
                if request and await request.is_disconnected():
                    logging.info("Client disconnected during error explanation, stopping (skip side effects)")
                    stream_completed = False
                    break
                yield {"event": evt.get("type", "message"), "data": json.dumps(evt.get("data", {}))}
            else:
                stream_completed = True
        except Exception as exc:
            logging.error(f"Error explanation streaming failed: {exc}")
            yield {"event": "error", "data": json.dumps({"message": str(exc)})}
            stream_completed = False

    return _get_streaming_response(event_generator)


@router.post("/api/refine/stream")
async def refine_document_stream(
    file_path: Optional[str] = None,
    request_text: Optional[str] = None,
    session_id: Optional[str] = Header(None, alias="X-Session-ID"),
    context: Optional[list] = None,
    model: Optional[str] = None,
    request: Request = None,
):
    # Rate limit AI calls per session
    client_id = session_id or "default"
    if not await _session_limiter.check_ai_call_limit(client_id):
        raise HTTPException(status_code=429, detail="AI rate limit exceeded. Please try again later.")
    
    async def event_generator():
        stream_completed = False
        try:
            if not request_text:
                yield {"event": "error", "data": json.dumps({"message": "Refinement request required"})}
                return
            if not file_path:
                yield {"event": "error", "data": json.dumps({"message": "File path required"})}
                return

            file_path_obj = validate_path(file_path)
            exists = await run_in_threadpool(file_path_obj.exists)
            if not exists:
                yield {"event": "error", "data": json.dumps({"message": f"File not found: {file_path}"})}
                return

            engine = await run_in_threadpool(LatexEngine, file_path_obj)
            document = engine.document

            planner = get_ai_planner(model=model)
            if not planner:
                yield {"event": "error", "data": json.dumps({"message": "AI planner not available"})}
                return

            async for evt in planner.refine_document_streaming(
                document=document, refinement_request=request_text, session_id=session_id, conversation_context=context
            ):
                # Check disconnect to avoid wasted work
                if request and await request.is_disconnected():
                    logging.info("Client disconnected during refinement, stopping (skip side effects)")
                    stream_completed = False
                    break
                yield {"event": evt.get("type", "message"), "data": json.dumps(evt.get("data", {}))}
            else:
                stream_completed = True
        except Exception as exc:
            logging.error(f"Document refinement streaming failed: {exc}")
            yield {"event": "error", "data": json.dumps({"message": str(exc)})}
            stream_completed = False

    return _get_streaming_response(event_generator)


@router.post("/api/suggest/stream")
async def suggest_improvements_stream(
    file_path: Optional[str] = None,
    session_id: Optional[str] = Header(None, alias="X-Session-ID"),
    request: Request = None,
):
    # Rate limit AI calls per session
    client_id = session_id or "default"
    if not await _session_limiter.check_ai_call_limit(client_id):
        raise HTTPException(status_code=429, detail="AI rate limit exceeded. Please try again later.")
    
    async def event_generator():
        stream_completed = False
        try:
            if not file_path:
                yield {"event": "error", "data": json.dumps({"message": "File path required"})}
                return
            file_path_obj = validate_path(file_path)
            exists = await run_in_threadpool(file_path_obj.exists)
            if not exists:
                yield {"event": "error", "data": json.dumps({"message": f"File not found: {file_path}"})}
                return
            engine = await run_in_threadpool(LatexEngine, file_path_obj)
            planner = get_ai_planner()
            if not planner:
                yield {"event": "error", "data": json.dumps({"message": "AI planner not available"})}
                return
            async for evt in planner.suggest_improvements_streaming(document=engine.document, session_id=session_id):
                # Check disconnect to avoid wasted work
                if request and await request.is_disconnected():
                    logging.info("Client disconnected during suggestions, stopping (skip side effects)")
                    stream_completed = False
                    break
                yield {"event": evt.get("type", "message"), "data": json.dumps(evt.get("data", {}))}
            else:
                stream_completed = True
        except Exception as exc:
            logging.error(f"Suggestions streaming failed: {exc}")
            yield {"event": "error", "data": json.dumps({"message": str(exc)})}
            stream_completed = False

    return _get_streaming_response(event_generator)


@router.post("/api/review/stream")
async def review_document_stream(
    file_path: Optional[str] = None,
    session_id: Optional[str] = Header(None, alias="X-Session-ID"),
    request: Request = None,
):
    # Rate limit AI calls per session
    client_id = session_id or "default"
    if not await _session_limiter.check_ai_call_limit(client_id):
        raise HTTPException(status_code=429, detail="AI rate limit exceeded. Please try again later.")
    
    async def event_generator():
        stream_completed = False
        try:
            if not file_path:
                yield {"event": "error", "data": json.dumps({"message": "File path required"})}
                return
            file_path_obj = validate_path(file_path)
            exists = await run_in_threadpool(file_path_obj.exists)
            if not exists:
                yield {"event": "error", "data": json.dumps({"message": f"File not found: {file_path}"})}
                return
            engine = await run_in_threadpool(LatexEngine, file_path_obj)
            planner = get_ai_planner()
            if not planner:
                yield {"event": "error", "data": json.dumps({"message": "AI planner not available"})}
                return
            async for evt in planner.review_document_streaming(document=engine.document, session_id=session_id):
                # Check disconnect to avoid wasted work
                if request and await request.is_disconnected():
                    logging.info("Client disconnected during review, stopping (skip side effects)")
                    stream_completed = False
                    break
                yield {"event": evt.get("type", "message"), "data": json.dumps(evt.get("data", {}))}
            else:
                stream_completed = True
        except Exception as exc:
            logging.error(f"Review streaming failed: {exc}")
            yield {"event": "error", "data": json.dumps({"message": str(exc)})}
            stream_completed = False

    return _get_streaming_response(event_generator)


@router.post("/api/bundle")
async def bundle_endpoint(file_path: str) -> dict:
    try:
        resolved_path = validate_path(file_path, access_type="read")
        bundle = DocumentBundle(resolved_path, workspace_root=get_workspace_root())
        success = await bundle.load_async()
        if not success:
            raise HTTPException(status_code=500, detail=f"Failed to load bundle: {bundle.errors}")
        return {"success": True, "bundle": bundle.as_dict(), "combined_text": bundle.get_combined_text()}
    except HTTPException:
        raise
    except Exception as exc:
        raise HTTPException(status_code=400, detail=f"Error bundling files: {str(exc)}")


@router.post("/api/agent", response_model=AgentResponse)
async def agent_edit_protocol_endpoint(req: AgentRequest, session_id: Optional[str] = Header(None, alias="X-Session-ID")) -> AgentResponse:
    # Rate limit AI calls per session (only if edits contain AI-generated content)
    # For now, we rate limit all agent requests to be safe
    client_id = session_id or "default"
    if not await _session_limiter.check_ai_call_limit(client_id):
        raise HTTPException(status_code=429, detail="AI rate limit exceeded. Please try again later.")
    
    results: List[AgentEditResult] = []
    compile_result = None
    try:
        for edit in req.edits:
            try:
                file_path = validate_path(edit.file_path, access_type="write")
                lock = get_file_lock(file_path)
                async with lock:
                    old_content = ""
                    # Use threadpool-safe exists to avoid optional aiofiles import
                    exists = await run_in_threadpool(file_path.exists)
                    if exists:
                        old_content = await read_file_with_limit(file_path)
                    current_etag = compute_etag(old_content) if exists else None
                    if edit.expected_etag and current_etag != edit.expected_etag:
                        results.append(
                            AgentEditResult(
                                file_path=edit.file_path,
                                success=False,
                                message=f"ETag conflict: expected {edit.expected_etag}, got {current_etag}",
                                conflict={
                                    "current_content": old_content,
                                    "current_etag": current_etag,
                                    "expected_etag": edit.expected_etag,
                                },
                            )
                        )
                        continue

                    new_content = old_content
                    message = ""

                    if edit.op == "rewrite_file":
                        new_content = edit.payload
                        message = f"Rewrote {file_path.name}"
                    elif edit.op == "replace_range":
                        try:
                            if edit.start_line is not None and edit.end_line is not None and edit.text is not None:
                                start_line = int(edit.start_line)
                                end_line = int(edit.end_line)
                                new_text = edit.text
                            else:
                                range_part, new_text = edit.payload.split(":", 1)
                                start_line, end_line = map(int, range_part.split(","))
                            if start_line < 1 or end_line < start_line:
                                raise ValueError(f"Invalid range: {start_line}-{end_line}")
                            lines = old_content.split("\n")
                            if end_line > len(lines):
                                raise ValueError(f"Line {end_line} exceeds file ({len(lines)} lines)")
                            start_idx = start_line - 1
                            end_idx = end_line
                            new_lines = lines[:start_idx] + new_text.split("\n") + lines[end_idx:]
                            new_content = "\n".join(new_lines)
                            message = f"Replaced lines {start_line}-{end_line} (1-based)"
                        except Exception as exc_inner:
                            results.append(
                                AgentEditResult(
                                    file_path=edit.file_path,
                                    success=False,
                                    message=f"Invalid replace_range payload: {str(exc_inner)}",
                                )
                            )
                            continue
                    elif edit.op == "insert":
                        try:
                            if edit.line is not None and (edit.insert_text is not None or edit.text is not None):
                                line_num = int(edit.line)
                                text = edit.insert_text if edit.insert_text is not None else edit.text  # type: ignore
                            else:
                                line_num_str, text = edit.payload.split(":", 1)
                                line_num = int(line_num_str)
                            if line_num < 1:
                                raise ValueError(f"Invalid line: {line_num} (must be >= 1)")
                            lines = old_content.split("\n")
                            if line_num > len(lines) + 1:
                                raise ValueError(f"Line {line_num} out of range (file has {len(lines)})")
                            insert_idx = line_num - 1
                            lines.insert(insert_idx, text)
                            new_content = "\n".join(lines)
                            message = f"Inserted before line {line_num} (1-based)"
                        except Exception as exc_inner:
                            results.append(
                                AgentEditResult(
                                    file_path=edit.file_path,
                                    success=False,
                                    message=f"Invalid insert payload: {str(exc_inner)}",
                                )
                            )
                            continue
                    elif edit.op in ("apply_dmp_patch", "apply_unified_diff"):
                        try:
                            from latex_ai.services.patch_utils import apply_semantic_patch

                            new_content = apply_semantic_patch(old_content, edit.payload)
                            if edit.op == "apply_unified_diff":
                                message = "Applied DMP patch (note: 'apply_unified_diff' is deprecated)"
                            else:
                                message = "Applied DMP patch safely"
                        except Exception as exc_inner:
                            results.append(
                                AgentEditResult(
                                    file_path=edit.file_path,
                                    success=False,
                                    message=f"Error applying patch: {str(exc_inner)}",
                                )
                            )
                            continue
                    else:
                        results.append(
                            AgentEditResult(file_path=edit.file_path, success=False, message=f"Unknown operation: {edit.op}")
                        )
                        continue

                    if new_content == old_content:
                        results.append(AgentEditResult(file_path=edit.file_path, success=True, message="No changes", diff=""))
                        continue

                    diff_lines = list(
                        unified_diff(
                            old_content.split("\n"),
                            new_content.split("\n"),
                            fromfile=file_path.name,
                            tofile=file_path.name,
                            lineterm="",
                        )
                    )
                    diff_str = "\n".join(diff_lines)

                    result = await save_with_etag_history_cache(
                        file_path,
                        new_content,
                        if_match=None,
                        history_db=state.history_db,
                        history_command="ai-agent-apply",
                        assume_locked=True,
                    )

                    if not result.get("success", False):
                        results.append(
                            AgentEditResult(
                                file_path=edit.file_path,
                                success=False,
                                message=f"Write failed: {result.get('error', 'unknown')}",
                                conflict=result.get("conflict"),
                            )
                        )
                        continue

                    results.append(AgentEditResult(file_path=edit.file_path, success=True, message=message, diff=diff_str))
            except HTTPException as exc_http:
                results.append(AgentEditResult(file_path=edit.file_path, success=False, message=f"HTTP error: {exc_http.detail}"))
            except Exception as exc_generic:
                results.append(AgentEditResult(file_path=edit.file_path, success=False, message=f"Error: {str(exc_generic)}"))

        if req.auto_compile and results and any(result.success for result in results):
            try:
                main_file = None
                if results[0].success:
                    main_file = validate_path(results[0].file_path, access_type="read")
                if main_file:
                    compile_result = await run_in_threadpool(compile_file, main_file, timeout=req.compile_timeout)
                    if hasattr(compile_result, "__dict__"):
                        compile_result = compile_result.__dict__
            except Exception as exc_compile:
                logging.error(f"Auto-compile failed: {exc_compile}")
                compile_result = {"error": str(exc_compile)}

        return AgentResponse(success=all(r.success for r in results), results=results, compile_result=compile_result)
    except Exception as exc:
        raise HTTPException(status_code=400, detail=f"Agent apply failed: {str(exc)}")


@router.post("/api/agent-loop/stream")
async def agent_loop_stream(req: AgentLoopRequest):
    from json import dumps as json_dumps

    async def event_generator():
        planner = get_ai_planner()
        file_path = validate_path(req.file_path, access_type="read")
        iteration = 0
        last_error = None
        try:
            data = json_dumps({"type": "thinking", "iteration": 0, "data": {"message": "Loading document context..."}})
            yield f"data: {data}\n\n"
            while iteration < req.max_iterations:
                iteration += 1
                bundle = DocumentBundle(file_path, workspace_root=get_workspace_root())
                await bundle.load_async()

                msg = f"Iteration {iteration}: proposing edits..."
                data = json_dumps({"type": "thinking", "iteration": iteration, "data": {"message": msg}})
                yield f"data: {data}\n\n"

                try:
                    # Generate multi-file structured edits using planner
                    prompt_base = req.instruction
                    if last_error:
                        prompt_base += f"\n\nPrevious compilation error:\n{last_error}"

                    agent_edits: List[AgentEdit] = await run_in_threadpool(planner.generate_edits, bundle, prompt_base, last_error or "")
                    proposal_edits = [{"file_path": e.file_path, "op": e.op} for e in agent_edits]
                    # Compute richer proposal diffs and ensure ETags on edits
                    diffs_obj = await compute_proposal_diffs(bundle, agent_edits)
                    etags_map = await capture_etags(agent_edits)
                    for e in agent_edits:
                        if e.file_path in etags_map:
                            e.expected_etag = etags_map[e.file_path]
                    proposal_diffs_obj = diffs_obj

                    if not agent_edits:
                        data = json_dumps({"type": "error", "iteration": iteration, "data": {"message": "AI did not propose any edits"}})
                        yield f"data: {data}\n\n"
                        break

                    summary = f"Proposed edits for {len(agent_edits)} file(s)"
                    data = json_dumps(
                        {
                            "type": "proposal",
                            "iteration": iteration,
                            "data": {
                                "summary": summary,
                                "edit_count": len(agent_edits),
                                "edits": proposal_edits,
                                "diffs": proposal_diffs_obj,
                            },
                        }
                    )
                    yield f"data: {data}\n\n"

                    if not req.auto_apply:
                        # In approval mode, pause after proposal and let the frontend decide.
                        data = json_dumps(
                            {
                                "type": "awaiting_approval",
                                "iteration": iteration,
                                "data": {"message": "Proposal ready for review and approval"},
                            }
                        )
                        yield f"data: {data}\n\n"
                        return

                    data = json_dumps(
                        {"type": "applying", "iteration": iteration, "data": {"message": f"Applying {len(agent_edits)} edit(s)..."}}
                    )
                    yield f"data: {data}\n\n"

                    # Use shared apply logic instead of calling endpoint
                    apply_result = await apply_agent_edits(
                        agent_edits,
                        history_db=state.history_db,
                        auto_compile=req.auto_compile,
                        main_file=file_path if req.auto_compile else None,
                    )

                    if req.auto_compile and apply_result.get("compile_result") is not None:
                        cr = apply_result.get("compile_result") or {}
                        ok = cr.get("error") is None and not cr.get("errors")
                        summary_err = None
                        if not ok:
                            errs = cr.get("errors", []) or []
                            summary_err = "\n".join([f"{e.get('line','?')}: {e.get('message','unknown')}" for e in errs[:3]])
                        data = json_dumps(
                            {"type": "compiled", "iteration": iteration, "data": {"success": bool(ok), "error_summary": summary_err}}
                        )
                        yield f"data: {data}\n\n"
                        if not ok and iteration < req.max_iterations:
                            last_error = summary_err or "Compilation errors"
                            # Optionally refine instruction from structured errors
                            errs = cr.get("errors") or []
                            refined = generate_fix_instruction(errs)
                            if refined:
                                last_error = refined
                            continue
                        elif not ok:
                            data = json_dumps(
                                {
                                    "type": "done",
                                    "iteration": iteration,
                                    "data": {"success": False, "message": "Max iterations reached with errors"},
                                }
                            )
                            yield f"data: {data}\n\n"
                            break
                    else:
                        data = json_dumps({"type": "done", "iteration": iteration, "data": {"success": True}})
                        yield f"data: {data}\n\n"
                        break

                    if req.auto_compile:
                        data = json_dumps({"type": "compiling", "iteration": iteration, "data": {"message": "Compiling..."}})
                        yield f"data: {data}\n\n"
                        try:
                            compile_result = await run_in_threadpool(compile_file, file_path, timeout=60)
                            errors = compile_result.get("errors", []) if isinstance(compile_result, dict) else getattr(compile_result, "errors", [])
                            if errors:
                                last_error = "\n".join([f"{e.get('line', '?')}: {e.get('message', 'unknown')}" for e in errors[:3]])
                                data = json_dumps(
                                    {
                                        "type": "compiled",
                                        "iteration": iteration,
                                        "data": {"success": False, "error_count": len(errors), "error_summary": last_error},
                                    }
                                )
                                yield f"data: {data}\n\n"
                                if iteration < req.max_iterations:
                                    data = json_dumps({"type": "thinking", "iteration": iteration, "data": {"message": "Proposing fixes..."}})
                                    yield f"data: {data}\n\n"
                                    continue
                                else:
                                    data = json_dumps(
                                        {
                                            "type": "done",
                                            "iteration": iteration,
                                            "data": {"success": False, "message": "Max iterations reached with errors"},
                                        }
                                    )
                                    yield f"data: {data}\n\n"
                                    break
                            else:
                                page_count = compile_result.get("page_count", 0)
                                data = json_dumps({"type": "compiled", "iteration": iteration, "data": {"success": True, "page_count": page_count}})
                                yield f"data: {data}\n\n"
                                data = json_dumps({"type": "done", "iteration": iteration, "data": {"success": True, "message": "Compilation successful"}})
                                yield f"data: {data}\n\n"
                                break
                        except Exception as exc_compile:
                            logging.error(f"Compile error: {exc_compile}")
                            last_error = str(exc_compile)
                            error_msg = f"Compile error: {exc_compile}"
                            data = json_dumps({"type": "error", "iteration": iteration, "data": {"message": error_msg}})
                            yield f"data: {data}\n\n"
                            continue
                    else:
                        data = json_dumps(
                            {"type": "done", "iteration": iteration, "data": {"success": True, "message": "Edits applied (no compilation)"}}
                        )
                        yield f"data: {data}\n\n"
                        break
                except Exception as exc_inner:
                    logging.error(f"Agent loop iteration {iteration} failed: {exc_inner}")
                    data = json_dumps({"type": "error", "iteration": iteration, "data": {"message": str(exc_inner)}})
                    yield f"data: {data}\n\n"
                    break
        except Exception as exc_outer:
            logging.error(f"Agent loop failed: {exc_outer}")
            yield f"data: {json_dumps({'type': 'error', 'iteration': 0, 'data': {'message': str(exc_outer)}})}\n\n"

    return _get_streaming_response(event_generator)


# Undo/Redo endpoints using history database
@router.post("/api/undo")
async def undo_last_edit(req: dict):
    """Undo last agent edit for a file by restoring previous history entry."""
    try:
        if not state.history_db:
            raise HTTPException(status_code=400, detail="History database not initialized")
        if "file_path" not in req:
            raise HTTPException(status_code=400, detail="file_path required")

        file_path = validate_path(req["file_path"], access_type="write")
        
        # Use atomic operation to prevent race conditions
        previous_entry = await run_in_threadpool(state.history_db.atomic_undo, str(file_path))
        
        if not previous_entry:
            return {"success": False, "message": "No agent edits to undo"}
        
        # Restore file with lock
        lock = get_file_lock(file_path)
        async with lock:
            from latex_ai.services.files import atomic_write
            await atomic_write(file_path, previous_entry.new_text)
        
        return {
            "success": True,
            "message": f"Restored to version {previous_entry.id}",
            "restored_id": previous_entry.id
        }
    except HTTPException:
        raise
    except Exception as exc:
        raise HTTPException(status_code=400, detail=f"Undo failed: {str(exc)}")


@router.post("/api/redo")
async def redo_edit(req: dict):
    """Redo a previously undone edit by restoring a specific history entry."""
    try:
        if not state.history_db:
            raise HTTPException(status_code=400, detail="History database not initialized")
        file_path_str = req.get("file_path")
        if not file_path_str:
            raise HTTPException(status_code=400, detail="file_path required")

        file_path = validate_path(file_path_str, access_type="write")
        history_id = req.get("history_id")

        if history_id is not None:
            # Restore the exact entry requested
            entry = await run_in_threadpool(state.history_db.get_entry, int(history_id))
            if not entry:
                raise HTTPException(status_code=404, detail=f"History entry not found: {history_id}")
            if Path(entry.file_path) != file_path:
                raise HTTPException(status_code=400, detail="History entry does not belong to requested file")
            target_entry = entry
        else:
            # Use atomic operation to redo next entry safely
            target_entry = await run_in_threadpool(state.history_db.atomic_redo, str(file_path))
            if not target_entry:
                return {"success": False, "message": "No redo available"}
        
        # Restore file with lock
        lock = get_file_lock(file_path)
        async with lock:
            from latex_ai.services.files import atomic_write
            await atomic_write(file_path, target_entry.new_text)
        
        return {
            "success": True,
            "message": f"Restored to history entry {target_entry.id}",
            "restored_id": target_entry.id
        }
    except HTTPException:
        raise
    except Exception as exc:
        raise HTTPException(status_code=400, detail=f"Redo failed: {str(exc)}")
