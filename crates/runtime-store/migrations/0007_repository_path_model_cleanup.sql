-- Removes superseded path fields that conflict with the runtime-first model.
--
-- App state remains under one application runtime root, while each repository
-- may only override its managed workspace root and build output root.

PRAGMA foreign_keys = OFF;

CREATE TABLE app_settings_v2 (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    workspace_root TEXT,
    artifacts_root TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

INSERT INTO app_settings_v2 (
    id,
    workspace_root,
    artifacts_root,
    created_at,
    updated_at
)
SELECT id,
       workspace_root,
       artifacts_root,
       created_at,
       updated_at
FROM app_settings;

DROP TABLE app_settings;
ALTER TABLE app_settings_v2 RENAME TO app_settings;

CREATE TABLE repositories_v3 (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    source_mode TEXT NOT NULL DEFAULT 'managed_repository'
        CHECK (source_mode IN ('managed_repository', 'local_workspace')),
    workspace_strategy TEXT NOT NULL DEFAULT 'managed_checkout'
        CHECK (workspace_strategy IN ('managed_checkout', 'direct', 'snapshot')),
    repo_url TEXT,
    local_path TEXT,
    credentials_id INTEGER,
    default_branch TEXT,
    artifacts_root_override TEXT,
    workspace_root_override TEXT,
    polling_interval_seconds INTEGER NOT NULL DEFAULT 300
        CHECK (polling_interval_seconds > 0),
    last_seen_tag TEXT,
    enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (credentials_id) REFERENCES credentials (id),
    UNIQUE (name),
    CHECK (
        (source_mode = 'managed_repository'
         AND workspace_strategy = 'managed_checkout'
         AND repo_url IS NOT NULL
         AND local_path IS NULL)
        OR (source_mode = 'local_workspace'
            AND workspace_strategy IN ('direct', 'snapshot')
            AND repo_url IS NULL
            AND local_path IS NOT NULL)
    )
);

INSERT INTO repositories_v3 (
    id,
    name,
    source_mode,
    workspace_strategy,
    repo_url,
    local_path,
    credentials_id,
    default_branch,
    artifacts_root_override,
    workspace_root_override,
    polling_interval_seconds,
    last_seen_tag,
    enabled,
    created_at,
    updated_at
)
SELECT id,
       name,
       source_mode,
       workspace_strategy,
       repo_url,
       local_path,
       credentials_id,
       default_branch,
       artifacts_root_override,
       workspace_root_override,
       polling_interval_seconds,
       last_seen_tag,
       enabled,
       created_at,
       updated_at
FROM repositories;

DROP TABLE repositories;
ALTER TABLE repositories_v3 RENAME TO repositories;

CREATE UNIQUE INDEX idx_repositories_repo_url_unique
    ON repositories (repo_url)
    WHERE repo_url IS NOT NULL;

CREATE UNIQUE INDEX idx_repositories_local_path_unique
    ON repositories (local_path)
    WHERE local_path IS NOT NULL;

CREATE INDEX idx_repositories_source_mode ON repositories (source_mode);

PRAGMA foreign_keys = ON;
