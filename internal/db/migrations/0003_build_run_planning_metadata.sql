-- Persist build planning decisions per build run so execution can consume a
-- deterministic Unity version and container image reference.

ALTER TABLE build_runs
    ADD COLUMN unity_version TEXT;

ALTER TABLE build_runs
    ADD COLUMN image_ref TEXT;