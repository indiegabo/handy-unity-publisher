PRAGMA foreign_keys = OFF;
CREATE TABLE release_runs_v3 (
    id INTEGER PRIMARY KEY,
    repository_id INTEGER NOT NULL,
    git_tag TEXT NOT NULL,
    git_commit TEXT,
    trigger_source TEXT NOT NULL DEFAULT 'poll',
    trigger_rule_id INTEGER,
    source_metadata_json TEXT NOT NULL DEFAULT '{}',
    source_identity TEXT NOT NULL DEFAULT 'managed_tag',
    engine_version TEXT,
    status TEXT NOT NULL CHECK (
        status IN (
            'detected',
            'queued',
            'running',
            'succeeded',
            'failed',
            'canceled'
        )
    ),
    started_at TEXT,
    finished_at TEXT,
    error_message TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (repository_id) REFERENCES repositories (id) ON DELETE CASCADE,
    FOREIGN KEY (trigger_rule_id) REFERENCES trigger_rules (id) ON DELETE
    SET NULL
);
INSERT INTO release_runs_v3 (
        id,
        repository_id,
        git_tag,
        git_commit,
        trigger_source,
        trigger_rule_id,
        source_metadata_json,
        source_identity,
        engine_version,
        status,
        started_at,
        finished_at,
        error_message,
        created_at,
        updated_at
    )
SELECT id,
    repository_id,
    git_tag,
    git_commit,
    trigger_source,
    trigger_rule_id,
    source_metadata_json,
    source_identity,
    engine_version,
    status,
    started_at,
    finished_at,
    error_message,
    created_at,
    updated_at
FROM release_runs;
DROP TABLE release_runs;
ALTER TABLE release_runs_v3
    RENAME TO release_runs;
CREATE INDEX idx_release_runs_repository_status ON release_runs (repository_id, status);
CREATE INDEX idx_release_runs_trigger_source_status ON release_runs (trigger_source, status);
PRAGMA foreign_keys = ON;