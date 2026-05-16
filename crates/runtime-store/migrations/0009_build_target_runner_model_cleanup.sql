-- Fresh databases already receive the contract-first build target schema from
-- 0002_pipeline_definitions.sql.
--
-- Legacy databases created before that consolidation are upgraded through a
-- schema-aware SQL path selected by runtime-store during migration execution.

SELECT 1;