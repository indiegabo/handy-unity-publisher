// Package worker defines coordination contracts for queue-driven background
// execution.
package worker

import (
	"context"
	"time"
)

// Queue stores serializable job payloads for background workers.
type Queue interface {
	// Enqueue appends one payload to the named queue.
	Enqueue(ctx context.Context, name string, payload []byte) error
	// Dequeue blocks for up to wait and returns nil when the queue stays empty.
	Dequeue(ctx context.Context, name string, wait time.Duration) ([]byte, error)
}

// Lock represents one acquired lease over a coordination key.
type Lock interface {
	Key() string
	Token() string
	Release(ctx context.Context) error
}

// LockManager coordinates exclusive work claims across workers.
type LockManager interface {
	Acquire(ctx context.Context, name string, ttl time.Duration) (Lock, bool, error)
}

// IdempotencyStore tracks short-lived operation keys to prevent duplicate work.
type IdempotencyStore interface {
	Claim(ctx context.Context, key string, ttl time.Duration) (bool, error)
	Forget(ctx context.Context, key string) error
}
