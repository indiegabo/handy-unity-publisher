package release_test

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"path/filepath"
	"testing"
	"time"

	"github.com/indiegabo/handy-unity-bulder/internal/config"
	"github.com/indiegabo/handy-unity-bulder/internal/db"
	"github.com/indiegabo/handy-unity-bulder/internal/release"
	"github.com/indiegabo/handy-unity-bulder/internal/repository"
	"github.com/indiegabo/handy-unity-bulder/internal/worker"
)

func TestDispatcherDispatchManualQueuesReleaseJob(t *testing.T) {
	t.Parallel()

	ctx := context.Background()
	database := newDispatchTestDatabase(t)
	repositoryStore := repository.NewStore(database)
	releaseStore := release.NewStore(database)
	queue := &queueStub{}
	dispatcher := release.NewDispatcher(releaseStore, queue)

	repo, err := repositoryStore.Create(ctx, repository.CreateInput{
		Name:    "dispatch-queue",
		RepoURL: "https://example.com/org/dispatch-queue.git",
	})
	if err != nil {
		t.Fatalf("create repository: %v", err)
	}

	record, err := dispatcher.DispatchManual(ctx, release.ManualDispatchInput{
		RepositoryID: repo.ID,
		GitTag:       "v1.4.0",
		GitCommit:    "cafebabe",
		RequestedVia: "cli",
	})
	if err != nil {
		t.Fatalf("dispatch manual release: %v", err)
	}

	if record.Status != release.StatusQueued {
		t.Fatalf("expected queued status, got %q", record.Status)
	}

	if len(queue.payloads) != 1 {
		t.Fatalf("expected one queued payload, got %d", len(queue.payloads))
	}

	if queue.names[0] != release.QueueName {
		t.Fatalf("expected queue name %q, got %q", release.QueueName, queue.names[0])
	}

	var job release.Job
	if err := json.Unmarshal(queue.payloads[0], &job); err != nil {
		t.Fatalf("decode release job payload: %v", err)
	}

	if job.ReleaseRunID != record.ID {
		t.Fatalf("expected job release run id %d, got %d", record.ID, job.ReleaseRunID)
	}

	if job.RepositoryID != repo.ID {
		t.Fatalf("expected job repository id %d, got %d", repo.ID, job.RepositoryID)
	}
}

func TestDispatcherLeavesDetectedStatusWhenQueueEnqueueFails(t *testing.T) {
	t.Parallel()

	ctx := context.Background()
	database := newDispatchTestDatabase(t)
	repositoryStore := repository.NewStore(database)
	releaseStore := release.NewStore(database)
	dispatcher := release.NewDispatcher(
		releaseStore,
		&queueStub{err: errors.New("queue offline")},
	)

	repo, err := repositoryStore.Create(ctx, repository.CreateInput{
		Name:    "dispatch-failure",
		RepoURL: "https://example.com/org/dispatch-failure.git",
	})
	if err != nil {
		t.Fatalf("create repository: %v", err)
	}

	_, err = dispatcher.DispatchManual(ctx, release.ManualDispatchInput{
		RepositoryID: repo.ID,
		GitTag:       "v9.9.9",
	})
	if err == nil {
		t.Fatal("expected dispatch error when queue enqueue fails")
	}

	var status string
	if err := database.QueryRowContext(
		ctx,
		`SELECT status FROM release_runs WHERE repository_id = ? AND git_tag = ?`,
		repo.ID,
		"v9.9.9",
	).Scan(&status); err != nil {
		t.Fatalf("query failed dispatch status: %v", err)
	}

	if status != release.StatusDetected {
		t.Fatalf("expected detected status after enqueue failure, got %q", status)
	}
}

func TestDispatcherQueueReleaseRunRejectsConcurrentDispatch(t *testing.T) {
	t.Parallel()

	ctx := context.Background()
	database := newDispatchTestDatabase(t)
	repositoryStore := repository.NewStore(database)
	releaseStore := release.NewStore(database)
	dispatcher := release.NewDispatcher(releaseStore, &queueStub{}).WithCoordination(
		&lockManagerStub{ok: false},
		nil,
	)

	repo, err := repositoryStore.Create(ctx, repository.CreateInput{
		Name:    "dispatch-lock",
		RepoURL: "https://example.com/org/dispatch-lock.git",
	})
	if err != nil {
		t.Fatalf("create repository: %v", err)
	}

	record, err := releaseStore.CreateManualDispatch(ctx, release.ManualDispatchInput{
		RepositoryID: repo.ID,
		GitTag:       "v2.0.0",
	})
	if err != nil {
		t.Fatalf("create manual dispatch: %v", err)
	}

	_, err = dispatcher.QueueReleaseRun(ctx, record.ID)
	if !errors.Is(err, release.ErrDispatchInProgress) {
		t.Fatalf("expected dispatch in progress error, got %v", err)
	}
}

func TestDispatcherQueueReleaseRunRejectsAlreadyClaimedDispatch(t *testing.T) {
	t.Parallel()

	ctx := context.Background()
	database := newDispatchTestDatabase(t)
	repositoryStore := repository.NewStore(database)
	releaseStore := release.NewStore(database)
	dispatcher := release.NewDispatcher(releaseStore, &queueStub{}).WithCoordination(
		&lockManagerStub{ok: true},
		&idempotencyStoreStub{claimResult: false},
	)

	repo, err := repositoryStore.Create(ctx, repository.CreateInput{
		Name:    "dispatch-claimed",
		RepoURL: "https://example.com/org/dispatch-claimed.git",
	})
	if err != nil {
		t.Fatalf("create repository: %v", err)
	}

	record, err := releaseStore.CreateManualDispatch(ctx, release.ManualDispatchInput{
		RepositoryID: repo.ID,
		GitTag:       "v2.1.0",
	})
	if err != nil {
		t.Fatalf("create manual dispatch: %v", err)
	}

	_, err = dispatcher.QueueReleaseRun(ctx, record.ID)
	if !errors.Is(err, release.ErrDispatchAlreadyClaimed) {
		t.Fatalf("expected dispatch already claimed error, got %v", err)
	}
}

func TestDispatcherClearsIdempotencyClaimWhenEnqueueFails(t *testing.T) {
	t.Parallel()

	ctx := context.Background()
	database := newDispatchTestDatabase(t)
	repositoryStore := repository.NewStore(database)
	releaseStore := release.NewStore(database)
	idempotency := &idempotencyStoreStub{claimResult: true}
	dispatcher := release.NewDispatcher(
		releaseStore,
		&queueStub{err: errors.New("queue offline")},
	).WithCoordination(&lockManagerStub{ok: true}, idempotency)

	repo, err := repositoryStore.Create(ctx, repository.CreateInput{
		Name:    "dispatch-forget",
		RepoURL: "https://example.com/org/dispatch-forget.git",
	})
	if err != nil {
		t.Fatalf("create repository: %v", err)
	}

	record, err := releaseStore.CreateManualDispatch(ctx, release.ManualDispatchInput{
		RepositoryID: repo.ID,
		GitTag:       "v2.2.0",
	})
	if err != nil {
		t.Fatalf("create manual dispatch: %v", err)
	}

	_, err = dispatcher.QueueReleaseRun(ctx, record.ID)
	if err == nil {
		t.Fatal("expected enqueue failure")
	}

	if len(idempotency.forgotKeys) != 1 {
		t.Fatalf("expected one forgotten idempotency key, got %d", len(idempotency.forgotKeys))
	}
}

func TestDispatcherDispatchManualRebuildRequeuesExistingRelease(t *testing.T) {
	t.Parallel()

	ctx := context.Background()
	database := newDispatchTestDatabase(t)
	repositoryStore := repository.NewStore(database)
	releaseStore := release.NewStore(database)
	queue := &queueStub{}
	idempotency := &idempotencyStoreStub{claimResult: true}
	dispatcher := release.NewDispatcher(releaseStore, queue).WithCoordination(
		&lockManagerStub{ok: true},
		idempotency,
	)

	repo, err := repositoryStore.Create(ctx, repository.CreateInput{
		Name:    "dispatch-rebuild",
		RepoURL: "https://example.com/org/dispatch-rebuild.git",
	})
	if err != nil {
		t.Fatalf("create repository: %v", err)
	}

	seed, err := releaseStore.CreateManualDispatch(ctx, release.ManualDispatchInput{
		RepositoryID: repo.ID,
		GitTag:       "v3.0.0",
		RequestedVia: "cli",
	})
	if err != nil {
		t.Fatalf("create seed release: %v", err)
	}
	if _, err := releaseStore.MarkQueued(ctx, seed.ID); err != nil {
		t.Fatalf("mark seed release queued: %v", err)
	}

	rebuilt, err := dispatcher.DispatchManualRebuild(ctx, release.ManualDispatchInput{
		RepositoryID: repo.ID,
		GitTag:       "v3.0.0",
		GitCommit:    "feedface",
		RequestedVia: "hub",
	})
	if err != nil {
		t.Fatalf("dispatch rebuild: %v", err)
	}

	if rebuilt.ID != seed.ID {
		t.Fatalf("expected reused release id %d, got %d", seed.ID, rebuilt.ID)
	}
	if rebuilt.Status != release.StatusQueued {
		t.Fatalf("expected queued status, got %q", rebuilt.Status)
	}
	if len(queue.payloads) != 1 {
		t.Fatalf("expected one queued payload, got %d", len(queue.payloads))
	}
	if len(idempotency.forgotKeys) != 1 {
		t.Fatalf("expected one forgotten idempotency key, got %d", len(idempotency.forgotKeys))
	}
	if want := fmt.Sprintf("release-run:%d:queued", seed.ID); idempotency.forgotKeys[0] != want {
		t.Fatalf("expected forgotten idempotency key %q, got %q", want, idempotency.forgotKeys[0])
	}
}

type queueStub struct {
	names    []string
	payloads [][]byte
	err      error
}

func (q *queueStub) Enqueue(
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

func (q *queueStub) Dequeue(context.Context, string, time.Duration) ([]byte, error) {
	return nil, fmt.Errorf("not implemented")
}

type lockManagerStub struct {
	ok  bool
	err error
}

func (l *lockManagerStub) Acquire(
	context.Context,
	string,
	time.Duration,
) (worker.Lock, bool, error) {
	if l.err != nil {
		return nil, false, l.err
	}

	return &lockStub{}, l.ok, nil
}

type lockStub struct{}

func (l *lockStub) Key() string                   { return "lock" }
func (l *lockStub) Token() string                 { return "token" }
func (l *lockStub) Release(context.Context) error { return nil }

type idempotencyStoreStub struct {
	claimResult bool
	claimErr    error
	forgetErr   error
	forgotKeys  []string
}

func (s *idempotencyStoreStub) Claim(
	_ context.Context,
	_ string,
	_ time.Duration,
) (bool, error) {
	if s.claimErr != nil {
		return false, s.claimErr
	}

	return s.claimResult, nil
}

func (s *idempotencyStoreStub) Forget(
	_ context.Context,
	key string,
) error {
	s.forgotKeys = append(s.forgotKeys, key)
	return s.forgetErr
}

func newDispatchTestDatabase(t *testing.T) *sql.DB {
	t.Helper()

	dataDir := t.TempDir()
	cfg := config.Config{
		DataDir:      dataDir,
		DatabasePath: filepath.Join(dataDir, "test.db"),
	}

	database, err := db.Open(context.Background(), cfg)
	if err != nil {
		t.Fatalf("open test database: %v", err)
	}

	t.Cleanup(func() {
		if err := database.Close(); err != nil {
			t.Fatalf("close test database: %v", err)
		}
	})

	return database
}
