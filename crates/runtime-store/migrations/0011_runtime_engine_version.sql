-- Rename execution-level Unity-specific version columns to generic engine_version
-- so release and build runtime facts stay engine-aware outside Unity contracts.

ALTER TABLE release_runs RENAME COLUMN unity_version TO engine_version;
ALTER TABLE build_runs RENAME COLUMN unity_version TO engine_version;
