-- Rebuilds build target rows without deprecated runner override columns.

PRAGMA foreign_keys = OFF;

CREATE TABLE build_targets_v2 (
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

INSERT INTO build_targets_v2 (
    id,
    repository_id,
    name,
    platform,
    runner_type,
    build_method,
    output_kind,
    output_path_template,
    unity_version_override,
    timeout_seconds,
    enabled,
    config_json,
    created_at,
    updated_at
)
SELECT id,
       repository_id,
       name,
       platform,
       runner_type,
       build_method,
       output_kind,
       output_path_template,
       unity_version_override,
       timeout_seconds,
       enabled,
       config_json,
       created_at,
       updated_at
FROM build_targets;

DROP TABLE build_targets;
ALTER TABLE build_targets_v2 RENAME TO build_targets;

CREATE INDEX idx_build_targets_repository_id ON build_targets (repository_id);

PRAGMA foreign_keys = ON;