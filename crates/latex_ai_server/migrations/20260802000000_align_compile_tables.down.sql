-- Reverses 20260802000000_align_compile_tables: restores the pre-M5 shapes of
-- compile_queue (id/session_id) and compile_errors (session_id, no FK).
-- Rows cannot be converted between the two shapes, so both tables come back
-- empty; the up migration discarded them as never-populated in the same way.
DROP INDEX IF EXISTS idx_errors_history;
DROP INDEX IF EXISTS idx_queue_status;
DROP TABLE IF EXISTS compile_errors;
DROP TABLE IF EXISTS compile_queue;

CREATE TABLE compile_queue (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    file_path TEXT NOT NULL,
    status TEXT NOT NULL,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    started_at DATETIME,
    completed_at DATETIME
);

CREATE TABLE compile_errors (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    file_path TEXT NOT NULL,
    message TEXT NOT NULL,
    line INTEGER,
    column INTEGER,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);
