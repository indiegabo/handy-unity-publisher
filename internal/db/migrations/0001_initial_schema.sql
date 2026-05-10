-- This schema keeps large logs and artifacts on disk and stores only metadata.

CREATE TABLE credentials (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    kind TEXT NOT NULL,
    config_json TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (name)
);

CREATE TABLE repositories (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    repo_url TEXT NOT NULL,
    credentials_id INTEGER,
    default_branch TEXT,
    polling_interval_seconds INTEGER NOT NULL DEFAULT 300 CHECK (polling_interval_seconds > 0),
    last_seen_tag TEXT,
    enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (credentials_id) REFERENCES credentials (id),
    UNIQUE (name),
    UNIQUE (repo_url)
);

-- Trigger rules describe which sources may create release work for a
-- repository. Their operational configuration stays flexible in JSON until the
-- dedicated CRUD surface is implemented.
CREATE TABLE trigger_rules (
    id INTEGER PRIMARY KEY,
    repository_id INTEGER NOT NULL,
    name TEXT NOT NULL,
    source TEXT NOT NULL CHECK (source IN ('manual', 'poll', 'webhook')),
    enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    config_json TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (repository_id) REFERENCES repositories (id) ON DELETE CASCADE,
    UNIQUE (repository_id, name)
);

CREATE TABLE build_targets (
    id INTEGER PRIMARY KEY,
    repository_id INTEGER NOT NULL,
    name TEXT NOT NULL,
    platform TEXT NOT NULL,
    runner_type TEXT NOT NULL DEFAULT 'gameci',
    build_method TEXT,
    output_kind TEXT,
    output_path_template TEXT,
    unity_version_override TEXT,
    image_override TEXT,
    timeout_seconds INTEGER NOT NULL DEFAULT 3600 CHECK (timeout_seconds > 0),
    enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    config_json TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (repository_id) REFERENCES repositories (id) ON DELETE CASCADE,
    UNIQUE (repository_id, name)
);

CREATE TABLE publish_targets (
    id INTEGER PRIMARY KEY,
    repository_id INTEGER NOT NULL,
    name TEXT NOT NULL,
    kind TEXT NOT NULL,
    credentials_id INTEGER,
    enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    config_json TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (repository_id) REFERENCES repositories (id) ON DELETE CASCADE,
    FOREIGN KEY (credentials_id) REFERENCES credentials (id),
    UNIQUE (repository_id, name)
);

-- Bindings are explicit so each build output can target only selected publishers.
CREATE TABLE build_publish_bindings (
    id INTEGER PRIMARY KEY,
    build_target_id INTEGER NOT NULL,
    publish_target_id INTEGER NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    options_json TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (build_target_id) REFERENCES build_targets (id) ON DELETE CASCADE,
    FOREIGN KEY (publish_target_id) REFERENCES publish_targets (id) ON DELETE CASCADE,
    UNIQUE (build_target_id, publish_target_id)
);

CREATE TABLE release_runs (
    id INTEGER PRIMARY KEY,
    repository_id INTEGER NOT NULL,
    git_tag TEXT NOT NULL,
    git_commit TEXT,
    unity_version TEXT,
    status TEXT NOT NULL CHECK (status IN ('detected', 'queued', 'running', 'succeeded', 'failed', 'canceled')),
    started_at TEXT,
    finished_at TEXT,
    error_message TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (repository_id) REFERENCES repositories (id) ON DELETE CASCADE,
    UNIQUE (repository_id, git_tag)
);

CREATE TABLE build_runs (
    id INTEGER PRIMARY KEY,
    release_run_id INTEGER NOT NULL,
    build_target_id INTEGER NOT NULL,
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

CREATE INDEX idx_build_targets_repository_id ON build_targets (repository_id);
CREATE INDEX idx_publish_targets_repository_id ON publish_targets (repository_id);
CREATE INDEX idx_trigger_rules_repository_source ON trigger_rules (repository_id, source);
CREATE INDEX idx_release_runs_repository_status ON release_runs (repository_id, status);
CREATE INDEX idx_build_runs_release_status ON build_runs (release_run_id, status);
CREATE INDEX idx_artifacts_build_run_id ON artifacts (build_run_id);
CREATE INDEX idx_publish_runs_release_status ON publish_runs (release_run_id, status);