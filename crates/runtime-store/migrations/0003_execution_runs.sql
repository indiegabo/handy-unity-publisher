-- Persist execution metadata while keeping large logs and artifacts on disk.

CREATE TABLE release_runs (
    id INTEGER PRIMARY KEY,
    repository_id INTEGER NOT NULL,
    git_tag TEXT NOT NULL,
    git_commit TEXT,
    trigger_source TEXT NOT NULL DEFAULT 'poll',
    trigger_rule_id INTEGER,
    source_metadata_json TEXT NOT NULL DEFAULT '{}',
    engine_version TEXT,
    status TEXT NOT NULL CHECK (status IN ('detected', 'queued', 'running', 'succeeded', 'failed', 'canceled')),
    started_at TEXT,
    finished_at TEXT,
    error_message TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (repository_id) REFERENCES repositories (id) ON DELETE CASCADE,
    FOREIGN KEY (trigger_rule_id) REFERENCES trigger_rules (id) ON DELETE SET NULL,
    UNIQUE (repository_id, git_tag)
);

CREATE TABLE build_runs (
    id INTEGER PRIMARY KEY,
    release_run_id INTEGER NOT NULL,
    build_target_id INTEGER NOT NULL,
    engine_version TEXT,
    image_ref TEXT,
    status TEXT NOT NULL CHECK (status IN ('queued', 'running', 'succeeded', 'failed', 'canceled')),
    workspace_path TEXT,
    log_path TEXT,
    artifact_root_path TEXT,
    started_at TEXT,
    finished_at TEXT,
    error_message TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (release_run_id) REFERENCES release_runs (id) ON DELETE CASCADE,
    FOREIGN KEY (build_target_id) REFERENCES build_targets (id) ON DELETE CASCADE,
    UNIQUE (release_run_id, build_target_id)
);

CREATE TABLE artifacts (
    id INTEGER PRIMARY KEY,
    build_run_id INTEGER NOT NULL,
    name TEXT NOT NULL,
    kind TEXT NOT NULL,
    path TEXT NOT NULL,
    size_bytes INTEGER,
    checksum_sha256 TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (build_run_id) REFERENCES build_runs (id) ON DELETE CASCADE
);

CREATE TABLE publish_runs (
    id INTEGER PRIMARY KEY,
    release_run_id INTEGER NOT NULL,
    build_run_id INTEGER NOT NULL,
    publish_target_id INTEGER NOT NULL,
    artifact_id INTEGER,
    status TEXT NOT NULL CHECK (status IN ('queued', 'running', 'succeeded', 'failed', 'canceled')),
    destination_ref TEXT,
    started_at TEXT,
    finished_at TEXT,
    error_message TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (release_run_id) REFERENCES release_runs (id) ON DELETE CASCADE,
    FOREIGN KEY (build_run_id) REFERENCES build_runs (id) ON DELETE CASCADE,
    FOREIGN KEY (publish_target_id) REFERENCES publish_targets (id) ON DELETE CASCADE,
    FOREIGN KEY (artifact_id) REFERENCES artifacts (id) ON DELETE SET NULL
);

CREATE INDEX idx_release_runs_repository_status ON release_runs (repository_id, status);
CREATE INDEX idx_release_runs_trigger_source_status ON release_runs (trigger_source, status);
CREATE INDEX idx_build_runs_release_status ON build_runs (release_run_id, status);
CREATE INDEX idx_artifacts_build_run_id ON artifacts (build_run_id);
CREATE INDEX idx_publish_runs_release_status ON publish_runs (release_run_id, status);