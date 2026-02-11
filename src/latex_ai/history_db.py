"""SQLite-based history tracking for LaTeX-AI edits."""

import sqlite3
import threading
import time
from contextlib import contextmanager
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path
from typing import List, Optional


@dataclass
class HistoryEntry:
    """Single entry in edit history."""
    
    id: int
    file_path: str
    command: str
    timestamp: datetime
    new_text: str
    diff: str
    success: bool = True


class HistoryDB:
    """SQLite database for tracking edit history with thread-safe connection management."""
    
    def __init__(self, db_path: Path):
        """Initialize history database.
        
        Args:
            db_path: Path to SQLite database file
        """
        self.db_path = Path(db_path)
        self.db_path.parent.mkdir(parents=True, exist_ok=True)
        self._transaction_lock = threading.Lock()  # Protect undo/redo operations
        self._init_db()
    
    @contextmanager
    def get_connection(self):
        """Get a connection for the current request.
        
        This creates a new connection per request to avoid SQLite threading issues.
        """
        conn = sqlite3.connect(self.db_path)
        conn.row_factory = sqlite3.Row
        try:
            yield conn
        finally:
            conn.close()
    
    def _init_db(self) -> None:
        """Initialize database schema."""
        with self.get_connection() as conn:
            # Create history table if it doesn't exist
            # Add edit_type column for different save types (manual-save, ai-agent-apply, apply-hunks, restore)
            conn.execute("""
                CREATE TABLE IF NOT EXISTS history (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    file_path TEXT NOT NULL,
                    command TEXT NOT NULL,
                    timestamp DATETIME DEFAULT CURRENT_TIMESTAMP,
                    new_text TEXT NOT NULL,
                    diff TEXT NOT NULL,
                    success BOOLEAN DEFAULT 1,
                    edit_type TEXT DEFAULT 'manual-save'
                )
            """)
            
            # Migrate existing table if needed (add edit_type column if missing)
            try:
                conn.execute("ALTER TABLE history ADD COLUMN edit_type TEXT DEFAULT 'manual-save'")
            except sqlite3.OperationalError:
                pass  # Column already exists
            
            # Create AI plan cache table for persistent storage
            conn.execute("""
                CREATE TABLE IF NOT EXISTS ai_cache (
                    cache_key TEXT PRIMARY KEY,
                    plan_json TEXT NOT NULL,
                    timestamp DATETIME DEFAULT CURRENT_TIMESTAMP,
                    ttl_seconds INTEGER DEFAULT 3600
                )
            """)
            
            # Create index for cache expiration queries
            conn.execute("""
                CREATE INDEX IF NOT EXISTS idx_ai_cache_timestamp 
                ON ai_cache(timestamp)
            """)
            
            # Create job queue table for persistent compilation jobs
            conn.execute("""
                CREATE TABLE IF NOT EXISTS job_queue (
                    job_id TEXT PRIMARY KEY,
                    file_path TEXT NOT NULL,
                    options_json TEXT NOT NULL,
                    status TEXT NOT NULL,
                    progress REAL DEFAULT 0.0,
                    pdf_url TEXT,
                    diagnostics_json TEXT,
                    error TEXT,
                    created_at REAL NOT NULL,
                    started_at REAL,
                    completed_at REAL
                )
            """)
            
            conn.execute("""
                CREATE INDEX IF NOT EXISTS idx_job_status 
                ON job_queue(status)
            """)
            
            conn.commit()
    
    def add_entry(self, file_path: str, command: str, new_text: str, diff: str, edit_type: str = "manual-save") -> int:
        """Record edit in history and enforce per-file limit.
        
        Args:
            file_path: Path to edited file
            command: Command executed
            new_text: Resulting file content
            diff: Unified diff showing changes
            edit_type: Type of edit (manual-save, ai-agent-apply, apply-hunks, restore)
            
        Returns:
            ID of created history entry
        """
        with self.get_connection() as conn:
            cursor = conn.execute(
                """INSERT INTO history (file_path, command, new_text, diff, edit_type) 
                   VALUES (?, ?, ?, ?, ?)""",
                (str(file_path), command, new_text, diff, edit_type)
            )
            entry_id = cursor.lastrowid

            # Enforce history_limit per file (newest retained)
            try:
                from .config import get_config
                cfg = get_config()
                limit = getattr(cfg, 'history_limit', 100) or 100
            except Exception:
                limit = 100

            if limit > 0:
                conn.execute(
                    """
                    DELETE FROM history
                    WHERE id IN (
                        SELECT id FROM history
                        WHERE file_path = ?
                        ORDER BY id DESC
                        LIMIT -1 OFFSET ?
                    )
                    """,
                    (str(file_path), limit)
                )

            conn.commit()
            return entry_id
    
    def get_history(self, file_path: str, limit: int = 50, offset: int = 0) -> List[HistoryEntry]:
        """Fetch edit history for a file with pagination.
        
        Args:
            file_path: Path to file
            limit: Maximum number of entries to return (default: 50)
            offset: Number of entries to skip (for pagination, default: 0)
            
        Returns:
            List of HistoryEntry objects, newest first
        """
        with self.get_connection() as conn:
            cursor = conn.execute(
                """SELECT id, file_path, command, timestamp, new_text, diff, success 
                   FROM history 
                   WHERE file_path = ? 
                   ORDER BY id DESC 
                   LIMIT ? OFFSET ?""",
                (str(file_path), limit, offset)
            )
            
            entries = []
            for row in cursor.fetchall():
                entries.append(HistoryEntry(
                    id=row["id"],
                    file_path=row["file_path"],
                    command=row["command"],
                    timestamp=datetime.fromisoformat(row["timestamp"]),
                    new_text=row["new_text"],
                    diff=row["diff"],
                    success=bool(row["success"])
                ))
            
            return entries
    
    def count_history(self, file_path: str) -> int:
        """Count total history entries for a file.
        
        Args:
            file_path: Path to file
            
        Returns:
            Total number of history entries
        """
        with self.get_connection() as conn:
            cursor = conn.execute(
                "SELECT COUNT(*) as count FROM history WHERE file_path = ?",
                (str(file_path),)
            )
            row = cursor.fetchone()
            return row['count'] if row else 0
    
    def get_entry(self, entry_id: int) -> Optional[HistoryEntry]:
        """Fetch single history entry by ID.
        
        Args:
            entry_id: History entry ID
            
        Returns:
            HistoryEntry or None if not found
        """
        with self.get_connection() as conn:
            cursor = conn.execute(
                """SELECT id, file_path, command, timestamp, new_text, diff, success 
                   FROM history 
                   WHERE id = ?""",
                (entry_id,)
            )
            
            row = cursor.fetchone()
            if not row:
                return None
            
            return HistoryEntry(
                id=row["id"],
                file_path=row["file_path"],
                command=row["command"],
                timestamp=datetime.fromisoformat(row["timestamp"]),
                new_text=row["new_text"],
                diff=row["diff"],
                success=bool(row["success"])
            )
    
    def delete_old_entries(self, days: int = 7) -> int:
        """Delete history entries older than specified days.
        
        Args:
            days: Age threshold in days
            
        Returns:
            Number of entries deleted
        """
        with self.get_connection() as conn:
            cursor = conn.execute(
                """DELETE FROM history 
                   WHERE datetime(timestamp) < datetime('now', '-' || ? || ' days')""",
                (days,)
            )
            conn.commit()
            return cursor.rowcount
    
    def __enter__(self):
        """Context manager entry."""
        return self
    
    def __exit__(self, exc_type, exc_val, exc_tb):
        """Context manager exit - no-op as connections are per-request."""
        pass
    
    def close(self) -> None:
        """Close database connections.
        
        Note: With connection-per-request pattern, this is a no-op.
        Connections are closed automatically in the context manager.
        """
        pass
    
    def atomic_undo(self, file_path: str) -> Optional[HistoryEntry]:
        """Atomically find and return previous entry for undo operation.
        
        This method protects against race conditions by using a transaction lock.
        
        Args:
            file_path: Path to file
            
        Returns:
            Previous HistoryEntry or None if no undo available
        """
        with self._transaction_lock:
            entries = self.get_history(file_path, 100)
            
            # Find last agent edit
            agent_commands = ["ai-agent-apply", "apply-hunks", "bulk-apply"]
            agent_edits = [e for e in entries if any(cmd in e.command for cmd in agent_commands)]
            
            if not agent_edits:
                return None
            
            # Get index of most recent agent edit
            last_agent = agent_edits[0]
            idx = entries.index(last_agent)
            
            # Return entry BEFORE the agent edit (if exists)
            if idx + 1 < len(entries):
                return entries[idx + 1]
            
            return None
    
    def atomic_redo(self, file_path: str) -> Optional[HistoryEntry]:
        """Atomically find and return next entry for redo operation.
        
        This method protects against race conditions by using a transaction lock.
        
        Args:
            file_path: Path to file
            
        Returns:
            Next HistoryEntry or None if no redo available
        """
        with self._transaction_lock:
            entries = self.get_history(file_path, 100)
            
            # Find entries newer than current that were undone
            # This is a simplified implementation - could track undo/redo state in DB
            if len(entries) < 2:
                return None
            
            # Return the second-most-recent entry
            return entries[1] if len(entries) > 1 else None
    
    # ========================================================================
    # AI Cache Operations (Persistent)
    # ========================================================================
    
    def get_ai_cache(self, cache_key: str, ttl: int = 3600) -> Optional[str]:
        """Retrieve cached AI plan if still valid.
        
        Args:
            cache_key: Unique cache key (hash of content + model + request)
            ttl: Time-to-live in seconds (default: 1 hour)
            
        Returns:
            Cached plan JSON or None if expired/missing
        """
        with self.get_connection() as conn:
            cursor = conn.execute(
                """
                SELECT plan_json, timestamp, ttl_seconds
                FROM ai_cache
                WHERE cache_key = ?
                """,
                (cache_key,)
            )
            row = cursor.fetchone()
            
            if not row:
                return None
            
            # Check if cache is still valid
            cache_time = datetime.fromisoformat(row["timestamp"])
            cache_ttl = row["ttl_seconds"]
            age = (datetime.now() - cache_time).total_seconds()
            
            if age > cache_ttl:
                # Expired - delete it
                conn.execute("DELETE FROM ai_cache WHERE cache_key = ?", (cache_key,))
                conn.commit()
                return None
            
            return row["plan_json"]
    
    def set_ai_cache(self, cache_key: str, plan_json: str, ttl: int = 3600) -> None:
        """Store AI plan in persistent cache.
        
        Args:
            cache_key: Unique cache key
            plan_json: Serialized plan JSON
            ttl: Time-to-live in seconds (default: 1 hour)
        """
        with self.get_connection() as conn:
            conn.execute(
                """
                INSERT OR REPLACE INTO ai_cache (cache_key, plan_json, timestamp, ttl_seconds)
                VALUES (?, ?, CURRENT_TIMESTAMP, ?)
                """,
                (cache_key, plan_json, ttl)
            )
            conn.commit()
    
    def cleanup_ai_cache(self, max_age_hours: int = 24) -> int:
        """Remove expired AI cache entries.
        
        Args:
            max_age_hours: Delete entries older than this (default: 24 hours)
            
        Returns:
            Number of entries deleted
        """
        with self.get_connection() as conn:
            cursor = conn.execute(
                """
                DELETE FROM ai_cache
                WHERE datetime(timestamp) < datetime('now', '-' || ? || ' hours')
                """,
                (max_age_hours,)
            )
            conn.commit()
            return cursor.rowcount
    
    # ========================================================================
    # Job Queue Operations (Persistent)
    # ========================================================================
    
    def save_job(self, job_id: str, file_path: str, options: dict, status: str,
                 progress: float = 0.0, pdf_url: str = None, diagnostics: dict = None,
                 error: str = None, created_at: float = None, started_at: float = None,
                 completed_at: float = None) -> None:
        """Persist compilation job state.
        
        Args:
            job_id: Unique job identifier
            file_path: Path to LaTeX file
            options: Compilation options dict
            status: Job status (queued, compiling, completed, failed, cancelled)
            progress: Progress percentage (0.0 to 1.0)
            pdf_url: Path to generated PDF (if completed)
            diagnostics: Diagnostics dict (errors, warnings, etc.)
            error: Error message (if failed)
            created_at: Creation timestamp
            started_at: Start timestamp
            completed_at: Completion timestamp
        """
        import json
        
        with self.get_connection() as conn:
            conn.execute(
                """
                INSERT OR REPLACE INTO job_queue 
                (job_id, file_path, options_json, status, progress, pdf_url, 
                 diagnostics_json, error, created_at, started_at, completed_at)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                """,
                (job_id, file_path, json.dumps(options or {}), status, progress,
                 pdf_url, json.dumps(diagnostics or {}), error,
                 created_at or time.time(), started_at, completed_at)
            )
            conn.commit()
    
    def load_pending_jobs(self) -> list:
        """Load all pending/queued jobs from database for recovery.
        
        Returns:
            List of job dictionaries with status=queued or compiling
        """
        import json
        
        with self.get_connection() as conn:
            cursor = conn.execute(
                """
                SELECT job_id, file_path, options_json, status, progress, created_at
                FROM job_queue
                WHERE status IN ('queued', 'compiling')
                ORDER BY created_at ASC
                """
            )
            
            jobs = []
            for row in cursor.fetchall():
                jobs.append({
                    'job_id': row['job_id'],
                    'file_path': row['file_path'],
                    'options': json.loads(row['options_json']),
                    'status': row['status'],
                    'progress': row['progress'],
                    'created_at': row['created_at']
                })
            
            return jobs
    
    def delete_old_jobs(self, max_age_hours: int = 24) -> int:
        """Remove old completed/failed job records.
        
        Args:
            max_age_hours: Delete completed jobs older than this
            
        Returns:
            Number of jobs deleted
        """
        with self.get_connection() as conn:
            cursor = conn.execute(
                """
                DELETE FROM job_queue
                WHERE status IN ('completed', 'failed', 'cancelled')
                AND completed_at IS NOT NULL
                AND (? - completed_at) > ?
                """,
                (time.time(), max_age_hours * 3600)
            )
            conn.commit()
            return cursor.rowcount
