"""
SSE (Server-Sent Events) streaming utilities for FastAPI.

Provides reusable helpers for streaming responses in a consistent format.
"""
import json
import asyncio
import threading
import logging
from typing import AsyncIterator, Any, Dict, Optional, Callable

try:
    from sse_starlette.sse import ServerSentEvent
    HAS_SSE_STARLETTE = True
except ImportError:
    HAS_SSE_STARLETTE = False

logger = logging.getLogger(__name__)


async def safe_stream_with_thread(
    thread_func: Callable[[asyncio.Queue, threading.Event], None],
    timeout: float = 30.0
) -> AsyncIterator[dict]:
    """Safely stream from a thread with cleanup on cancellation.
    
    Args:
        thread_func: Function that runs in thread, receives (queue, stop_event)
        timeout: Timeout for queue reads (default 30s)
        
    Yields:
        Event dictionaries from the thread
        
    Example:
        def worker(queue, stop_event):
            for i in range(10):
                if stop_event.is_set():
                    break
                queue.put({"value": i})
            queue.put(None)  # Sentinel
        
        async for event in safe_stream_with_thread(worker):
            yield event
    """
    queue: asyncio.Queue = asyncio.Queue()
    stop_event = threading.Event()
    
    # Wrapper to bridge sync queue to async queue
    def thread_wrapper():
        import queue as sync_queue
        thread_queue = sync_queue.Queue()
        
        def put_to_async(item):
            asyncio.run_coroutine_threadsafe(queue.put(item), asyncio.get_event_loop())
        
        try:
            thread_func(thread_queue, stop_event)
        except Exception as e:
            logger.error(f"Thread error: {e}")
            put_to_async({"type": "error", "data": {"message": str(e)}})
        finally:
            put_to_async(None)  # Sentinel
    
    thread = threading.Thread(target=thread_wrapper, daemon=True)
    thread.start()
    
    try:
        while True:
            try:
                event = await asyncio.wait_for(queue.get(), timeout=timeout)
                if event is None:  # Sentinel for completion
                    break
                yield event
            except asyncio.TimeoutError:
                yield {"type": "keepalive", "data": {}}
    except asyncio.CancelledError:
        # Client disconnected - stop thread
        logger.info("Stream cancelled, stopping thread")
        stop_event.set()
        raise
    finally:
        stop_event.set()
        thread.join(timeout=1.0)
        if thread.is_alive():
            logger.warning("Thread did not stop cleanly")


async def sse_stream(
    event_iter: AsyncIterator[Dict[str, Any]],
    event_type: Optional[str] = None
) -> AsyncIterator[str]:
    """
    Convert async event iterator to SSE format.
    
    Args:
        event_iter: Async iterator yielding event dictionaries
        event_type: Optional event type for all events
        
    Yields:
        SSE-formatted strings ready for StreamingResponse
        
    Example:
        async def generate_events():
            yield {"status": "started"}
            yield {"status": "processing", "progress": 50}
            yield {"status": "done"}
            
        return StreamingResponse(
            sse_stream(generate_events()),
            media_type="text/event-stream"
        )
    """
    if HAS_SSE_STARLETTE:
        # Use sse-starlette if available
        async for event_data in event_iter:
            sse = ServerSentEvent(data=json.dumps(event_data), event=event_type)
            yield sse
    else:
        # Fallback to manual SSE formatting
        async for event_data in event_iter:
            data_str = json.dumps(event_data)
            if event_type:
                yield f"event: {event_type}\n"
            yield f"data: {data_str}\n\n"


async def sse_message(event: str, data: Any) -> str:
    """
    Format a single SSE message.
    
    Args:
        event: Event type
        data: Event data (will be JSON-serialized)
        
    Returns:
        SSE-formatted string
    """
    data_str = json.dumps(data) if not isinstance(data, str) else data
    if HAS_SSE_STARLETTE:
        sse = ServerSentEvent(data=data_str, event=event)
        return sse
    else:
        return f"event: {event}\ndata: {data_str}\n\n"


class SSEResponse:
    """
    Helper class for creating SSE streaming responses.
    
    Usage:
        @app.get("/stream")
        async def stream_endpoint():
            async def events():
                yield {"type": "start"}
                yield {"type": "progress", "value": 50}
                yield {"type": "done"}
            return SSEResponse(events())
    """
    
    def __init__(self, event_iter: AsyncIterator[Dict[str, Any]], event_type: Optional[str] = None):
        self.event_iter = event_iter
        self.event_type = event_type
        
    async def __call__(self):
        """Make this compatible with StreamingResponse."""
        async for chunk in sse_stream(self.event_iter, self.event_type):
            yield chunk
