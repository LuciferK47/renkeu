-- Realign the compile tables to masterplan §7.1 (M5).
--
-- `compile_errors` was created with a `session_id` and no foreign key, and
-- nothing ever read or wrote it. The plan's audit gate requires each error to
-- reference the history row it belongs to, so the table is recreated with the
-- FK. Dropping is safe precisely because it was never populated.
DROP TABLE IF EXISTS compile_errors;

CREATE TABLE compile_errors (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    history_id INTEGER NOT NULL,
    iteration INTEGER NOT NULL,
    line INTEGER,
    file TEXT,
    message TEXT NOT NULL,
    error_type TEXT NOT NULL,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (history_id) REFERENCES history(id) ON DELETE CASCADE
);

CREATE INDEX idx_errors_history ON compile_errors(history_id);

-- `compile_queue` gains the columns needed to recover after a crash: a job
-- that was in flight when the process died is marked abandoned on startup
-- rather than sitting "running" forever.
DROP TABLE IF EXISTS compile_queue;

CREATE TABLE compile_queue (
    job_id TEXT PRIMARY KEY,
    file_path TEXT NOT NULL,
    status TEXT NOT NULL,
    error TEXT,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    completed_at DATETIME
);

CREATE INDEX idx_queue_status ON compile_queue(status);
