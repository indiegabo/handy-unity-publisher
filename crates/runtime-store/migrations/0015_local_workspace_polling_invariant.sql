-- Enforces the repository polling invariant: only managed repositories carry
-- a positive remote polling cadence, while local workspaces always persist 0.

PRAGMA foreign_keys = OFF;

CREATE TABLE repositories_v4 (
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
    polling_interval_seconds INTEGER NOT NULL DEFAULT 300,
    last_seen_tag TEXT,
    enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    engine_kind TEXT NOT NULL DEFAULT 'unity',
    source_provider_id TEXT,
    source_instance_url TEXT,
    visibility_status TEXT NOT NULL DEFAULT 'unknown',
    auth_requirement_status TEXT NOT NULL DEFAULT 'unknown',
    auth_binding_status TEXT NOT NULL DEFAULT 'unknown',
    auth_status_message TEXT NOT NULL DEFAULT '',
    auth_last_verified_at TEXT,
    FOREIGN KEY (credentials_id) REFERENCES credentials (id),
    UNIQUE (name),
    CHECK (
        (source_mode = 'managed_repository'
         AND workspace_strategy = 'managed_checkout'
         AND repo_url IS NOT NULL
         AND local_path IS NULL
         AND polling_interval_seconds > 0)
        OR (source_mode = 'local_workspace'
            AND workspace_strategy IN ('direct', 'snapshot')
            AND repo_url IS NULL
            AND local_path IS NOT NULL
            AND polling_interval_seconds = 0)
    )
);

INSERT INTO repositories_v4 (
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
    updated_at,
    engine_kind,
    source_provider_id,
    source_instance_url,
    visibility_status,
    auth_requirement_status,
    auth_binding_status,
    auth_status_message,
    auth_last_verified_at
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
       CASE
           WHEN source_mode = 'local_workspace' THEN 0
           ELSE polling_interval_seconds
       END,
       last_seen_tag,
       enabled,
       created_at,
       updated_at,
       engine_kind,
       source_provider_id,
       source_instance_url,
       visibility_status,
       auth_requirement_status,
       auth_binding_status,
       auth_status_message,
       auth_last_verified_at
FROM repositories;

DROP TABLE repositories;
ALTER TABLE repositories_v4 RENAME TO repositories;

CREATE UNIQUE INDEX idx_repositories_repo_url_unique
    ON repositories (repo_url)
    WHERE repo_url IS NOT NULL;

CREATE UNIQUE INDEX idx_repositories_local_path_unique
    ON repositories (local_path)
    WHERE local_path IS NOT NULL;

CREATE INDEX idx_repositories_source_mode ON repositories (source_mode);

PRAGMA foreign_keys = ON;