-- Persists repository access assessment state so UI and runtime can reason
-- about provider, visibility, and auth binding without recomputing it on
-- every inspection read.
ALTER TABLE repositories
ADD COLUMN source_provider_id TEXT;

ALTER TABLE repositories
ADD COLUMN source_instance_url TEXT;

ALTER TABLE repositories
ADD COLUMN visibility_status TEXT NOT NULL DEFAULT 'unknown';

ALTER TABLE repositories
ADD COLUMN auth_requirement_status TEXT NOT NULL DEFAULT 'unknown';

ALTER TABLE repositories
ADD COLUMN auth_binding_status TEXT NOT NULL DEFAULT 'unknown';

ALTER TABLE repositories
ADD COLUMN auth_status_message TEXT NOT NULL DEFAULT '';

ALTER TABLE repositories
ADD COLUMN auth_last_verified_at TEXT;