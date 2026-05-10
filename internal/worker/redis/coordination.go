// Package redis implements worker coordination contracts on top of Redis.
package redis

import (
	"context"
	"fmt"
	"time"

	"github.com/google/uuid"
	redisv9 "github.com/redis/go-redis/v9"

	"github.com/indiegabo/handy-unity-bulder/internal/worker"
)

// defaultPrefix namespaces all Redis coordination keys used by the worker
// runtime.
const defaultPrefix = "hgb"

// releaseLockScript deletes a lock key only when the caller still owns the
// lease token.
var releaseLockScript = redisv9.NewScript(`
if redis.call("GET", KEYS[1]) == ARGV[1] then
  return redis.call("DEL", KEYS[1])
end
return 0
`)

// Queue stores worker payloads inside Redis lists.
type Queue struct {
	client *redisv9.Client
	prefix string
}

// LockManager coordinates exclusive work claims using Redis SETNX leases.
type LockManager struct {
	client *redisv9.Client
	prefix string
}

// IdempotencyStore records short-lived operation keys in Redis.
type IdempotencyStore struct {
	client *redisv9.Client
	prefix string
}

// lock is the Redis-backed lease handle returned by the lock manager.
type lock struct {
	client *redisv9.Client
	key    string
	token  string
}

// Interface assertions keep Redis coordination implementations aligned with
// worker contracts.
var _ worker.Queue = (*Queue)(nil)
var _ worker.LockManager = (*LockManager)(nil)
var _ worker.IdempotencyStore = (*IdempotencyStore)(nil)
var _ worker.Lock = (*lock)(nil)

// NewQueue creates a Redis-backed queue implementation.
func NewQueue(client *redisv9.Client) *Queue {
	return &Queue{client: client, prefix: defaultPrefix}
}

// NewLockManager creates a Redis-backed lock manager implementation.
func NewLockManager(client *redisv9.Client) *LockManager {
	return &LockManager{client: client, prefix: defaultPrefix}
}

// NewIdempotencyStore creates a Redis-backed idempotency store.
func NewIdempotencyStore(client *redisv9.Client) *IdempotencyStore {
	return &IdempotencyStore{client: client, prefix: defaultPrefix}
}

// Enqueue appends one payload to the named queue.
func (q *Queue) Enqueue(ctx context.Context, name string, payload []byte) error {
	if err := q.client.RPush(ctx, q.queueKey(name), payload).Err(); err != nil {
		return fmt.Errorf("enqueue %s: %w", name, err)
	}

	return nil
}

// Dequeue blocks for up to wait and returns nil when no payload arrives.
func (q *Queue) Dequeue(
	ctx context.Context,
	name string,
	wait time.Duration,
) ([]byte, error) {
	values, err := q.client.BLPop(ctx, wait, q.queueKey(name)).Result()
	if err == redisv9.Nil {
		return nil, nil
	}
	if err != nil {
		return nil, fmt.Errorf("dequeue %s: %w", name, err)
	}
	if len(values) != 2 {
		return nil, fmt.Errorf("dequeue %s: unexpected response length %d", name, len(values))
	}

	return []byte(values[1]), nil
}

// Acquire attempts to claim a short-lived exclusive lease for a named key.
func (m *LockManager) Acquire(
	ctx context.Context,
	name string,
	ttl time.Duration,
) (worker.Lock, bool, error) {
	token := uuid.NewString()
	key := m.lockKey(name)

	ok, err := m.client.SetNX(ctx, key, token, ttl).Result()
	if err != nil {
		return nil, false, fmt.Errorf("acquire lock %s: %w", name, err)
	}
	if !ok {
		return nil, false, nil
	}

	return &lock{client: m.client, key: key, token: token}, true, nil
}

// Claim records a short-lived operation key and returns false on duplicates.
func (s *IdempotencyStore) Claim(
	ctx context.Context,
	key string,
	ttl time.Duration,
) (bool, error) {
	ok, err := s.client.SetNX(ctx, s.idempotencyKey(key), "1", ttl).Result()
	if err != nil {
		return false, fmt.Errorf("claim idempotency key %s: %w", key, err)
	}

	return ok, nil
}

// Forget clears a previously claimed idempotency key so the operation can be
// retried after a local failure before queue handoff completes.
func (s *IdempotencyStore) Forget(ctx context.Context, key string) error {
	if err := s.client.Del(ctx, s.idempotencyKey(key)).Err(); err != nil {
		return fmt.Errorf("forget idempotency key %s: %w", key, err)
	}

	return nil
}

// Key returns the fully-qualified Redis key for the lock.
func (l *lock) Key() string {
	return l.key
}

// Token returns the randomly generated token used to own the lease.
func (l *lock) Token() string {
	return l.token
}

// Release removes the lease only if the caller still owns the token.
func (l *lock) Release(ctx context.Context) error {
	deleted, err := releaseLockScript.Run(ctx, l.client, []string{l.key}, l.token).Int()
	if err != nil {
		return fmt.Errorf("release lock %s: %w", l.key, err)
	}
	if deleted == 0 {
		return fmt.Errorf("release lock %s: lease was not owned", l.key)
	}

	return nil
}

// queueKey builds the fully-qualified Redis list key for one queue name.
func (q *Queue) queueKey(name string) string {
	return fmt.Sprintf("%s:queue:%s", q.prefix, name)
}

// lockKey builds the fully-qualified Redis key for one lock name.
func (m *LockManager) lockKey(name string) string {
	return fmt.Sprintf("%s:lock:%s", m.prefix, name)
}

// idempotencyKey builds the fully-qualified Redis key for one idempotency
// token.
func (s *IdempotencyStore) idempotencyKey(key string) string {
	return fmt.Sprintf("%s:idempotency:%s", s.prefix, key)
}
