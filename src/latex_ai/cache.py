"""Intelligent caching layer for expensive operations."""

import asyncio
import hashlib
import time
from collections import OrderedDict
from typing import Any, Optional


class LRUCache:
    """Thread-safe LRU cache with expiration."""
    
    def __init__(self, max_size: int = 1000, ttl_seconds: int = 3600):
        """Initialize LRU cache.
        
        Args:
            max_size: Maximum number of entries
            ttl_seconds: Time-to-live for cached entries
        """
        self.max_size = max_size
        self.ttl_seconds = ttl_seconds
        self.cache: OrderedDict[str, tuple[Any, float]] = OrderedDict()
        self._lock = asyncio.Lock()
    
    async def get(self, key: str) -> Optional[Any]:
        """Get value from cache.
        
        Args:
            key: Cache key
            
        Returns:
            Cached value or None if not found/expired
        """
        async with self._lock:
            if key not in self.cache:
                return None
            
            value, timestamp = self.cache[key]
            
            # Check if expired
            if time.time() - timestamp > self.ttl_seconds:
                del self.cache[key]
                return None
            
            # Move to end (most recently used)
            self.cache.move_to_end(key)
            return value
    
    async def set(self, key: str, value: Any):
        """Set value in cache.
        
        Args:
            key: Cache key
            value: Value to cache
        """
        async with self._lock:
            # Remove if exists (will re-add at end)
            if key in self.cache:
                del self.cache[key]
            
            # Add new entry
            self.cache[key] = (value, time.time())
            
            # Evict oldest if over size limit
            if len(self.cache) > self.max_size:
                self.cache.popitem(last=False)
    
    async def delete(self, key: str):
        """Delete entry from cache.
        
        Args:
            key: Cache key
        """
        async with self._lock:
            if key in self.cache:
                del self.cache[key]
    
    async def clear(self):
        """Clear all cached entries."""
        async with self._lock:
            self.cache.clear()
    
    async def get_stats(self) -> dict:
        """Get cache statistics.
        
        Returns:
            Dictionary with cache stats
        """
        async with self._lock:
            return {
                'size': len(self.cache),
                'max_size': self.max_size,
                'utilization': len(self.cache) / self.max_size if self.max_size > 0 else 0
            }


class ResponseCache:
    """Cache for API responses and expensive computations."""
    
    def __init__(self):
        """Initialize response cache."""
        self.diff_cache = LRUCache(max_size=500, ttl_seconds=1800)  # 30min
        self.file_list_cache = LRUCache(max_size=100, ttl_seconds=300)  # 5min
        self.pdf_cache = LRUCache(max_size=50, ttl_seconds=3600)  # 1hour
    
    @staticmethod
    def get_content_hash(content: str) -> str:
        """Generate hash for file content.
        
        Args:
            content: File content
            
        Returns:
            SHA256 hash (truncated to 16 chars)
        """
        return hashlib.sha256(content.encode('utf-8')).hexdigest()[:16]
    
    async def get_cached_diff(self, content_hash: str, command: str) -> Optional[dict]:
        """Get cached diff result.
        
        Args:
            content_hash: Hash of file content
            command: Command that was applied
            
        Returns:
            Cached diff result or None
        """
        key = f"diff:{content_hash}:{command}"
        return await self.diff_cache.get(key)
    
    async def cache_diff(self, content_hash: str, command: str, result: dict):
        """Cache diff result.
        
        Args:
            content_hash: Hash of file content
            command: Command that was applied
            result: Diff result to cache
        """
        key = f"diff:{content_hash}:{command}"
        await self.diff_cache.set(key, result)
    
    async def get_cached_file_list(self, directory: str, depth: int) -> Optional[list]:
        """Get cached file listing.
        
        Args:
            directory: Directory path
            depth: Scan depth
            
        Returns:
            Cached file list or None
        """
        key = f"files:{directory}:{depth}"
        return await self.file_list_cache.get(key)
    
    async def cache_file_list(self, directory: str, depth: int, files: list):
        """Cache file listing.
        
        Args:
            directory: Directory path
            depth: Scan depth
            files: File list to cache
        """
        key = f"files:{directory}:{depth}"
        await self.file_list_cache.set(key, files)
    
    async def invalidate_file_list(self, directory: str = None):
        """Invalidate file list cache.
        
        Args:
            directory: Specific directory to invalidate, or None for all
        """
        async with self.file_list_cache._lock:
            if directory:
                # Remove specific directory entries
                keys_to_delete = [k for k in self.file_list_cache.cache.keys() if directory in k]
                for key in keys_to_delete:
                    del self.file_list_cache.cache[key]
            else:
                # Clear all file lists
                self.file_list_cache.cache.clear()
    
    async def get_stats(self) -> dict:
        """Get cache statistics.
        
        Returns:
            Dictionary with all cache stats
        """
        return {
            'diff_cache': await self.diff_cache.get_stats(),
            'file_list_cache': await self.file_list_cache.get_stats(),
            'pdf_cache': await self.pdf_cache.get_stats()
        }


# Global cache instance
_response_cache: Optional[ResponseCache] = None


def get_cache() -> ResponseCache:
    """Get or create global response cache."""
    global _response_cache
    if _response_cache is None:
        _response_cache = ResponseCache()
    return _response_cache
