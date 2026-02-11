"""Compilation queue system to prevent CPU overload from concurrent LaTeX compilations."""

import asyncio
import hashlib
import time
import uuid
from dataclasses import dataclass, field
from enum import Enum
from pathlib import Path
from typing import Dict, Optional


class JobStatus(str, Enum):
    """Compilation job status."""
    QUEUED = "queued"
    COMPILING = "compiling"
    COMPLETED = "completed"
    FAILED = "failed"
    CANCELLED = "cancelled"


@dataclass
class CompilationJob:
    """A single compilation job."""
    job_id: str
    file_path: Path
    options: dict = field(default_factory=dict)
    status: JobStatus = JobStatus.QUEUED
    progress: float = 0.0
    pdf_url: Optional[str] = None
    diagnostics: dict = field(default_factory=dict)
    error: Optional[str] = None
    created_at: float = field(default_factory=time.time)
    started_at: Optional[float] = None
    completed_at: Optional[float] = None
    _subscribers: list = field(default_factory=list, init=False, repr=False)
    _compiler = None  # Track compiler instance for process cancellation
    
    async def subscribe(self) -> asyncio.Queue:
        """Subscribe to job progress updates.
        
        Returns:
            Queue that will receive progress events
        """
        queue = asyncio.Queue()
        self._subscribers.append(queue)
        return queue
    
    async def unsubscribe(self, queue: asyncio.Queue):
        """Unsubscribe from job progress updates.
        
        Args:
            queue: The queue to remove from subscribers
        """
        try:
            self._subscribers.remove(queue)
        except ValueError:
            pass  # Queue already removed
    
    def has_subscribers(self) -> bool:
        """Check if job has any active subscribers.
        
        Returns:
            True if there are active subscribers
        """
        return len(self._subscribers) > 0
    
    async def _notify_subscribers(self, event: dict):
        """Notify all subscribers of progress event.
        
        Args:
            event: Progress event dictionary
        """
        for queue in self._subscribers:
            try:
                await queue.put(event)
            except Exception:
                pass  # Ignore errors sending to individual subscribers
    
    async def compile(self):
        """Execute the compilation."""
        from .pdf_compiler import PDFCompiler, CompileError
        from pathlib import Path
        import tempfile
        
        self.status = JobStatus.COMPILING
        self.started_at = time.time()
        self.progress = 0.1
        await self._notify_subscribers({
            'status': 'compiling',
            'progress': 0.1,
            'step': 'Starting compilation'
        })
        
        try:
            # Run compilation in threadpool to avoid blocking
            from fastapi.concurrency import run_in_threadpool
            
            await self._notify_subscribers({
                'status': 'compiling',
                'progress': 0.3,
                'step': 'Running pdflatex'
            })
            
            # Use compile_file to support multi-file projects and correct naming
            file_path = Path(self.file_path)
            output_dir = self.options.get('output_dir')
            
            # Create compiler instance for this job to support cancellation
            from .pdf_compiler import PDFCompiler
            import tempfile
            compiler = PDFCompiler(working_dir=tempfile.gettempdir())
            self._compiler = compiler  # Store for cancellation
            
            result = await run_in_threadpool(
                __import__('latex_ai.pdf_compiler', fromlist=['compile_file']).compile_file,
                str(file_path),
                output_dir
            )
            
            # Check if job was cancelled during compilation
            if self.status == JobStatus.CANCELLED:
                return
            
            await self._notify_subscribers({
                'status': 'compiling',
                'progress': 0.9,
                'step': 'Processing output'
            })
            
            self.progress = 1.0
            
            if result.success:
                self.status = JobStatus.COMPLETED
                self.pdf_url = str(result.pdf_path) if result.pdf_path else None
                self.diagnostics = {
                    'errors': [
                        {
                            'line': e.line,
                            'file': getattr(e, 'file', '') or '',
                            'message': getattr(e, 'message', str(e)),
                            'type': e.error_type
                        }
                        for e in result.errors
                    ],
                    'warnings': [
                        {
                            'line': w.line,
                            'file': getattr(w, 'file', '') or '',
                            'message': getattr(w, 'message', str(w)),
                            'type': w.error_type
                        }
                        for w in result.warnings
                    ],
                    'page_count': getattr(result, 'page_count', 0)
                }
                await self._notify_subscribers({
                    'status': 'completed',
                    'progress': 1.0,
                    'pdf_url': self.pdf_url,
                    'diagnostics': self.diagnostics
                })
            else:
                self.status = JobStatus.FAILED
                self.error = result.errors[0].message if result.errors else "Compilation failed"
                self.diagnostics = {
                    'errors': [
                        {
                            'line': e.line,
                            'file': getattr(e, 'file', '') or '',
                            'message': getattr(e, 'message', str(e)),
                            'type': e.error_type
                        }
                        for e in result.errors
                    ],
                    'warnings': [
                        {
                            'line': w.line,
                            'file': getattr(w, 'file', '') or '',
                            'message': getattr(w, 'message', str(w)),
                            'type': w.error_type
                        }
                        for w in result.warnings
                    ]
                }
                await self._notify_subscribers({
                    'status': 'failed',
                    'error': self.error,
                    'diagnostics': self.diagnostics
                })
        
        except Exception as e:
            # Don't update status if already cancelled
            if self.status != JobStatus.CANCELLED:
                self.status = JobStatus.FAILED
                self.error = str(e)
                self.diagnostics = {'errors': [{'line': 0, 'message': str(e), 'type': 'error'}]}
                await self._notify_subscribers({
                    'status': 'failed',
                    'error': str(e),
                    'diagnostics': self.diagnostics
                })
        
        finally:
            self.completed_at = time.time()
            self._compiler = None
    
    def set_error(self, error: Exception):
        """Mark job as failed with error."""
        self.status = JobStatus.FAILED
        self.error = str(error)
        self.completed_at = time.time()
    
    def to_dict(self) -> dict:
        """Convert to dictionary for API response."""
        return {
            'job_id': self.job_id,
            'status': self.status.value,
            'progress': self.progress,
            'pdf_url': self.pdf_url,
            'diagnostics': self.diagnostics,
            'error': self.error,
            'created_at': self.created_at,
            'started_at': self.started_at,
            'completed_at': self.completed_at,
            'duration': (self.completed_at - self.started_at) if self.completed_at and self.started_at else None
        }


class CompilationQueue:
    """Manages compilation jobs with concurrency control and debouncing."""
    
    DEBOUNCE_DELAY = 0.25  # 250ms debounce for rapid edits
    CLEANUP_INTERVAL = 300  # 5 minutes cleanup interval
    CLEANUP_MAX_AGE = 3600  # 1 hour max age for completed jobs
    
    def __init__(self, max_concurrent: int = 2):
        """Initialize compilation queue.
        
        Args:
            max_concurrent: Maximum number of concurrent compilations
        
        NOTE: This queue maintains in-memory state (active_jobs, _pending_hash, etc.)
        and is designed for single-process deployment. Under multi-worker uvicorn
        (--workers N), each worker has its own queue instance, causing:
        - Job IDs to be non-unique across workers
        - "Latest PDF" semantics to diverge
        - Deduplication to fail across worker boundaries
        
        For multi-worker/multi-instance deployments, migrate job state to Redis
        or implement a shared backend (see ARCHITECTURE.md for details).
        """
        # Defer creation of asyncio primitives to a running loop (in start())
        self.queue: asyncio.Queue | None = None
        self.semaphore: asyncio.Semaphore | None = None
        self.active_jobs: Dict[str, CompilationJob] = {}
        self._pending_hash: Dict[str, str] = {}  # file_hash -> job_id for deduplication
        self._job_to_hash: Dict[str, str] = {}  # job_id -> file_hash for cleanup
        self._latest_job_by_path: Dict[str, str] = {}  # file_path -> job_id for O(1) path-based lookups
        self._last_completed_at_by_path: Dict[str, float] = {}  # file_path -> completion time for debounce
        self.max_concurrent = max_concurrent
        self._workers = []
        self._running = False
        self._cleanup_task: Optional[asyncio.Task] = None  # Background cleanup task
        self._state_lock: asyncio.Lock | None = None  # Protects active_jobs, _pending_hash, _latest_job_by_path mutations
        self._debounce_timers: Dict[str, asyncio.Task] = {}  # file_path -> debounce task
        self._debounce_lock: asyncio.Lock | None = None  # Protects debounce timer mutations

    def _ensure_initialized(self):
        """Ensure asyncio primitives are created under the current loop."""
        if self.queue is None:
            self.queue = asyncio.Queue()
        if self.semaphore is None:
            self.semaphore = asyncio.Semaphore(self.max_concurrent)
        if self._state_lock is None:
            self._state_lock = asyncio.Lock()
        if self._debounce_lock is None:
            self._debounce_lock = asyncio.Lock()
    
    def _compute_file_hash(self, file_path: Path) -> str:
        """Compute SHA256 hash of file content for deduplication.
        
        Args:
            file_path: Path to file
            
        Returns:
            SHA256 hash as hex string
        """
        try:
            file_content = file_path.read_bytes()
            return hashlib.sha256(file_content).hexdigest()
        except Exception:
            # If file can't be read, use path-based hash
            return hashlib.sha256(str(file_path).encode()).hexdigest()
    
    async def _compute_bundle_hash(self, file_path: Path) -> str:
        """Compute SHA256 hash of entire bundle (main file + dependencies).
        
        This ensures proper deduplication for multi-file projects.
        MUST match PDFCompiler._compute_bundle_hash() for cache consistency.
        
        Args:
            file_path: Path to main LaTeX file
            
        Returns:
            SHA256 hash of bundle as hex string
        """
        from .document_bundle import DocumentBundle
        
        try:
            hasher = hashlib.sha256()
            
            # Load bundle with all dependencies
            bundle = DocumentBundle(file_path, workspace_root=file_path.parent)
            await bundle.load_async()
            
            # Hash main file content
            main_content = bundle.files.get(str(file_path), "")
            hasher.update(f"MAIN:{file_path.name}:".encode('utf-8'))
            hasher.update(main_content.encode('utf-8'))
            
            # Hash all dependencies in sorted order (stable ordering)
            # Use full relative path to avoid collisions between files with same basename
            for dep_path in sorted(bundle.files.keys()):
                if dep_path != str(file_path):
                    content = bundle.files[dep_path]
                    # Use workspace-relative path to avoid collisions (e.g., sections/intro.tex vs appendix/intro.tex)
                    try:
                        dep_rel = str(Path(dep_path).relative_to(file_path.parent).as_posix())
                    except ValueError:
                        # If not relative to main file parent, use absolute posix path
                        dep_rel = str(Path(dep_path).as_posix())
                    hasher.update(f"\nDEP:{dep_rel}:".encode('utf-8'))
                    hasher.update(content.encode('utf-8'))
            
            return hasher.hexdigest()
        except Exception:
            # Fallback to single file hash on error
            return self._compute_file_hash(file_path)
    
    async def submit_debounced(self, file_path: Path, options: dict = None) -> CompilationJob:
        """Submit compilation job with debounce delay (250ms).
        
        Batches rapid edits together to avoid excessive compilations.
        If another request comes within 250ms, cancels the previous debounce
        and resets the timer with the new file state.
        
        Args:
            file_path: Path to LaTeX file
            options: Compilation options
            
        Returns:
            CompilationJob object (may be reused if debouncing)
        """
        file_path_str = str(file_path)
        
        self._ensure_initialized()
        async with self._debounce_lock:  # type: ignore[arg-type]
            # Cancel previous debounce timer if it exists
            if file_path_str in self._debounce_timers:
                old_task = self._debounce_timers[file_path_str]
                if not old_task.done():
                    old_task.cancel()
            
            # Create new debounce task
            async def debounced_submit():
                try:
                    await asyncio.sleep(self.DEBOUNCE_DELAY)
                    return await self.submit(file_path, options)
                except asyncio.CancelledError:
                    pass
            
            task = asyncio.create_task(debounced_submit())
            self._debounce_timers[file_path_str] = task
            
            # Wait for the debounced submit to complete
            try:
                return await task
            finally:
                # Clean up the task reference
                if file_path_str in self._debounce_timers and self._debounce_timers[file_path_str] is task:
                    del self._debounce_timers[file_path_str]
    
    async def submit(self, file_path: Path, options: dict = None) -> CompilationJob:
        """Submit compilation job.
        
        Args:
            file_path: Path to LaTeX file
            options: Compilation options
            
        Returns:
            CompilationJob object
        
        Performance notes:
        - Bundle hash is computed OUTSIDE the lock to prevent I/O blocking other requests
        - Path-based deduplication is O(1) via _latest_job_by_path index
        - Lock is held only for state mutations (active_jobs, _pending_hash, etc.)
        """
        self._ensure_initialized()
        
        # PHASE 1: Compute bundle hash OUTSIDE the lock
        # This is where expensive I/O happens (reading document bundles, dependency walks)
        # We do this before acquiring the lock to avoid blocking other submits
        try:
            file_hash = await self._compute_bundle_hash(file_path)
        except Exception:
            # Fallback to file content hash if bundle computation fails
            file_hash = self._compute_file_hash(file_path)
        
        # PHASE 2: Acquire lock only for state mutations
        async with self._state_lock:  # type: ignore[arg-type]
            import logging
            
            # Check if identical compilation is already running (content-based dedup)
            if file_hash in self._pending_hash:
                existing_job_id = self._pending_hash[file_hash]
                if existing_job_id in self.active_jobs:
                    existing_job = self.active_jobs[existing_job_id]
                    if existing_job.status in {JobStatus.QUEUED, JobStatus.COMPILING}:
                        logging.info(f"Deduplicating compile request (hash match) for {file_path}")
                        return existing_job
            
            # Path-based deduplication (O(1) via index, not O(n) scan)
            # Check if there's already a queued/compiling job for this file_path
            now = time.time()
            if file_path in self._latest_job_by_path:
                latest_job_id = self._latest_job_by_path[file_path]
                if latest_job_id in self.active_jobs:
                    latest_job = self.active_jobs[latest_job_id]
                    if latest_job.status in {JobStatus.QUEUED, JobStatus.COMPILING}:
                        logging.info(f"Deduplicating compile request (path match) for {file_path}")
                        return latest_job
                    
                    # Debounce recently completed jobs (avoid thrash from rapid re-clicks)
                    if latest_job.status == JobStatus.COMPLETED:
                        completion_time = self._last_completed_at_by_path.get(str(file_path), 0)
                        if now - completion_time < 0.5:
                            logging.info(f"Debouncing compile request for {file_path} (completed {(now - completion_time):.2f}s ago)")
                            return latest_job
            
            # Create and register new job
            job_id = str(uuid.uuid4())
            job = CompilationJob(
                job_id=job_id,
                file_path=file_path,
                options=options or {}
            )
            
            self.active_jobs[job_id] = job
            self._pending_hash[file_hash] = job_id  # Track hash for deduplication
            self._job_to_hash[job_id] = file_hash  # Track for cleanup
            self._latest_job_by_path[str(file_path)] = job_id  # Track for O(1) path lookups
            await self.queue.put(job)  # type: ignore[union-attr]
            
            return job
    
    async def get_job(self, job_id: str) -> Optional[CompilationJob]:
        """Get job by ID.
        
        Args:
            job_id: Job identifier
            
        Returns:
            CompilationJob or None
        """
        return self.active_jobs.get(job_id)
    
    async def cancel_job(self, job_id: str) -> bool:
        """Cancel a queued or running job.
        
        If the job is currently compiling, terminates the underlying process.
        
        Args:
            job_id: Job identifier
            
        Returns:
            True if cancelled, False if not found or already completed
        """
        async with self._state_lock:  # Protect status update
            job = self.active_jobs.get(job_id)
            if not job:
                return False
            
            if job.status in (JobStatus.COMPLETED, JobStatus.FAILED, JobStatus.CANCELLED):
                return False
            
            # If currently compiling, try to kill the process
            if job.status == JobStatus.COMPILING and job._compiler is not None:
                try:
                    job._compiler.cancel_compilation(job_id=job_id)
                except Exception:
                    pass  # Ignore errors during cancellation
            
            job.status = JobStatus.CANCELLED
            job.completed_at = time.time()
            return True
    
    async def worker(self):
        """Process compilation jobs with concurrency limit."""
        self._ensure_initialized()
        while self._running:
            try:
                # Wait for job with timeout to allow graceful shutdown
                job = await asyncio.wait_for(self.queue.get(), timeout=1.0)  # type: ignore[union-attr]
                
                # Skip if job was cancelled while queued
                if job.status == JobStatus.CANCELLED:
                    self.queue.task_done()
                    continue
                
                # Acquire semaphore to limit concurrency
                async with self.semaphore:  # type: ignore[union-attr]
                    try:
                        await job.compile()
                    except Exception as e:
                        job.set_error(e)
                    finally:
                        self.queue.task_done()
            
            except asyncio.TimeoutError:
                # No job available, continue loop
                continue
            except Exception as e:
                # Log unexpected errors but keep worker running
                print(f"Compilation worker error: {e}")
    
    async def cleanup_worker(self):
        """Periodic cleanup of old completed jobs to prevent memory leak."""
        while self._running:
            try:
                # Wait for cleanup interval
                await asyncio.sleep(self.CLEANUP_INTERVAL)
                
                # Perform cleanup
                await self.cleanup_old_jobs(max_age_seconds=self.CLEANUP_MAX_AGE)
                
            except asyncio.CancelledError:
                # Task cancelled during shutdown
                break
            except Exception as e:
                # Log unexpected errors but keep cleanup running
                print(f"Cleanup worker error: {e}")
    
    async def start(self):
        """Start worker tasks and cleanup task."""
        if self._running:
            return
        
        # Initialize asyncio primitives under the running loop
        self._ensure_initialized()
        self._running = True
        
        # Start compilation workers
        self._workers = [
            asyncio.create_task(self.worker())
            for _ in range(self.max_concurrent)
        ]
        
        # Start cleanup worker
        self._cleanup_task = asyncio.create_task(self.cleanup_worker())
    
    async def stop(self):
        """Stop all workers gracefully."""
        self._running = False
        
        # Cancel cleanup task
        if self._cleanup_task and not self._cleanup_task.done():
            self._cleanup_task.cancel()
            try:
                await self._cleanup_task
            except asyncio.CancelledError:
                pass
        
        # Wait for workers to finish
        if self._workers:
            await asyncio.gather(*self._workers, return_exceptions=True)
        
        self._workers = []
        self._cleanup_task = None
    
    async def cleanup_old_jobs(self, max_age_seconds: int = 3600):
        """Remove completed jobs older than max_age.
        
        Also cleans up path-based indices to prevent unbounded growth.
        
        Args:
            max_age_seconds: Maximum age in seconds
        """
        async with self._state_lock:  # Protect dictionary mutations
            now = time.time()
            to_remove = []
            path_to_latest_job = {}  # Track latest job per path
            
            # First pass: identify expired jobs and track active jobs per path
            for job_id, job in self.active_jobs.items():
                if job.completed_at and (now - job.completed_at) > max_age_seconds:
                    to_remove.append(job_id)
                else:
                    # Track latest job per path (for retention in index)
                    path_key = str(job.file_path)
                    if path_key not in path_to_latest_job or job.created_at > path_to_latest_job[path_key].created_at:
                        path_to_latest_job[path_key] = job
            
            # Second pass: remove expired jobs and clean up indices
            for job_id in to_remove:
                job = self.active_jobs.get(job_id)
                
                # Clean up hash tracking
                if job_id in self._job_to_hash:
                    file_hash = self._job_to_hash[job_id]
                    if self._pending_hash.get(file_hash) == job_id:
                        del self._pending_hash[file_hash]
                    del self._job_to_hash[job_id]
                
                # Clean up path indices for this job
                if job:
                    path_key = str(job.file_path)
                    if self._latest_job_by_path.get(path_key) == job_id:
                        # Only clean if this was the tracked latest job
                        del self._latest_job_by_path[path_key]
                    # Clean up completion time tracking
                    if path_key in self._last_completed_at_by_path:
                        del self._last_completed_at_by_path[path_key]
                
                del self.active_jobs[job_id]
            
            # Update path indices with latest active jobs
            for path_key, job in path_to_latest_job.items():
                self._latest_job_by_path[path_key] = job.job_id
                if job.status == JobStatus.COMPLETED and job.completed_at:
                    self._last_completed_at_by_path[path_key] = job.completed_at
    
    def get_stats(self) -> dict:
        """Get queue statistics.
        
        Returns:
            Dictionary with queue stats
        """
        statuses = {}
        for job in self.active_jobs.values():
            statuses[job.status.value] = statuses.get(job.status.value, 0) + 1
        
        return {
            'total_jobs': len(self.active_jobs),
            'queue_size': self.queue.qsize(),
            'max_concurrent': self.max_concurrent,
            'statuses': statuses
        }


# Global compilation queue instance
_compilation_queue: Optional[CompilationQueue] = None


def get_compilation_queue() -> CompilationQueue:
    """Get or create global compilation queue."""
    global _compilation_queue
    if _compilation_queue is None:
        _compilation_queue = CompilationQueue(max_concurrent=2)
    return _compilation_queue
