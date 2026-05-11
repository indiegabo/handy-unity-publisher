-- Creates durable local queue, lease, and idempotency tables.

CREATE TABLE worker_queue_messages (
    id INTEGER PRIMARY KEY,
    queue_name TEXT NOT NULL,
    payload BLOB NOT NULL,
    leased_by TEXT,
    lease_token TEXT,
    lease_expires_at_unix_millis INTEGER,
    dequeue_count INTEGER NOT NULL DEFAULT 0 CHECK (dequeue_count >= 0),
    enqueued_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE worker_coordination_leases (
    name TEXT PRIMARY KEY,
    token TEXT NOT NULL,
    lease_expires_at_unix_millis INTEGER NOT NULL,
    acquired_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE worker_idempotency_keys (
    idempotency_key TEXT PRIMARY KEY,
    claim_expires_at_unix_millis INTEGER NOT NULL,
    claimed_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_worker_queue_messages_claim
    ON worker_queue_messages (queue_name, lease_expires_at_unix_millis, id);

CREATE INDEX idx_worker_coordination_leases_expiry
    ON worker_coordination_leases (lease_expires_at_unix_millis);

CREATE INDEX idx_worker_idempotency_keys_expiry
    ON worker_idempotency_keys (claim_expires_at_unix_millis);