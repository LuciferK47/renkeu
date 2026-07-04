CREATE TABLE history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    file_path TEXT NOT NULL,
    command TEXT NOT NULL,
    timestamp DATETIME DEFAULT CURRENT_TIMESTAMP,
    new_text TEXT NOT NULL,
    diff TEXT NOT NULL,
    success BOOLEAN DEFAULT 1,
    edit_type TEXT DEFAULT 'manual-save'
);

CREATE TABLE ai_cache (
    cache_key TEXT PRIMARY KEY,             -- "file_hash\x1fmodel\x1frequest_hash"
    plan_json TEXT NOT NULL,
    timestamp DATETIME DEFAULT CURRENT_TIMESTAMP,
    ttl_seconds INTEGER DEFAULT 3600
);
CREATE INDEX idx_ai_cache_timestamp ON ai_cache(timestamp);
