ALTER TABLE build_runs ADD COLUMN current_stage_key TEXT;
ALTER TABLE build_runs ADD COLUMN current_stage_label TEXT;
ALTER TABLE build_runs ADD COLUMN current_stage_status TEXT;
ALTER TABLE build_runs ADD COLUMN heartbeat_at TEXT;
ALTER TABLE build_runs ADD COLUMN last_progress_message TEXT;

CREATE TABLE build_run_steps (
    id INTEGER PRIMARY KEY,
    build_run_id INTEGER NOT NULL,
    position INTEGER NOT NULL DEFAULT 0,
    step_key TEXT NOT NULL,
    step_label TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('running', 'succeeded', 'failed', 'canceled')),
    log_path TEXT NOT NULL,
    last_message TEXT,
    heartbeat_at TEXT,
    started_at TEXT,
    finished_at TEXT,
    error_message TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (build_run_id) REFERENCES build_runs (id) ON DELETE CASCADE,
    UNIQUE (build_run_id, step_key)
);

CREATE INDEX idx_build_run_steps_build_run_position
    ON build_run_steps (build_run_id, position, id);

CREATE INDEX idx_build_run_steps_build_run_status
    ON build_run_steps (build_run_id, status);