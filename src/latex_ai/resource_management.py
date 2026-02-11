"""Resource management and security enhancements for compilation queue and AI services.

Implements:
- Timeout management for compilation jobs
- Disk quota and space checks
- Per-user rate limiting
- Memory management with LRU eviction
- Cancellation token support for AI streaming
"""

import asyncio
import time
from dataclasses import dataclass, field
from typing import Dict, Optional
from collections import OrderedDict
from pathlib import Path


[TIMEOUT_MANAGEMENT]
# Configuration constants
DEFAULT_COMPILE_TIMEOUT = 30  # seconds, for single pdflatex run
MAX_COMPILE_TIMEOUT = 120  # absolute maximum timeout
DISK_QUOTA_BYTES = 1 * 1024 * 1024 * 1024  # 1GB per user
DISK_CLEANUP_THRESHOLD = 0.8  # Clean up when 80% full
RATE_LIMIT_PER_USER = 10  # max requests per minute
RATE_LIMIT_WINDOW = 60  # seconds


class CompilationTimeout(asyncio.TimeoutError):
    """Raised when compilation exceeds timeout."""
    pass


class DiskQuotaExceeded(Exception):
    """Raised when user's disk quota is exceeded."""
    pass


class RateLimitExceeded(Exception):
    """Raised when user exceeds rate limit."""
    pass


@dataclass
class TimeoutControl:
    """Manages timeout for a compilation job with cancellation support."""
    job_id: str
    timeout_seconds: int = DEFAULT_COMPILE_TIMEOUT
    start_time: float = field(default_factory=time.time)
    cancel_event: asyncio.Event = field(default_factory=asyncio.Event)
    completed: bool = False
    
    async def check_timeout(self) -> bool:
        """Check if timeout has been exceeded.
        
        Returns:
            True if timed out, False otherwise
        """
        elapsed = time.time() - self.start_time
        if elapsed > self.timeout_seconds:
            self.cancel_event.set()
            return True
        return False
    
    async def wait_with_timeout(self, coro):
        """Execute coroutine with timeout enforcement.
        
        Args:
            coro: Coroutine to execute
            
        Returns:
            Result from coroutine
            
        Raises:
            CompilationTimeout: If timeout exceeded
        """
        try:
            return await asyncio.wait_for(coro, timeout=self.timeout_seconds)
        except asyncio.TimeoutError as e:
            self.cancel_event.set()
            raise CompilationTimeout(
                f"Compilation of job {self.job_id} exceeded {self.timeout_seconds}s timeout"
            ) from e
    
    def mark_complete(self):
        """Mark job as completed (stop timeout)."""
        self.completed = True
        self.cancel_event.set()


[DISK_QUOTA_MANAGEMENT]


class DiskQuotaManager:
    """Manages per-user disk quotas and cleanup."""
    
    def __init__(self, base_dir: Path, quota_bytes: int = DISK_QUOTA_BYTES):
        """Initialize quota manager.
        
        Args:
            base_dir: Base directory for user data
            quota_bytes: Total quota per user
        """
        self.base_dir = Path(base_dir)
        self.quota_bytes = quota_bytes
        self._user_sizes: Dict[str, int] = {}
        self._last_cleanup: Dict[str, float] = {}
    
    def get_user_dir(self, user_id: str) -> Path:
        """Get directory for user's compilations.
        
        Args:
            user_id: User identifier
            
        Returns:
            Path to user's directory
        """
        user_dir = self.base_dir / user_id
        user_dir.mkdir(parents=True, exist_ok=True)
        return user_dir
    
    def check_quota(self, user_id: str) -> Dict[str, any]:
        """Check user's disk usage against quota.
        
        Args:
            user_id: User identifier
            
        Returns:
            Dict with usage_bytes, quota_bytes, percentage_used
            
        Raises:
            DiskQuotaExceeded: If quota is exceeded
        """
        user_dir = self.get_user_dir(user_id)
        
        # Calculate total size
        total_size = sum(
            f.stat().st_size for f in user_dir.rglob('*') if f.is_file()
        )
        
        self._user_sizes[user_id] = total_size
        percentage = (total_size / self.quota_bytes) * 100
        
        result = {
            'usage_bytes': total_size,
            'quota_bytes': self.quota_bytes,
            'percentage_used': percentage
        }
        
        # Trigger cleanup if approaching limit
        if percentage > DISK_CLEANUP_THRESHOLD:
            self._cleanup_old_files(user_id)
        
        # Raise if exceeded
        if total_size > self.quota_bytes:
            raise DiskQuotaExceeded(
                f"User {user_id} exceeded disk quota: "
                f"{total_size / 1024 / 1024:.1f}MB of {self.quota_bytes / 1024 / 1024:.1f}MB"
            )
        
        return result
    
    def _cleanup_old_files(self, user_id: str, keep_count: int = 5):
        """Remove oldest files when quota is approached.
        
        Keeps only the most recent keep_count compilations.
        
        Args:
            user_id: User identifier
            keep_count: Number of recent files to keep
        """
        user_dir = self.get_user_dir(user_id)
        
        # Get all PDF files sorted by modification time
        pdf_files = sorted(
            user_dir.glob('*.pdf'),
            key=lambda f: f.stat().st_mtime,
            reverse=True
        )
        
        # Remove files beyond keep_count
        for pdf in pdf_files[keep_count:]:
            try:
                pdf.unlink()
                # Also remove associated log/aux files
                for ext in ['.log', '.aux', '.fls', '.fdb_latexmk']:
                    aux_file = pdf.with_suffix(ext)
                    if aux_file.exists():
                        aux_file.unlink()
            except OSError:
                pass


[RATE_LIMITING]


class RateLimiter:
    """Per-user rate limiting for compilation requests."""
    
    def __init__(self, limit: int = RATE_LIMIT_PER_USER, window: int = RATE_LIMIT_WINDOW):
        """Initialize rate limiter.
        
        Args:
            limit: Max requests per window
            window: Time window in seconds
        """
        self.limit = limit
        self.window = window
        self._user_requests: Dict[str, list[float]] = {}  # user_id -> [timestamp, ...]
    
    def check_limit(self, user_id: str) -> Dict[str, any]:
        """Check if user has exceeded rate limit.
        
        Args:
            user_id: User identifier
            
        Returns:
            Dict with remaining, limit, reset_in_seconds
            
        Raises:
            RateLimitExceeded: If limit exceeded
        """
        now = time.time()
        cutoff = now - self.window
        
        # Get user's request history
        if user_id not in self._user_requests:
            self._user_requests[user_id] = []
        
        # Filter to only requests in current window
        recent = [ts for ts in self._user_requests[user_id] if ts > cutoff]
        self._user_requests[user_id] = recent
        
        # Check limit
        remaining = self.limit - len(recent)
        
        result = {
            'remaining': max(0, remaining),
            'limit': self.limit,
            'reset_in_seconds': (self._user_requests[user_id][0] + self.window - now) if recent else 0
        }
        
        if len(recent) >= self.limit:
            raise RateLimitExceeded(
                f"User {user_id} exceeded rate limit: {self.limit} requests per {self.window}s. "
                f"Reset in {result['reset_in_seconds']:.0f}s"
            )
        
        # Record this request
        self._user_requests[user_id].append(now)
        
        return result


[MEMORY_MANAGEMENT]


class LRUCompilationCache:
    """LRU cache for compilation results with max size limits."""
    
    def __init__(self, max_items: int = 100, max_age_seconds: int = 3600):
        """Initialize LRU cache.
        
        Args:
            max_items: Maximum number of cached compilations
            max_age_seconds: Maximum age before eviction
        """
        self.max_items = max_items
        self.max_age_seconds = max_age_seconds
        self._cache: OrderedDict[str, tuple[any, float]] = OrderedDict()  # hash -> (result, timestamp)
    
    def get(self, hash_key: str) -> Optional[any]:
        """Get cached result if not expired.
        
        Args:
            hash_key: Bundle hash key
            
        Returns:
            Cached result or None if expired/missing
        """
        if hash_key not in self._cache:
            return None
        
        result, timestamp = self._cache[hash_key]
        
        # Check age
        if time.time() - timestamp > self.max_age_seconds:
            del self._cache[hash_key]
            return None
        
        # Move to end (most recently used)
        self._cache.move_to_end(hash_key)
        return result
    
    def put(self, hash_key: str, result: any):
        """Cache compilation result.
        
        Args:
            hash_key: Bundle hash key
            result: Compilation result
        """
        # Remove if already exists (to re-add at end)
        if hash_key in self._cache:
            del self._cache[hash_key]
        
        # Add to cache
        self._cache[hash_key] = (result, time.time())
        
        # Evict oldest if exceeding max
        while len(self._cache) > self.max_items:
            oldest_key = next(iter(self._cache))
            del self._cache[oldest_key]
    
    def clear(self):
        """Clear entire cache."""
        self._cache.clear()
    
    def get_stats(self) -> Dict[str, any]:
        """Get cache statistics.
        
        Returns:
            Dict with size, max_size, items
        """
        return {
            'size': len(self._cache),
            'max_size': self.max_items,
            'items': list(self._cache.keys())
        }


[AI_CANCELLATION_TOKENS]


class AIStreamingCancellation:
    """Manages cancellation tokens for AI streaming operations.
    
    Allows stopping AI work on client disconnect or user request.
    """
    
    def __init__(self, session_id: str):
        """Initialize cancellation control.
        
        Args:
            session_id: Unique session identifier
        """
        self.session_id = session_id
        self.cancel_event = asyncio.Event()
        self.completed = False
        self.error: Optional[str] = None
    
    def should_cancel(self) -> bool:
        """Check if cancellation has been requested.
        
        Returns:
            True if cancel event is set
        """
        return self.cancel_event.is_set()
    
    def cancel(self, reason: str = "User requested cancellation"):
        """Request cancellation of AI operation.
        
        Args:
            reason: Reason for cancellation
        """
        self.error = reason
        self.cancel_event.set()
    
    async def sleep_interruptible(self, seconds: float):
        """Sleep that can be interrupted by cancellation.
        
        Args:
            seconds: Time to sleep
            
        Returns:
            True if interrupted, False if completed normally
        """
        try:
            await asyncio.wait_for(
                self.cancel_event.wait(),
                timeout=seconds
            )
            return True  # Cancelled
        except asyncio.TimeoutError:
            return False  # Completed normally


# Global instances
_quota_manager: Optional[DiskQuotaManager] = None
_rate_limiter: Optional[RateLimiter] = None
_compilation_cache: Optional[LRUCompilationCache] = None


def get_quota_manager(base_dir: Path = None) -> DiskQuotaManager:
    """Get or create global quota manager."""
    global _quota_manager
    if _quota_manager is None:
        if base_dir is None:
            base_dir = Path.home() / '.latex-ai' / 'compilations'
        _quota_manager = DiskQuotaManager(base_dir)
    return _quota_manager


def get_rate_limiter() -> RateLimiter:
    """Get or create global rate limiter."""
    global _rate_limiter
    if _rate_limiter is None:
        _rate_limiter = RateLimiter()
    return _rate_limiter


def get_compilation_cache() -> LRUCompilationCache:
    """Get or create global compilation cache."""
    global _compilation_cache
    if _compilation_cache is None:
        _compilation_cache = LRUCompilationCache()
    return _compilation_cache
