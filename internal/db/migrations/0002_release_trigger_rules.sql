-- Extend legacy bootstrap databases with trigger rule support and explicit
-- release trigger source metadata.

CREATE TABLE IF NOT EXISTS trigger_rules (
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

ALTER TABLE release_runs
    ADD COLUMN trigger_source TEXT NOT NULL DEFAULT 'poll';

ALTER TABLE release_runs
    ADD COLUMN trigger_rule_id INTEGER REFERENCES trigger_rules (id) ON DELETE SET NULL;

ALTER TABLE release_runs
    ADD COLUMN source_metadata_json TEXT NOT NULL DEFAULT '{}';

CREATE INDEX IF NOT EXISTS idx_trigger_rules_repository_source
    ON trigger_rules (repository_id, source);

CREATE INDEX IF NOT EXISTS idx_release_runs_trigger_source_status
    ON release_runs (trigger_source, status);