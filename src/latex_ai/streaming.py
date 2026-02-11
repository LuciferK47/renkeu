"""SSE and streaming utilities for API endpoints."""

import json
from typing import Any, AsyncIterator


async def stream_sse_events(event_iter: AsyncIterator[Any]) -> AsyncIterator[str]:
    """
    Convert an async event iterator into SSE (Server-Sent Events) format.
    
    Each item from event_iter should be a dict with optional 'event' and 'data' keys.
    
    Args:
        event_iter: Async iterator yielding dict items
        
    Yields:
        SSE-formatted strings (ready for StreamingResponse)
    """
    async for item in event_iter:
        if isinstance(item, dict):
            event_type = item.get("event", "message")
            data = item.get("data", "")
        else:
            event_type = "message"
            data = item
        
        # Format: "event: <type>\ndata: <json>\n\n"
        yield f"event: {event_type}\n"
        yield f"data: {json.dumps(data) if not isinstance(data, str) else data}\n\n"


async def stream_json_events(event_iter: AsyncIterator[Any]) -> AsyncIterator[str]:
    """
    Convert an async event iterator into newline-delimited JSON (NDJSON) format.
    
    Args:
        event_iter: Async iterator yielding dict or JSON-serializable items
        
    Yields:
        Newline-delimited JSON strings
    """
    async for item in event_iter:
        yield json.dumps(item) + "\n"
