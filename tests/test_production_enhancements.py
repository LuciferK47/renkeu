"""
Integration tests for production enhancements.

Tests resource limits, AI retry logic, compilation caching,
concurrency control, and ETag validation.
"""
import pytest
import asyncio
import hashlib
from pathlib import Path
from unittest.mock import Mock, patch, AsyncMock

from latex_ai.config import Config, get_config
from latex_ai.services.ai import generate_patch
from latex_ai.services.compile import compile_now, compile_async, _compute_content_hash, _compilation_cache
from latex_ai.ai_planner import AIPlanner


class TestResourceLimits:
    """Test resource limit configuration."""
    
    def test_config_has_resource_limits(self):
        """Verify Config class has all resource limit fields."""
        cfg = Config()
        assert hasattr(cfg, 'max_concurrent_compilations')
        assert hasattr(cfg, 'compilation_timeout')
        assert hasattr(cfg, 'max_ai_context_size')
        assert hasattr(cfg, 'max_websocket_connections')
        assert cfg.max_concurrent_compilations == 3
        assert cfg.compilation_timeout == 120
    
    def test_ai_retry_config(self):
        """Verify AI retry configuration exists."""
        cfg = Config()
        assert hasattr(cfg, 'ai_max_retries')
        assert hasattr(cfg, 'ai_retry_delay')
        assert hasattr(cfg, 'ai_timeout')
        assert cfg.ai_max_retries == 3
        assert cfg.ai_retry_delay == 1.0
        assert cfg.ai_timeout == 60


class TestAIRetryLogic:
    """Test AI retry logic with exponential backoff."""
    
    @pytest.mark.asyncio
    async def test_retry_on_transient_error(self):
        """Test that AI requests retry on transient failures."""
        mock_planner = Mock(spec=AIPlanner)
        call_count = 0
        
        def mock_stream_generate(prompt):
            nonlocal call_count
            call_count += 1
            if call_count < 2:
                raise Exception("Transient error")
            return "SEARCH/REPLACE blocks"
        
        mock_planner.stream_generate = mock_stream_generate
        
        with patch('latex_ai.services.ai.get_agent_context', new_callable=AsyncMock) as mock_context:
            mock_context.return_value = {"combined_text": "test content"}
            
            result = await generate_patch(
                mock_planner,
                Path("test.tex"),
                "fix error",
                max_retries=3,
                retry_delay=0.1  # Fast retry for testing
            )
            
            assert result == "SEARCH/REPLACE blocks"
            assert call_count == 2  # First failed, second succeeded
    
    @pytest.mark.asyncio
    async def test_fail_after_max_retries(self):
        """Test that AI requests fail after exhausting retries."""
        mock_planner = Mock(spec=AIPlanner)
        mock_planner.stream_generate = Mock(side_effect=Exception("Persistent error"))
        
        with patch('latex_ai.services.ai.get_agent_context', new_callable=AsyncMock) as mock_context:
            mock_context.return_value = {"combined_text": "test content"}
            
            with pytest.raises(Exception) as exc_info:
                await generate_patch(
                    mock_planner,
                    Path("test.tex"),
                    "fix error",
                    max_retries=2,
                    retry_delay=0.05
                )
            
            assert "failed after 2 attempts" in str(exc_info.value)
    
    @pytest.mark.asyncio
    async def test_exponential_backoff_timing(self):
        """Test that retry delays follow exponential backoff."""
        import time
        
        mock_planner = Mock(spec=AIPlanner)
        attempts = []
        
        def mock_stream_generate(prompt):
            attempts.append(time.time())
            if len(attempts) < 3:
                raise Exception("Retry me")
            return "success"
        
        mock_planner.stream_generate = mock_stream_generate
        
        with patch('latex_ai.services.ai.get_agent_context', new_callable=AsyncMock) as mock_context:
            mock_context.return_value = {"combined_text": "test"}
            
            start = time.time()
            await generate_patch(
                mock_planner,
                Path("test.tex"),
                "fix",
                max_retries=3,
                retry_delay=0.1
            )
            duration = time.time() - start
            
            # Should have delays: 0.1s, 0.2s (exponential)
            # Total ~0.3s + execution time
            assert duration >= 0.3
            assert duration < 1.0  # Sanity check


class TestCompilationCaching:
    """Test compilation result caching."""
    
    def setup_method(self):
        """Clear cache before each test."""
        _compilation_cache.clear()
    
    def test_content_hash_computation(self):
        """Test SHA-256 hash computation for content."""
        content1 = "Hello, world!"
        content2 = "Hello, world!"
        content3 = "Different content"
        
        hash1 = _compute_content_hash(content1)
        hash2 = _compute_content_hash(content2)
        hash3 = _compute_content_hash(content3)
        
        assert hash1 == hash2  # Same content = same hash
        assert hash1 != hash3  # Different content = different hash
        assert len(hash1) == 64  # SHA-256 = 64 hex chars
    
    def test_cache_hit_returns_cached_result(self, tmp_path):
        """Test that cache returns stored result on hit."""
        tex_file = tmp_path / "test.tex"
        tex_file.write_text("\\documentclass{article}\\begin{document}Hello\\end{document}")
        
        # First compile (cache miss)
        with patch('latex_ai.services.compile.compile_file') as mock_compile:
            mock_result = Mock(success=True, page_count=1, errors=[], warnings=[])
            mock_compile.return_value = mock_result
            
            result1 = compile_now(str(tex_file), use_cache=True)
            assert mock_compile.call_count == 1
        
        # Second compile with same content (cache hit)
        with patch('latex_ai.services.compile.compile_file') as mock_compile:
            result2 = compile_now(str(tex_file), use_cache=True)
            assert mock_compile.call_count == 0  # Not called (cache hit)
            assert result2.success == True
    
    def test_cache_miss_on_content_change(self, tmp_path):
        """Test that cache misses when content changes."""
        tex_file = tmp_path / "test.tex"
        tex_file.write_text("Original content")
        
        # First compile
        with patch('latex_ai.services.compile.compile_file') as mock_compile:
            mock_result = Mock(success=True, page_count=1, errors=[], warnings=[])
            mock_compile.return_value = mock_result
            compile_now(str(tex_file), use_cache=True)
        
        # Change content
        tex_file.write_text("Modified content")
        
        # Second compile should miss cache
        with patch('latex_ai.services.compile.compile_file') as mock_compile:
            mock_result = Mock(success=True, page_count=1, errors=[], warnings=[])
            mock_compile.return_value = mock_result
            
            compile_now(str(tex_file), use_cache=True)
            assert mock_compile.call_count == 1  # Called again (cache miss)
    
    def test_cache_disabled_when_use_cache_false(self, tmp_path):
        """Test that caching can be disabled."""
        tex_file = tmp_path / "test.tex"
        tex_file.write_text("Test content")
        
        # First compile with cache disabled
        with patch('latex_ai.services.compile.compile_file') as mock_compile:
            mock_result = Mock(success=True, page_count=1, errors=[], warnings=[])
            mock_compile.return_value = mock_result
            compile_now(str(tex_file), use_cache=False)
        
        # Second compile with cache disabled
        with patch('latex_ai.services.compile.compile_file') as mock_compile:
            mock_result = Mock(success=True, page_count=1, errors=[], warnings=[])
            mock_compile.return_value = mock_result
            compile_now(str(tex_file), use_cache=False)
            assert mock_compile.call_count == 1  # Called both times
    
    @pytest.mark.asyncio
    async def test_async_compile_uses_cache(self, tmp_path):
        """Test that async compilation also uses cache."""
        tex_file = tmp_path / "test.tex"
        tex_file.write_text("Async test content")
        
        # First async compile
        with patch('latex_ai.services.compile.compile_file') as mock_compile:
            mock_result = Mock(success=True, page_count=1, errors=[], warnings=[])
            mock_compile.return_value = mock_result
            await compile_async(str(tex_file), use_cache=True)
        
        # Second async compile (should hit cache)
        with patch('latex_ai.services.compile.compile_file') as mock_compile:
            result = await compile_async(str(tex_file), use_cache=True)
            assert mock_compile.call_count == 0  # Cache hit


class TestConcurrencyControl:
    """Test compilation semaphore and concurrency limiting."""
    
    @pytest.mark.asyncio
    async def test_semaphore_limits_concurrent_tasks(self):
        """Test that semaphore properly limits concurrent execution."""
        semaphore = asyncio.Semaphore(2)  # Max 2 concurrent
        active_count = 0
        max_active = 0
        
        async def task():
            nonlocal active_count, max_active
            async with semaphore:
                active_count += 1
                max_active = max(max_active, active_count)
                await asyncio.sleep(0.1)  # Simulate work
                active_count -= 1
        
        # Start 5 tasks concurrently
        await asyncio.gather(*[task() for _ in range(5)])
        
        # Max active should never exceed semaphore limit
        assert max_active == 2
    
    @pytest.mark.asyncio
    async def test_semaphore_queues_fairly(self):
        """Test that semaphore provides fair FIFO queuing."""
        semaphore = asyncio.Semaphore(1)
        order = []
        
        async def task(task_id):
            async with semaphore:
                order.append(task_id)
                await asyncio.sleep(0.01)
        
        # Start tasks in order
        await asyncio.gather(*[task(i) for i in range(5)])
        
        # Should execute in order (FIFO)
        assert order == [0, 1, 2, 3, 4]


class TestETagValidation:
    """Test ETag-based conflict detection."""
    
    def test_etag_computation_is_deterministic(self):
        """Test that same content produces same ETag."""
        from latex_ai.services.files import compute_etag
        
        content1 = "Test content"
        content2 = "Test content"
        content3 = "Different"
        
        etag1 = compute_etag(content1)
        etag2 = compute_etag(content2)
        etag3 = compute_etag(content3)
        
        assert etag1 == etag2
        assert etag1 != etag3
    
    @pytest.mark.asyncio
    async def test_etag_mismatch_raises_409(self):
        """Test that ETag mismatch raises 409 Conflict."""
        from latex_ai.services.files import save_with_etag_history_cache
        from fastapi import HTTPException
        
        # Simulate ETag conflict scenario
        # This would be tested via actual HTTP requests in integration tests
        pass  # Placeholder for integration test


if __name__ == "__main__":
    pytest.main([__file__, "-v"])
