package redis

import (
	"context"
	"testing"
	"time"

	miniredis "github.com/alicebob/miniredis/v2"
	redisv9 "github.com/redis/go-redis/v9"
)

func TestQueueRoundTrip(t *testing.T) {
	client := newRedisClient(t)
	queue := NewQueue(client)
	payload := []byte(`{"job":"build","target":"linux"}`)

	if err := queue.Enqueue(context.Background(), "builds", payload); err != nil {
		t.Fatalf("Enqueue() error = %v", err)
	}

	got, err := queue.Dequeue(context.Background(), "builds", time.Second)
	if err != nil {
		t.Fatalf("Dequeue() error = %v", err)
	}
	if string(got) != string(payload) {
		t.Fatalf("Dequeue() = %q, want %q", string(got), string(payload))
	}
}

func TestLockManagerAcquireAndRelease(t *testing.T) {
	client := newRedisClient(t)
	locks := NewLockManager(client)

	first, ok, err := locks.Acquire(context.Background(), "release:v1.2.3", time.Minute)
	if err != nil {
		t.Fatalf("Acquire() error = %v", err)
	}
	if !ok {
		t.Fatal("Acquire() = not acquired, want acquired")
	}

	if _, ok, err := locks.Acquire(context.Background(), "release:v1.2.3", time.Minute); err != nil {
		t.Fatalf("second Acquire() error = %v", err)
	} else if ok {
		t.Fatal("second Acquire() = acquired, want contention")
	}

	if err := first.Release(context.Background()); err != nil {
		t.Fatalf("Release() error = %v", err)
	}

	if _, ok, err := locks.Acquire(context.Background(), "release:v1.2.3", time.Minute); err != nil {
		t.Fatalf("third Acquire() error = %v", err)
	} else if !ok {
		t.Fatal("third Acquire() = not acquired, want lease available after release")
	}
}

func TestIdempotencyStoreClaimOnce(t *testing.T) {
	client := newRedisClient(t)
	store := NewIdempotencyStore(client)

	first, err := store.Claim(context.Background(), "repo-1:v1.0.0", time.Minute)
	if err != nil {
		t.Fatalf("first Claim() error = %v", err)
	}
	if !first {
		t.Fatal("first Claim() = false, want true")
	}

	second, err := store.Claim(context.Background(), "repo-1:v1.0.0", time.Minute)
	if err != nil {
		t.Fatalf("second Claim() error = %v", err)
	}
	if second {
		t.Fatal("second Claim() = true, want false")
	}
}

func TestIdempotencyStoreForgetAllowsRetry(t *testing.T) {
	client := newRedisClient(t)
	store := NewIdempotencyStore(client)

	claimed, err := store.Claim(context.Background(), "repo-1:v2.0.0", time.Minute)
	if err != nil {
		t.Fatalf("Claim() error = %v", err)
	}
	if !claimed {
		t.Fatal("Claim() = false, want true")
	}

	if err := store.Forget(context.Background(), "repo-1:v2.0.0"); err != nil {
		t.Fatalf("Forget() error = %v", err)
	}

	retry, err := store.Claim(context.Background(), "repo-1:v2.0.0", time.Minute)
	if err != nil {
		t.Fatalf("second Claim() error = %v", err)
	}
	if !retry {
		t.Fatal("second Claim() = false, want true after Forget")
	}
}

func newRedisClient(t *testing.T) *redisv9.Client {
	t.Helper()

	server := miniredis.RunT(t)
	client := redisv9.NewClient(&redisv9.Options{Addr: server.Addr()})
	t.Cleanup(func() {
		_ = client.Close()
		server.Close()
	})

	return client
}
