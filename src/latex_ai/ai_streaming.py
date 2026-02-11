"""Async streaming generators for LaTeX AI operations.

These functions were extracted from AIPlanner to reduce file size.
Each accepts an `AIPlanner` instance as first parameter so it can
access the Gemini client, model name, and helper methods.
"""
from __future__ import annotations

import asyncio
import logging
import re
from typing import TYPE_CHECKING, Optional

from .ai_prompts import (
    create_suggestions_prompt,
    create_review_prompt,
    create_refinement_prompt,
    create_error_prompt,
)

if TYPE_CHECKING:
    from .ai_planner import AIPlanner
    from .document import Document

logger = logging.getLogger(__name__)


async def suggest_improvements_streaming(
    planner: "AIPlanner",
    document: "Document",
    session_id: Optional[str] = None,
):
    """Stream proactive improvement suggestions for a LaTeX document.

    Yields SSE-compatible events with keys:
        type in {progress, assessment, strength, suggestion, complete, error}.
    """
    try:
        context = planner._prepare_smart_context(
            document, "analyze structure and suggest improvements"
        )
        prompt = create_suggestions_prompt(context)

        yield {"type": "progress", "data": {"message": "Analyzing document structure", "progress": 0.2}}

        response = planner.client.models.generate_content(
            model=planner.model_name,
            contents=prompt,
        )

        yield {"type": "progress", "data": {"message": "Generating suggestions", "progress": 0.6}}

        parsed = planner._parse_response(response.text)

        if parsed.get("overall_assessment"):
            yield {"type": "assessment", "data": {"text": parsed["overall_assessment"]}}

        for strength in parsed.get("strengths", [])[:3]:
            yield {"type": "strength", "data": {"text": strength}}

        for suggestion in parsed.get("suggestions", [])[:5]:
            yield {
                "type": "suggestion",
                "data": {
                    "category": suggestion.get("category", "General"),
                    "issue": suggestion.get("issue", ""),
                    "suggestion": suggestion.get("suggestion", ""),
                    "impact": suggestion.get("impact", ""),
                    "priority": suggestion.get("priority", "medium"),
                },
            }

        if session_id:
            summary = f"Document Assessment:\n{parsed.get('overall_assessment', 'Document reviewed.')}\n\n"
            summary += "Key Suggestions:\n"
            for s in parsed.get("suggestions", [])[:3]:
                summary += f"\u2022 [{s.get('priority', 'medium').upper()}] {s.get('issue', '')}: {s.get('suggestion', '')}\n"
            planner.add_to_conversation(session_id, "assistant", summary.strip())

        yield {"type": "complete", "data": parsed}

    except Exception as e:
        logger.error(f"Suggestions generation failed: {e}")
        yield {"type": "error", "data": {"message": str(e)}}


async def review_document_streaming(
    planner: "AIPlanner",
    document: "Document",
    session_id: Optional[str] = None,
):
    """Stream comprehensive document review analysis.

    Yields SSE-compatible events with keys:
        type in {progress, score, summary, section, action, complete, error}.
    """
    try:
        context = document.text  # Use full document for review
        prompt = create_review_prompt(context)

        yield {"type": "progress", "data": {"message": "Starting comprehensive review", "progress": 0.1}}

        response = planner.client.models.generate_content(
            model=planner.model_name,
            contents=prompt,
        )

        yield {"type": "progress", "data": {"message": "Analyzing review results", "progress": 0.7}}

        parsed = planner._parse_response(response.text)

        if "review_score" in parsed:
            yield {"type": "score", "data": {"score": parsed["review_score"]}}

        if "executive_summary" in parsed:
            yield {"type": "summary", "data": {"text": parsed["executive_summary"]}}

        for section in ["structure_analysis", "package_analysis", "style_analysis", "best_practices"]:
            if section in parsed:
                yield {
                    "type": "section",
                    "data": {
                        "name": section.replace("_", " ").title(),
                        "rating": parsed[section].get("rating", "N/A"),
                        "findings": parsed[section].get("findings", []),
                        "issues": parsed[section].get("issues", []),
                        "violations": parsed[section].get("violations", []),
                        "missing_packages": parsed[section].get("missing_packages", []),
                        "unused_packages": parsed[section].get("unused_packages", []),
                        "recommendations": parsed[section].get("recommendations", []),
                    },
                }

        for action in parsed.get("priority_actions", [])[:8]:
            yield {
                "type": "action",
                "data": {
                    "priority": action.get("priority", "medium"),
                    "action": action.get("action", ""),
                    "reason": action.get("reason", ""),
                    "code": action.get("code", ""),
                },
            }

        if session_id:
            summary = f"Code Review Complete (Score: {parsed.get('review_score', 'N/A')}/100)\n\n"
            summary += f"{parsed.get('executive_summary', 'Review completed.')}\n\n"
            summary += "Top Priority Actions:\n"
            for a in parsed.get("priority_actions", [])[:3]:
                summary += f"\u2022 [{a.get('priority', 'medium').upper()}] {a.get('action', '')}\n"
            planner.add_to_conversation(session_id, "assistant", summary.strip())

        yield {"type": "complete", "data": parsed}

    except Exception as e:
        logger.error(f"Document review failed: {e}")
        yield {"type": "error", "data": {"message": str(e)}}


async def refine_document_streaming(
    planner: "AIPlanner",
    document: "Document",
    refinement_request: str,
    session_id: Optional[str] = None,
    conversation_context: Optional[list] = None,
):
    """Stream AI-driven document refinement suggestions.

    Yields SSE-compatible events with keys:
        type in {progress, summary, change, step, complete, error}.
    """
    try:
        context = planner._prepare_smart_context(document, "refine document")

        if conversation_context:
            conv_summary = "\n\n".join([
                f"{msg.get('role','user').upper()}: {msg.get('content','')[:200]}"
                for msg in conversation_context[-3:]
            ])
            context = f"Recent context:\n{conv_summary}\n\nDocument:\n{context}"

        prompt = create_refinement_prompt(context, refinement_request)

        yield {"type": "progress", "data": {"message": "Planning refinements", "progress": 0.2}}

        response = planner.client.models.generate_content(
            model=planner.model_name,
            contents=prompt,
        )

        yield {"type": "progress", "data": {"message": "Analyzing document changes", "progress": 0.6}}

        parsed = planner._parse_response(response.text)

        if parsed.get("summary"):
            yield {"type": "summary", "data": {"text": parsed["summary"]}}

        for change in parsed.get("changes", [])[:10]:
            yield {
                "type": "change",
                "data": {
                    "location": change.get("location", ""),
                    "action": change.get("action", ""),
                    "rationale": change.get("rationale", ""),
                },
            }

        for step in parsed.get("patch_steps", [])[:5]:
            yield {
                "type": "step",
                "data": {
                    "command": step.get("command", ""),
                    "description": step.get("description", ""),
                    "rationale": step.get("rationale", ""),
                },
            }

        if session_id:
            response_text = f"{parsed.get('summary', '')}\n"
            for change in parsed.get("changes", [])[:3]:
                response_text += f"\u2022 {change.get('action', '')}\n"
            planner.add_to_conversation(session_id, "assistant", response_text.strip())

        yield {"type": "complete", "data": parsed}

    except Exception as e:
        logger.error(f"Document refinement streaming failed: {e}")
        yield {"type": "error", "data": {"message": str(e)}}


async def explain_errors_streaming(
    planner: "AIPlanner",
    document: "Document",
    diagnostics: list[dict],
    session_id: Optional[str] = None,
    conversation_context: Optional[list] = None,
):
    """Stream AI explanation and suggested fixes for compilation issues.

    Yields SSE-compatible events with keys:
        type in {progress, explain, step, complete, error}.
    """
    try:
        context = planner._prepare_smart_context(document, "explain errors")

        if conversation_context:
            conv_summary = "\n\n".join([
                f"{msg.get('role','user').upper()}: {msg.get('content','')}"
                for msg in conversation_context
            ])
            context = f"Previous conversation:\n{conv_summary}\n\nDocument context:\n{context}"

        prompt = create_error_prompt(context, diagnostics)

        yield {"type": "progress", "data": {"message": "Analyzing issues", "progress": 0.2}}

        response = planner.client.models.generate_content(
            model=planner.model_name,
            contents=prompt,
        )

        yield {"type": "progress", "data": {"message": "Processing explanation", "progress": 0.6}}

        parsed = planner._parse_response(response.text)

        if parsed.get("explanation"):
            yield {
                "type": "explain",
                "data": {
                    "text": parsed["explanation"],
                    "causes": parsed.get("likely_causes", []),
                },
            }

        for step in parsed.get("patch_steps", [])[:5]:
            normalized = {
                "command": step.get("command", ""),
                "description": step.get("description", "Suggested fix"),
                "rationale": step.get("rationale", ""),
            }
            yield {"type": "step", "data": normalized}

        if session_id and parsed.get("explanation"):
            planner.add_to_conversation(session_id, "assistant", parsed.get("explanation"))

        yield {"type": "complete", "data": parsed}

    except Exception as e:
        logger.error(f"AI error explanation failed: {e}")
        yield {"type": "error", "data": {"message": str(e)}}


async def stream_tokens(
    planner: "AIPlanner",
    prompt: str,
    chunk_size: int = 200,
):
    """Generate content tokens as they arrive (async).

    Since the Gemini SDK doesn't provide true streaming yet,
    this simulates streaming by chunking the response into
    sentences/paragraphs and yielding them gradually.

    Args:
        planner: AIPlanner instance
        prompt: Input prompt
        chunk_size: Approximate character count per chunk (default 200)

    Yields:
        Text chunks with artificial delay for progressive rendering
    """
    result = await asyncio.get_event_loop().run_in_executor(
        None,
        lambda: planner.stream_generate(prompt),
    )

    sentence_pattern = r'([.!?]+[\s\n]+)'
    parts = re.split(sentence_pattern, result)

    current_chunk = ""
    for part in parts:
        current_chunk += part

        if len(current_chunk) >= chunk_size or '\n\n' in current_chunk:
            if current_chunk.strip():
                yield current_chunk
                current_chunk = ""
                await asyncio.sleep(0.02)

    if current_chunk.strip():
        yield current_chunk
