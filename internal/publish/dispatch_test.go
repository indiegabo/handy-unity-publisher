package publish_test

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"testing"
	"time"

	"github.com/indiegabo/handy-unity-bulder/internal/publish"
	"github.com/indiegabo/handy-unity-bulder/internal/worker"
)

func TestDispatcherQueuesPublishJob(t *testing.T) {
	t.Parallel()

	queue := &publishQueueStub{}
	dispatcher := publish.NewDispatcher(queue)
	artifactID := int64(14)

	run := publish.Run{
		ID:              41,
		ReleaseRunID:    11,
		BuildRunID:      9,
		PublishTargetID: 7,
		ArtifactID:      &artifactID,
		Status:          publish.StatusQueued,
		CreatedAt:       "2026-05-09 23:40:10",
	}

	if err := dispatcher.Dispatch(context.Background(), run); err != nil {
		t.Fatalf("dispatch publish run: %v", err)
	}

	if len(queue.payloads) != 1 {
		t.Fatalf("expected one queued payload, got %d", len(queue.payloads))
	}

	if queue.names[0] != publish.QueueName {
		t.Fatalf("expected queue name %q, got %q", publish.QueueName, queue.names[0])
	}

	var job publish.Job
	if err := json.Unmarshal(queue.payloads[0], &job); err != nil {
		t.Fatalf("decode publish job payload: %v", err)
	}

	if job.PublishRunID != run.ID {
		t.Fatalf("expected publish run id %d, got %d", run.ID, job.PublishRunID)
	}

	if job.ArtifactID == nil || *job.ArtifactID != artifactID {
		t.Fatalf("expected artifact id %d, got %#v", artifactID, job.ArtifactID)
	}
}

func TestDispatcherLeavesClaimReversibleWhenEnqueueFails(t *testing.T) {
	t.Parallel()

	idempotency := &publishIdempotencyStoreStub{claimResult: true}
	dispatcher := publish.NewDispatcher(&publishQueueStub{err: errors.New("queue offline")}).WithCoordination(
		&publishLockManagerStub{ok: true},
		idempotency,
	)
	artifactID := int64(5)

	err := dispatcher.Dispatch(context.Background(), publish.Run{
		ID:              17,
		ReleaseRunID:    3,
		BuildRunID:      4,
		PublishTargetID: 8,
		ArtifactID:      &artifactID,
		Status:          publish.StatusQueued,
		CreatedAt:       "2026-05-09 23:40:11",
	})
	if err == nil {
		t.Fatal("expected enqueue failure")
	}

	if len(idempotency.forgotKeys) != 1 {
		t.Fatalf("expected one forgotten idempotency key, got %d", len(idempotency.forgotKeys))
	}
}

func TestBuildResultDispatcherPlansAndQueuesPublishRuns(t *testing.T) {
	t.Parallel()

	artifactID := int64(99)
	store := &planningStoreStub{
		runs: []publish.Run{{
			ID:              33,
			ReleaseRunID:    12,
			BuildRunID:      18,
			PublishTargetID: 7,
			ArtifactID:      &artifactID,
			Status:          publish.StatusQueued,
			CreatedAt:       "2026-05-09 23:40:12",
		}},
	}
	queue := &publishQueueStub{}
	handler := publish.NewBuildResultDispatcher(store, publish.NewDispatcher(queue))

	if err := handler.PlanBuildRun(context.Background(), 18); err != nil {
		t.Fatalf("plan and dispatch publish runs: %v", err)
	}

	if len(store.plannedBuildRunIDs) != 1 || store.plannedBuildRunIDs[0] != 18 {
		t.Fatalf("expected build run id 18 to be planned, got %#v", store.plannedBuildRunIDs)
	}

	if len(queue.payloads) != 1 {
		t.Fatalf("expected one queued publish payload, got %d", len(queue.payloads))
	}
}

func TestDispatcherTreatsRecreatedPublishRunIDAsNewDispatchRound(t *testing.T) {
	t.Parallel()

	artifactID := int64(5)
	queue := &publishQueueStub{}
	idempotency := &rememberingPublishIdempotencyStoreStub{}
	dispatcher := publish.NewDispatcher(queue).WithCoordination(
		&publishLockManagerStub{ok: true},
		idempotency,
	)

	firstRun := publish.Run{
		ID:              8,
		ReleaseRunID:    1,
		BuildRunID:      2,
		PublishTargetID: 3,
		ArtifactID:      &artifactID,
		Status:          publish.StatusQueued,
		CreatedAt:       "2026-05-09 23:40:13",
	}
	secondRun := firstRun
	secondRun.CreatedAt = "2026-05-09 23:55:13"

	if err := dispatcher.Dispatch(context.Background(), firstRun); err != nil {
		t.Fatalf("dispatch first publish run: %v", err)
	}
	if err := dispatcher.Dispatch(context.Background(), secondRun); err != nil {
		t.Fatalf("dispatch recreated publish run with same id: %v", err)
	}

	if len(queue.payloads) != 2 {
		t.Fatalf("expected two queued payloads for two publish rounds, got %d", len(queue.payloads))
	}
	if len(idempotency.claimedKeys) != 2 {
		t.Fatalf("expected two claimed keys, got %d", len(idempotency.claimedKeys))
	}
	if idempotency.claimedKeys[0] == idempotency.claimedKeys[1] {
		t.Fatalf("expected distinct publish idempotency keys, got %q", idempotency.claimedKeys[0])
	}
}

type planningStoreStub struct {
	plannedBuildRunIDs []int64
	runs               []publish.Run
	planErr            error
	listErr            error
}

func (s *planningStoreStub) PlanBuildRun(
	_ context.Context,
	buildRunID int64,
) error {
	s.plannedBuildRunIDs = append(s.plannedBuildRunIDs, buildRunID)
	return s.planErr
}

func (s *planningStoreStub) ListRunsByBuildRun(
	_ context.Context,
	_ int64,
) ([]publish.Run, error) {
	if s.listErr != nil {
		return nil, s.listErr
	}

	return append([]publish.Run(nil), s.runs...), nil
}

type publishQueueStub struct {
	names    []string
	payloads [][]byte
	err      error
}

func (q *publishQueueStub) Enqueue(
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

func (q *publishQueueStub) Dequeue(context.Context, string, time.Duration) ([]byte, error) {
	return nil, fmt.Errorf("not implemented")
}

type publishLockManagerStub struct {
	ok  bool
	err error
}

func (l *publishLockManagerStub) Acquire(
	context.Context,
	string,
	time.Duration,
) (worker.Lock, bool, error) {
	if l.err != nil {
		return nil, false, l.err
	}

	return &publishLockStub{}, l.ok, nil
}

type publishLockStub struct{}

func (l *publishLockStub) Key() string                   { return "lock" }
func (l *publishLockStub) Token() string                 { return "token" }
func (l *publishLockStub) Release(context.Context) error { return nil }

type publishIdempotencyStoreStub struct {
	claimResult bool
	claimErr    error
	forgetErr   error
	forgotKeys  []string
}

func (s *publishIdempotencyStoreStub) Claim(
	_ context.Context,
	_ string,
	_ time.Duration,
) (bool, error) {
	if s.claimErr != nil {
		return false, s.claimErr
	}

	return s.claimResult, nil
}

func (s *publishIdempotencyStoreStub) Forget(
	_ context.Context,
	key string,
) error {
	s.forgotKeys = append(s.forgotKeys, key)
	return s.forgetErr
}

type rememberingPublishIdempotencyStoreStub struct {
	claimedKeys []string
	claimedSet  map[string]struct{}
}

func (s *rememberingPublishIdempotencyStoreStub) Claim(
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

func (s *rememberingPublishIdempotencyStoreStub) Forget(
	_ context.Context,
	key string,
) error {
	delete(s.claimedSet, key)
	return nil
}
