"""Rate limiting for API endpoints with session support."""

import asyncio
import time
from collections import defaultdict
from typing import Dict, List, Optional


class RateLimiter:
    """Simple in-memory rate limiter using sliding window."""
    
    def __init__(self, max_requests: int, window_seconds: int):
        """Initialize rate limiter.
        
        Args:
            max_requests: Maximum requests allowed in window
            window_seconds: Time window in seconds
        """
        self.max_requests = max_requests
        self.window = window_seconds
        self.requests: Dict[str, List[float]] = defaultdict(list)
        self._lock = asyncio.Lock()
    
    async def check_limit(self, client_id: str) -> bool:
        """Check if client is within rate limit.
        
        Args:
            client_id: Unique identifier for client (IP, user ID, session ID, etc.)
            
        Returns:
            True if within limit, False if exceeded
        """
        async with self._lock:
            now = time.time()
            cutoff = now - self.window
            
            # Clean old requests outside window
            self.requests[client_id] = [
                t for t in self.requests[client_id] if t > cutoff
            ]
            
            # Check if limit exceeded
            if len(self.requests[client_id]) >= self.max_requests:
                return False
            
            # Record this request
            self.requests[client_id].append(now)
            return True
    
    async def get_remaining(self, client_id: str) -> int:
        """Get remaining requests for client.
        
        Args:
            client_id: Unique identifier for client
            
        Returns:
            Number of requests remaining in current window
        """
        async with self._lock:
            now = time.time()
            cutoff = now - self.window
            
            # Clean old requests
            self.requests[client_id] = [
                t for t in self.requests[client_id] if t > cutoff
            ]
            
            return max(0, self.max_requests - len(self.requests[client_id]))
    
    async def reset(self, client_id: str = None):
        """Reset rate limit for a client or all clients.
        
        Args:
            client_id: Client to reset, or None to reset all
        """
        async with self._lock:
            if client_id:
                self.requests.pop(client_id, None)
            else:
                self.requests.clear()


class SessionBasedRateLimiter:
    """Session-aware rate limiter with per-session limits."""
    
    def __init__(self):
        """Initialize session-based rate limiter with standard limits."""
        # Per-session limits: 100 compiles/min, 10 AI calls/min
        self.compile_limiter = RateLimiter(max_requests=100, window_seconds=60)
        self.ai_call_limiter = RateLimiter(max_requests=10, window_seconds=60)
        self._lock = asyncio.Lock()
    
    async def check_compile_limit(self, session_id: str) -> bool:
        """Check if session can submit a compilation job.
        
        Args:
            session_id: Unique session identifier
            
        Returns:
            True if within limit, False if exceeded
        """
        return await self.compile_limiter.check_limit(f"compile:{session_id}")
    
    async def check_ai_call_limit(self, session_id: str) -> bool:
        """Check if session can make an AI planning call.
        
        Args:
            session_id: Unique session identifier
            
        Returns:
            True if within limit, False if exceeded
        """
        return await self.ai_call_limiter.check_limit(f"ai:{session_id}")
    
    async def get_compile_remaining(self, session_id: str) -> int:
        """Get remaining compile requests for session.
        
        Args:
            session_id: Unique session identifier
            
        Returns:
            Number of compile requests remaining in window
        """
        return await self.compile_limiter.get_remaining(f"compile:{session_id}")
    
    async def get_ai_remaining(self, session_id: str) -> int:
        """Get remaining AI call requests for session.
        
        Args:
            session_id: Unique session identifier
            
        Returns:
            Number of AI calls remaining in window
        """
        return await self.ai_call_limiter.get_remaining(f"ai:{session_id}")
    
    async def reset_session(self, session_id: str):
        """Reset all rate limits for a session.
        
        Args:
            session_id: Unique session identifier
        """
        await self.compile_limiter.reset(f"compile:{session_id}")
        await self.ai_call_limiter.reset(f"ai:{session_id}")


# Global rate limiters for different operations
compile_limiter = RateLimiter(max_requests=10, window_seconds=60)  # 10 compilations per minute
api_limiter = RateLimiter(max_requests=100, window_seconds=60)  # 100 API calls per minute
