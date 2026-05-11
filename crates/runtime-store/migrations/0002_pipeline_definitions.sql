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
    runner_type TEXT NOT NULL DEFAULT 'host-native',
    build_method TEXT,
    output_kind TEXT,
    output_path_template TEXT,
    unity_version_override TEXT,
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

CREATE INDEX idx_build_targets_repository_id ON build_targets (repository_id);
CREATE INDEX idx_publish_targets_repository_id ON publish_targets (repository_id);
CREATE INDEX idx_trigger_rules_repository_source ON trigger_rules (repository_id, source);