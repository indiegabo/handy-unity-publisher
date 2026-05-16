-- Persists repository engine selection and the engine-scoped build contract.
ALTER TABLE repositories
ADD COLUMN engine_kind TEXT NOT NULL DEFAULT 'unity';
ALTER TABLE build_targets
ADD COLUMN build_kind TEXT NOT NULL DEFAULT 'player';
ALTER TABLE build_targets
ADD COLUMN contract_json TEXT NOT NULL DEFAULT '{}';