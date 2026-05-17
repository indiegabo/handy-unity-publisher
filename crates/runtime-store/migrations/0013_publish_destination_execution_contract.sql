-- Persist publish execution snapshots and canonical artifact active locations.
ALTER TABLE artifacts
ADD COLUMN active_location_kind TEXT NOT NULL DEFAULT 'runtime_artifact';
ALTER TABLE artifacts
ADD COLUMN active_location_ref TEXT NOT NULL DEFAULT '';
UPDATE artifacts
SET active_location_kind = CASE
        WHEN TRIM(COALESCE(active_location_kind, '')) = '' THEN 'runtime_artifact'
        ELSE active_location_kind
    END,
    active_location_ref = CASE
        WHEN TRIM(COALESCE(active_location_ref, '')) = '' THEN path
        ELSE active_location_ref
    END;
ALTER TABLE publish_runs
ADD COLUMN execution_contract_json TEXT NOT NULL DEFAULT '{}';