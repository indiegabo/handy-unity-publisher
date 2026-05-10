package publish

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"strings"
	"time"

	"github.com/indiegabo/handy-unity-bulder/internal/worker"
)

// QueueName is the Redis queue used to hand off queued publish work.
const QueueName = "publish-runs"

// Dispatch coordination TTLs bound lock ownership and duplicate suppression
// for publish queue handoff.
const (
	defaultDispatchLockTTL        = 30 * time.Second
	defaultDispatchIdempotencyTTL = 24 * time.Hour
)

var (
	// ErrDispatchInProgress reports that another dispatcher currently owns the
	// queue handoff for the same publish run.
	ErrDispatchInProgress = errors.New("publish dispatch in progress")
	// ErrDispatchAlreadyClaimed reports that queue handoff was already claimed.
	ErrDispatchAlreadyClaimed = errors.New("publish dispatch already claimed")
)

// Job is the serializable payload pushed to downstream publish workers.
type Job struct {
	PublishRunID    int64  `json:"publish_run_id"`
	ReleaseRunID    int64  `json:"release_run_id"`
	BuildRunID      int64  `json:"build_run_id"`
	PublishTargetID int64  `json:"publish_target_id"`
	ArtifactID      *int64 `json:"artifact_id,omitempty"`
}

// Dispatcher coordinates publish-run queue handoff.
type Dispatcher struct {
	queue            worker.Queue
	lockManager      worker.LockManager
	idempotencyStore worker.IdempotencyStore
}

// NewDispatcher creates a publish dispatcher over a queue.
func NewDispatcher(queue worker.Queue) *Dispatcher {
	return &Dispatcher{queue: queue}
}

// WithCoordination adds Redis-backed lock and idempotency coordination.
func (d *Dispatcher) WithCoordination(
	lockManager worker.LockManager,
	idempotencyStore worker.IdempotencyStore,
) *Dispatcher {
	d.lockManager = lockManager
	d.idempotencyStore = idempotencyStore
	return d
}

// DispatchMany enqueues all queued publish runs. Already-claimed runs are
// treated as no-ops so repeated handoff stays idempotent.
func (d *Dispatcher) DispatchMany(ctx context.Context, runs []Run) error {
	for _, run := range runs {
		if err := d.Dispatch(ctx, run); err != nil {
			if errors.Is(err, ErrDispatchAlreadyClaimed) {
				continue
			}

			return err
		}
	}

	return nil
}

// Dispatch hands off one queued publish run to the shared queue-backed worker
// path.
func (d *Dispatcher) Dispatch(ctx context.Context, run Run) error {
	if run.ID <= 0 || run.Status != StatusQueued {
		return fmt.Errorf("%w: publish run must be queued before dispatch", ErrInvalid)
	}

	if d.lockManager != nil {
		lock, ok, err := d.lockManager.Acquire(
			ctx,
			dispatchLockKey(run.ID),
			defaultDispatchLockTTL,
		)
		if err != nil {
			return fmt.Errorf("acquire publish dispatch lock: %w", err)
		}
		if !ok {
			return ErrDispatchInProgress
		}
		defer func() {
			_ = lock.Release(ctx)
		}()
	}

	claimedKey := dispatchIdempotencyKey(run.ID, run.CreatedAt)
	claimed := false
	var err error
	if d.idempotencyStore != nil {
		claimed, err = d.idempotencyStore.Claim(
			ctx,
			claimedKey,
			defaultDispatchIdempotencyTTL,
		)
		if err != nil {
			return fmt.Errorf("claim publish dispatch idempotency: %w", err)
		}
		if !claimed {
			return ErrDispatchAlreadyClaimed
		}
	}

	payload, err := MarshalJob(run)
	if err != nil {
		if claimed && d.idempotencyStore != nil {
			_ = d.idempotencyStore.Forget(ctx, claimedKey)
		}
		return fmt.Errorf("marshal publish job: %w", err)
	}

	if err := d.queue.Enqueue(ctx, QueueName, payload); err != nil {
		if claimed && d.idempotencyStore != nil {
			_ = d.idempotencyStore.Forget(ctx, claimedKey)
		}
		return fmt.Errorf("enqueue publish run %d: %w", run.ID, err)
	}

	return nil
}

// planningStore captures the publish planning operations used after a build
// succeeds.
type planningStore interface {
	PlanBuildRun(ctx context.Context, buildRunID int64) error
	ListRunsByBuildRun(ctx context.Context, buildRunID int64) ([]Run, error)
}

// BuildResultDispatcher expands a finished build into queued publish runs and
// immediately dispatches them to the publish worker queue.
type BuildResultDispatcher struct {
	store      planningStore
	dispatcher *Dispatcher
}

// NewBuildResultDispatcher creates a build-result dispatcher over a planning
// store and publish queue dispatcher.
func NewBuildResultDispatcher(
	store planningStore,
	dispatcher *Dispatcher,
) *BuildResultDispatcher {
	return &BuildResultDispatcher{store: store, dispatcher: dispatcher}
}

// PlanBuildRun materializes queued publish runs for a build result and enqueues
// them for downstream publish workers.
func (d *BuildResultDispatcher) PlanBuildRun(
	ctx context.Context,
	buildRunID int64,
) error {
	if d.store == nil {
		return fmt.Errorf("%w: publish planning store is required", ErrInvalid)
	}
	if d.dispatcher == nil {
		return fmt.Errorf("%w: publish dispatcher is required", ErrInvalid)
	}

	if err := d.store.PlanBuildRun(ctx, buildRunID); err != nil {
		return err
	}

	runs, err := d.store.ListRunsByBuildRun(ctx, buildRunID)
	if err != nil {
		return err
	}
	if len(runs) == 0 {
		return nil
	}

	if err := d.dispatcher.DispatchMany(ctx, runs); err != nil {
		return fmt.Errorf("dispatch publish runs for build run %d: %w", buildRunID, err)
	}

	return nil
}

// dispatchLockKey builds the coordination lock key for one publish run.
func dispatchLockKey(publishRunID int64) string {
	return fmt.Sprintf("publish-run:%d:dispatch", publishRunID)
}

// dispatchIdempotencyKey builds the idempotency key for one publish queue
// handoff. The creation timestamp is part of the key so rebuilds that
// recreate publish rows with recycled SQLite ids do not collide with an older
// dispatch claim.
func dispatchIdempotencyKey(publishRunID int64, createdAt string) string {
	publishRunToken := fmt.Sprintf("%d", publishRunID)
	createdAt = strings.TrimSpace(createdAt)
	if createdAt == "" {
		return fmt.Sprintf("publish-run:%s:queued", publishRunToken)
	}

	createdAtToken := strings.NewReplacer(
		" ", "T",
		":", "-",
	).Replace(createdAt)

	return fmt.Sprintf("publish-run:%s:%s:queued", publishRunToken, createdAtToken)
}

// MarshalJob encodes one publish-run record into the downstream job contract.
func MarshalJob(run Run) ([]byte, error) {
	if run.ID <= 0 {
		return nil, fmt.Errorf("%w: publish run id must be greater than zero", ErrInvalid)
	}
	if run.ReleaseRunID <= 0 {
		return nil, fmt.Errorf("%w: release_run_id must be greater than zero", ErrInvalid)
	}
	if run.BuildRunID <= 0 {
		return nil, fmt.Errorf("%w: build_run_id must be greater than zero", ErrInvalid)
	}
	if run.PublishTargetID <= 0 {
		return nil, fmt.Errorf("%w: publish_target_id must be greater than zero", ErrInvalid)
	}
	if run.ArtifactID != nil && *run.ArtifactID <= 0 {
		return nil, fmt.Errorf("%w: artifact_id must be greater than zero when present", ErrInvalid)
	}

	encoded, err := json.Marshal(Job{
		PublishRunID:    run.ID,
		ReleaseRunID:    run.ReleaseRunID,
		BuildRunID:      run.BuildRunID,
		PublishTargetID: run.PublishTargetID,
		ArtifactID:      run.ArtifactID,
	})
	if err != nil {
		return nil, err
	}

	return encoded, nil
}

// UnmarshalJob decodes one queued publish job payload and validates the fields
// required by downstream workers.
func UnmarshalJob(payload []byte) (Job, error) {
	var job Job
	if err := json.Unmarshal(payload, &job); err != nil {
		return Job{}, fmt.Errorf("%w: decode publish job payload: %v", ErrInvalid, err)
	}

	if job.PublishRunID <= 0 {
		return Job{}, fmt.Errorf("%w: publish_run_id must be greater than zero", ErrInvalid)
	}
	if job.ReleaseRunID <= 0 {
		return Job{}, fmt.Errorf("%w: release_run_id must be greater than zero", ErrInvalid)
	}
	if job.BuildRunID <= 0 {
		return Job{}, fmt.Errorf("%w: build_run_id must be greater than zero", ErrInvalid)
	}
	if job.PublishTargetID <= 0 {
		return Job{}, fmt.Errorf("%w: publish_target_id must be greater than zero", ErrInvalid)
	}
	if job.ArtifactID != nil && *job.ArtifactID <= 0 {
		return Job{}, fmt.Errorf("%w: artifact_id must be greater than zero when present", ErrInvalid)
	}

	return job, nil
}
