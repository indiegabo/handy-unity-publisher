CREATE TABLE execution_cleanup_records (
    id INTEGER PRIMARY KEY,
    build_run_id INTEGER,
    publish_run_id INTEGER,
    cleanup_policy TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('completed', 'failed')),
    workspace_path TEXT,
    workspace_removed INTEGER NOT NULL DEFAULT 0
        CHECK (workspace_removed IN (0, 1)),
    workspace_bytes_before INTEGER
        CHECK (workspace_bytes_before IS NULL OR workspace_bytes_before >= 0),
    workspace_bytes_after INTEGER
        CHECK (workspace_bytes_after IS NULL OR workspace_bytes_after >= 0),
    retained_file_count INTEGER NOT NULL DEFAULT 0
        CHECK (retained_file_count >= 0),
    retained_bytes INTEGER NOT NULL DEFAULT 0
        CHECK (retained_bytes >= 0),
    error_message TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    finished_at TEXT,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (build_run_id) REFERENCES build_runs (id) ON DELETE CASCADE,
    FOREIGN KEY (publish_run_id) REFERENCES publish_runs (id) ON DELETE CASCADE,
    CHECK (
        (build_run_id IS NOT NULL AND publish_run_id IS NULL)
        OR
        (build_run_id IS NULL AND publish_run_id IS NOT NULL)
    ),
    UNIQUE (build_run_id),
    UNIQUE (publish_run_id)
);

CREATE TABLE retained_execution_files (
    id INTEGER PRIMARY KEY,
    build_run_id INTEGER,
    publish_run_id INTEGER,
    role TEXT NOT NULL,
    path TEXT NOT NULL,
    source_path TEXT,
    content_type TEXT,
    content_encoding TEXT,
    status TEXT NOT NULL CHECK (status IN ('retained', 'purged')),
    original_size_bytes INTEGER
        CHECK (original_size_bytes IS NULL OR original_size_bytes >= 0),
    stored_size_bytes INTEGER
        CHECK (stored_size_bytes IS NULL OR stored_size_bytes >= 0),
    checksum_sha256 TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    purged_at TEXT,
    error_message TEXT,
    FOREIGN KEY (build_run_id) REFERENCES build_runs (id) ON DELETE CASCADE,
    FOREIGN KEY (publish_run_id) REFERENCES publish_runs (id) ON DELETE CASCADE,
    CHECK (
        (build_run_id IS NOT NULL AND publish_run_id IS NULL)
        OR
        (build_run_id IS NULL AND publish_run_id IS NOT NULL)
    )
);

CREATE INDEX idx_execution_cleanup_records_build_run
    ON execution_cleanup_records (build_run_id);

CREATE INDEX idx_execution_cleanup_records_publish_run
    ON execution_cleanup_records (publish_run_id);

CREATE INDEX idx_retained_execution_files_build_run_status
    ON retained_execution_files (build_run_id, status, id);

CREATE INDEX idx_retained_execution_files_publish_run_status
    ON retained_execution_files (publish_run_id, status, id);