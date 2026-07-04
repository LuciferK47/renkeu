CREATE TABLE compile_queue (
    job_id TEXT PRIMARY KEY,
    file_path TEXT NOT NULL,
    bundle_hash TEXT NOT NULL,
    options_json TEXT NOT NULL,
    status TEXT NOT NULL,
    progress REAL DEFAULT 0.0,
    pdf_blob_path TEXT,
    diagnostics_json TEXT,
    error TEXT,
    created_at REAL NOT NULL,
    started_at REAL,
    completed_at REAL
);
