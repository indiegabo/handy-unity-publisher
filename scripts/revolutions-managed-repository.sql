-- Registers the first runtime-only managed_repository slice for
-- https://github.com/indiegabo/revolutions.git directly in SQLite.
--
-- Execute this against the application runtime database after runtime bootstrap.
-- The database belongs to the app-level runtime root, not to one repository.
-- The script is intentionally narrow: it seeds one managed repository,
-- one manual trigger rule, and one Windows host-native build target.
--
-- The runtime auto-manages repository checkouts, build logs, and transient
-- build workspaces under one repository-specific workspace root.
--
-- This first validation slice does not enable polling and does not register
-- publish targets. Those concerns should be added only when the runtime slice
-- under test explicitly needs them.
--
-- Replace __REVOLUTIONS_PROJECT_PAT__ locally before execution. The repository
-- does not commit live credentials, even for disposable test tokens.

BEGIN IMMEDIATE;

INSERT INTO credentials (
    name,
    kind,
    config_json
) VALUES (
    'Revolutions/origin',
    'git-http-basic',
    '{"username":"indiegabo","password":"__REVOLUTIONS_PROJECT_PAT__"}'
)
ON CONFLICT(name) DO UPDATE SET
    kind = excluded.kind,
    config_json = excluded.config_json,
    updated_at = CURRENT_TIMESTAMP;

INSERT INTO repositories (
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
    enabled
) VALUES (
    'Revolutions',
    'managed_repository',
    'managed_checkout',
    'https://github.com/indiegabo/revolutions.git',
    NULL,
    (SELECT id FROM credentials WHERE name = 'Revolutions/origin'),
    'main',
    'D:\Users\gabao\Revolutions\builds-output',
    'D:\Users\gabao\RevolutionsHandyUnityBuilderWorkspace',
    300,
    NULL,
    1
)
ON CONFLICT(name) DO UPDATE SET
    source_mode = excluded.source_mode,
    workspace_strategy = excluded.workspace_strategy,
    repo_url = excluded.repo_url,
    local_path = excluded.local_path,
    credentials_id = excluded.credentials_id,
    default_branch = excluded.default_branch,
    artifacts_root_override = excluded.artifacts_root_override,
    workspace_root_override = excluded.workspace_root_override,
    polling_interval_seconds = excluded.polling_interval_seconds,
    last_seen_tag = excluded.last_seen_tag,
    enabled = excluded.enabled,
    updated_at = CURRENT_TIMESTAMP;

INSERT INTO trigger_rules (
    repository_id,
    name,
    source,
    enabled,
    config_json
) VALUES (
    (SELECT id FROM repositories WHERE name = 'Revolutions'),
    'manual-build-now',
    'manual',
    1,
    '{}'
)
ON CONFLICT(repository_id, name) DO UPDATE SET
    source = excluded.source,
    enabled = excluded.enabled,
    config_json = excluded.config_json,
    updated_at = CURRENT_TIMESTAMP;

INSERT INTO build_targets (
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
    config_json
) VALUES (
    (SELECT id FROM repositories WHERE name = 'Revolutions'),
    'windows-player',
    'windows',
    'host-native',
    'Builder.PerformWindows',
    'archive',
    'Builds/Players',
    '6000.4.3f1',
    5400,
    1,
    '{"unity_executable_path":"C:\\Program Files\\Unity\\Hub\\Editor\\6000.4.3f1\\Editor\\Unity.exe"}'
)
ON CONFLICT(repository_id, name) DO UPDATE SET
    platform = excluded.platform,
    runner_type = excluded.runner_type,
    build_method = excluded.build_method,
    output_kind = excluded.output_kind,
    output_path_template = excluded.output_path_template,
    unity_version_override = excluded.unity_version_override,
    timeout_seconds = excluded.timeout_seconds,
    enabled = excluded.enabled,
    config_json = excluded.config_json,
    updated_at = CURRENT_TIMESTAMP;

COMMIT;