CREATE TABLE runtime_metadata (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

INSERT INTO runtime_metadata (key, value, updated_at)
VALUES (
    'bootstrap_schema_version',
    '1',
    strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
);