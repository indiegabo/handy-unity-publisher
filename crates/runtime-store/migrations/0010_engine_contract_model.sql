-- Persists repository engine selection for the contract-first schema.
ALTER TABLE repositories
ADD COLUMN engine_kind TEXT NOT NULL DEFAULT 'unity';
