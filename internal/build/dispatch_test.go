package build_test

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"testing"
	"time"

	"github.com/indiegabo/handy-unity-bulder/internal/build"
	"github.com/indiegabo/handy-unity-bulder/internal/worker"
)

func TestDispatcherQueuesBuildJob(t *testing.T) {
	t.Parallel()

	queue := &buildQueueStub{}
	dispatcher := build.NewDispatcher(queue)
	unityVersion := "2022.3.14f1"
	imageRef := "unityci/editor:ubuntu-2022.3.14f1-webgl-3"

	run := build.Run{
		ID:            41,
		ReleaseRunID:  11,
		BuildTargetID: 7,
		UnityVersion:  &unityVersion,
		ImageRef:      &imageRef,
		Status:        build.StatusQueued,
		CreatedAt:     "2026-05-09 23:40:00",
	}

	if err := dispatcher.Dispatch(context.Background(), run); err != nil {
		t.Fatalf("dispatch build run: %v", err)
	}

	if len(queue.payloads) != 1 {
		t.Fatalf("expected one queued payload, got %d", len(queue.payloads))
	}

	if queue.names[0] != build.QueueName {
		t.Fatalf("expected queue name %q, got %q", build.QueueName, queue.names[0])
	}

	var job build.Job
	if err := json.Unmarshal(queue.payloads[0], &job); err != nil {
		t.Fatalf("decode build job payload: %v", err)
	}

	if job.BuildRunID != run.ID {
		t.Fatalf("expected build run id %d, got %d", run.ID, job.BuildRunID)
	}

	if job.ImageRef != imageRef {
		t.Fatalf("expected image ref %q, got %q", imageRef, job.ImageRef)
	}
}

func TestDispatcherLeavesClaimReversibleWhenEnqueueFails(t *testing.T) {
	t.Parallel()

	idempotency := &buildIdempotencyStoreStub{claimResult: true}
	dispatcher := build.NewDispatcher(&buildQueueStub{err: errors.New("queue offline")}).WithCoordination(
		&buildLockManagerStub{ok: true},
		idempotency,
	)
	unityVersion := "2022.3.14f1"
	imageRef := "unityci/editor:ubuntu-2022.3.14f1-base-3"

	err := dispatcher.Dispatch(context.Background(), build.Run{
		ID:            17,
		ReleaseRunID:  3,
		BuildTargetID: 5,
		UnityVersion:  &unityVersion,
		ImageRef:      &imageRef,
		Status:        build.StatusQueued,
		CreatedAt:     "2026-05-09 23:40:01",
	})
	if err == nil {
		t.Fatal("expected enqueue failure")
	}

	if len(idempotency.forgotKeys) != 1 {
		t.Fatalf("expected one forgotten idempotency key, got %d", len(idempotency.forgotKeys))
	}
}

func TestDispatchManyIgnoresAlreadyClaimedBuildRuns(t *testing.T) {
	t.Parallel()

	dispatcher := build.NewDispatcher(&buildQueueStub{}).WithCoordination(
		&buildLockManagerStub{ok: true},
		&buildIdempotencyStoreStub{claimResult: false},
	)
	unityVersion := "2022.3.14f1"
	imageRef := "unityci/editor:ubuntu-2022.3.14f1-base-3"

	err := dispatcher.DispatchMany(context.Background(), []build.Run{{
		ID:            23,
		ReleaseRunID:  9,
		BuildTargetID: 4,
		UnityVersion:  &unityVersion,
		ImageRef:      &imageRef,
		Status:        build.StatusQueued,
		CreatedAt:     "2026-05-09 23:40:02",
	}})
	if err != nil {
		t.Fatalf("dispatch many should ignore already claimed run, got %v", err)
	}
}

func TestDispatcherTreatsRecreatedBuildRunIDAsNewDispatchRound(t *testing.T) {
	t.Parallel()

	queue := &buildQueueStub{}
	idempotency := &rememberingBuildIdempotencyStoreStub{}
	dispatcher := build.NewDispatcher(queue).WithCoordination(
		&buildLockManagerStub{ok: true},
		idempotency,
	)
	unityVersion := "2022.3.14f1"
	imageRef := "unityci/editor:ubuntu-2022.3.14f1-base-3"

	firstRun := build.Run{
		ID:            7,
		ReleaseRunID:  1,
		BuildTargetID: 2,
		UnityVersion:  &unityVersion,
		ImageRef:      &imageRef,
		Status:        build.StatusQueued,
		CreatedAt:     "2026-05-09 23:40:03",
	}
	secondRun := firstRun
	secondRun.CreatedAt = "2026-05-09 23:55:03"

	if err := dispatcher.Dispatch(context.Background(), firstRun); err != nil {
		t.Fatalf("dispatch first build run: %v", err)
	}
	if err := dispatcher.Dispatch(context.Background(), secondRun); err != nil {
		t.Fatalf("dispatch recreated build run with same id: %v", err)
	}

	if len(queue.payloads) != 2 {
		t.Fatalf("expected two queued payloads for two dispatch rounds, got %d", len(queue.payloads))
	}
	if len(idempotency.claimedKeys) != 2 {
		t.Fatalf("expected two claimed keys, got %d", len(idempotency.claimedKeys))
	}
	if idempotency.claimedKeys[0] == idempotency.claimedKeys[1] {
		t.Fatalf("expected distinct idempotency keys, got %q", idempotency.claimedKeys[0])
	}
}

type buildQueueStub struct {
	names    []string
	payloads [][]byte
	err      error
}

func (q *buildQueueStub) Enqueue(
	_ context.Context,
	name string,
	payload []byte,
) error {
	if q.err != nil {
		return q.err
	}

	q.names = append(q.names, name)
	q.payloads = append(q.payloads, append([]byte(nil), payload...))
	return nil
}

func (q *buildQueueStub) Dequeue(context.Context, string, time.Duration) ([]byte, error) {
	return nil, fmt.Errorf("not implemented")
}

type buildLockManagerStub struct {
	ok  bool
	err error
}

func (l *buildLockManagerStub) Acquire(
	context.Context,
	string,
	time.Duration,
) (worker.Lock, bool, error) {
	if l.err != nil {
		return nil, false, l.err
	}

	return &buildLockStub{}, l.ok, nil
}

type buildLockStub struct{}

func (l *buildLockStub) Key() string                   { return "lock" }
func (l *buildLockStub) Token() string                 { return "token" }
func (l *buildLockStub) Release(context.Context) error { return nil }

type buildIdempotencyStoreStub struct {
	claimResult bool
	claimErr    error
	forgetErr   error
	forgotKeys  []string
}

func (s *buildIdempotencyStoreStub) Claim(
	_ context.Context,
	_ string,
	_ time.Duration,
) (bool, error) {
	if s.claimErr != nil {
		return false, s.claimErr
	}

	return s.claimResult, nil
}

func (s *buildIdempotencyStoreStub) Forget(
	_ context.Context,
	key string,
) error {
	s.forgotKeys = append(s.forgotKeys, key)
	return s.forgetErr
}

type rememberingBuildIdempotencyStoreStub struct {
	claimedKeys []string
	claimedSet  map[string]struct{}
}

func (s *rememberingBuildIdempotencyStoreStub) Claim(
	_ context.Context,
	key string,
	_ time.Duration,
) (bool, error) {
	if s.claimedSet == nil {
		s.claimedSet = make(map[string]struct{})
	}
	if _, exists := s.claimedSet[key]; exists {
		return false, nil
	}

	s.claimedSet[key] = struct{}{}
	s.claimedKeys = append(s.claimedKeys, key)
	return true, nil
}

func (s *rememberingBuildIdempotencyStoreStub) Forget(
	_ context.Context,
	key string,
) error {
	delete(s.claimedSet, key)
	return nil
}
