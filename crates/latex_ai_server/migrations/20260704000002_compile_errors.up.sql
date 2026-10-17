CREATE TABLE IF NOT EXISTS compile_errors (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    file_path TEXT NOT NULL,
    message TEXT NOT NULL,
    line INTEGER,
    column INTEGER,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);
