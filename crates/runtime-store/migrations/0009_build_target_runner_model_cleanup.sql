-- Fresh databases already receive the contract-first build target schema from
-- 0002_pipeline_definitions.sql.
--
-- Databases created before that consolidation are no longer migrated in place.
-- Operators must reset local runtime state instead of carrying forward the
-- pre-contract build target schema.

SELECT 1;