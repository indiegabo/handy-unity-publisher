-- Persists repository engine selection and rebuilds build targets into the
-- engine-scoped contract model.
ALTER TABLE repositories
ADD COLUMN engine_kind TEXT NOT NULL DEFAULT 'unity';

PRAGMA foreign_keys = OFF;

CREATE TABLE build_targets_v3 (
	id INTEGER PRIMARY KEY,
	repository_id INTEGER NOT NULL,
	name TEXT NOT NULL,
	build_kind TEXT NOT NULL DEFAULT 'player',
	runner_type TEXT NOT NULL DEFAULT 'host-native',
	output_kind TEXT,
	output_path_template TEXT,
	timeout_seconds INTEGER NOT NULL DEFAULT 3600 CHECK (timeout_seconds > 0),
	enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
	contract_json TEXT NOT NULL DEFAULT '{}',
	config_json TEXT NOT NULL DEFAULT '{}',
	created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
	updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
	FOREIGN KEY (repository_id) REFERENCES repositories (id) ON DELETE CASCADE,
	UNIQUE (repository_id, name)
);

INSERT INTO build_targets_v3 (
	id,
	repository_id,
	name,
	build_kind,
	runner_type,
	output_kind,
	output_path_template,
	timeout_seconds,
	enabled,
	contract_json,
	config_json,
	created_at,
	updated_at
)
SELECT id,
	   repository_id,
	   name,
	   'player',
	   runner_type,
	   output_kind,
	   output_path_template,
	   timeout_seconds,
	   enabled,
	   json_object(
		   'unity',
		   json_object(
			   'targetPlatform', platform,
			   'buildMethod', COALESCE(build_method, ''),
			   'editorVersion', COALESCE(unity_version_override, '')
		   )
	   ),
	   config_json,
	   created_at,
	   updated_at
FROM build_targets;

DROP TABLE build_targets;
ALTER TABLE build_targets_v3 RENAME TO build_targets;

CREATE INDEX idx_build_targets_repository_id ON build_targets (repository_id);

PRAGMA foreign_keys = ON;