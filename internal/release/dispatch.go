package release

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"strings"
	"time"

	"github.com/indiegabo/handy-unity-bulder/internal/worker"
)

// QueueName is the Redis queue used to hand off release work to downstream
// planners and workers.
const QueueName = "release-runs"

// Dispatch coordination TTLs bound lock ownership and duplicate suppression
// for release queue handoff.
const (
	defaultDispatchLockTTL        = 30 * time.Second
	defaultDispatchIdempotencyTTL = 24 * time.Hour
)

var (
	// ErrDispatchInProgress reports that another dispatcher currently owns the
	// queue handoff for the same release run.
	ErrDispatchInProgress = errors.New("release dispatch in progress")
	// ErrDispatchAlreadyClaimed reports that queue handoff was already claimed by
	// another dispatcher instance.
	ErrDispatchAlreadyClaimed = errors.New("release dispatch already claimed")
)

// Job is the serializable payload pushed to downstream release workers.
type Job struct {
	ReleaseRunID  int64   `json:"release_run_id"`
	RepositoryID  int64   `json:"repository_id"`
	GitTag        string  `json:"git_tag"`
	GitCommit     *string `json:"git_commit,omitempty"`
	TriggerSource string  `json:"trigger_source"`
	TriggerRuleID *int64  `json:"trigger_rule_id,omitempty"`
}

// Dispatcher coordinates durable release creation and queue handoff.
type Dispatcher struct {
	store            Store
	queue            worker.Queue
	lockManager      worker.LockManager
	idempotencyStore worker.IdempotencyStore
}

// NewDispatcher creates a release dispatcher over a store and queue.
func NewDispatcher(store Store, queue worker.Queue) *Dispatcher {
	return &Dispatcher{store: store, queue: queue}
}

// WithCoordination adds Redis-backed lock and idempotency coordination to the
// dispatcher so concurrent or repeated queue handoffs stay bounded.
func (d *Dispatcher) WithCoordination(
	lockManager worker.LockManager,
	idempotencyStore worker.IdempotencyStore,
) *Dispatcher {
	d.lockManager = lockManager
	d.idempotencyStore = idempotencyStore
	return d
}

// DispatchManual persists one manual release run, enqueues its downstream job,
// and marks the release as queued once the queue handoff succeeds.
func (d *Dispatcher) DispatchManual(
	ctx context.Context,
	input ManualDispatchInput,
) (Record, error) {
	record, err := d.store.CreateManualDispatch(ctx, input)
	if err != nil {
		return Record{}, err
	}

	return d.QueueReleaseRun(ctx, record.ID)
}

// DispatchManualRebuild reuses an existing manual or polling release for the
// same repository tag when present, clears derived build state, and requeues
// the release for another build and publish pass.
func (d *Dispatcher) DispatchManualRebuild(
	ctx context.Context,
	input ManualDispatchInput,
) (Record, error) {
	record, err := d.store.RebuildManualDispatch(ctx, input)
	if err != nil {
		return Record{}, err
	}

	if d.idempotencyStore != nil {
		if err := d.idempotencyStore.Forget(
			ctx,
			dispatchIdempotencyKey(record.ID),
		); err != nil {
			return Record{}, fmt.Errorf(
				"reset release dispatch idempotency: %w",
				err,
			)
		}
	}

	return d.QueueReleaseRun(ctx, record.ID)
}

// DispatchPoll persists one polling-discovered release run, enqueues its
// downstream job, and marks the release as queued once the handoff succeeds.
func (d *Dispatcher) DispatchPoll(
	ctx context.Context,
	input PollDispatchInput,
) (Record, error) {
	record, err := d.store.CreatePollDispatch(ctx, input)
	if err != nil {
		return Record{}, err
	}

	return d.QueueReleaseRun(ctx, record.ID)
}

// DispatchRepositoryPoll persists one polling-discovered release run that was
// scheduled directly from repository automation instead of a trigger rule.
func (d *Dispatcher) DispatchRepositoryPoll(
	ctx context.Context,
	input RepositoryPollDispatchInput,
) (Record, error) {
	record, err := d.store.CreateRepositoryPollDispatch(ctx, input)
	if err != nil {
		return Record{}, err
	}

	return d.QueueReleaseRun(ctx, record.ID)
}

// QueueReleaseRun hands off one detected release run to the shared queue-backed
// worker path. This is the convergence point used by manual and future polling
// dispatch flows.
func (d *Dispatcher) QueueReleaseRun(
	ctx context.Context,
	releaseRunID int64,
) (Record, error) {
	record, err := d.store.Get(ctx, releaseRunID)
	if err != nil {
		return Record{}, err
	}

	if record.Status == StatusQueued {
		return record, nil
	}

	if d.lockManager != nil {
		lock, ok, err := d.lockManager.Acquire(
			ctx,
			dispatchLockKey(record.ID),
			defaultDispatchLockTTL,
		)
		if err != nil {
			return Record{}, fmt.Errorf("acquire release dispatch lock: %w", err)
		}
		if !ok {
			return Record{}, ErrDispatchInProgress
		}
		defer func() {
			_ = lock.Release(ctx)
		}()

		record, err = d.store.Get(ctx, releaseRunID)
		if err != nil {
			return Record{}, err
		}
		if record.Status == StatusQueued {
			return record, nil
		}
	}

	claimedKey := dispatchIdempotencyKey(record.ID)
	claimed := false
	if d.idempotencyStore != nil {
		claimed, err = d.idempotencyStore.Claim(
			ctx,
			claimedKey,
			defaultDispatchIdempotencyTTL,
		)
		if err != nil {
			return Record{}, fmt.Errorf("claim release dispatch idempotency: %w", err)
		}
		if !claimed {
			return Record{}, ErrDispatchAlreadyClaimed
		}
	}

	payload, err := MarshalJob(record)
	if err != nil {
		if claimed && d.idempotencyStore != nil {
			_ = d.idempotencyStore.Forget(ctx, claimedKey)
		}
		return Record{}, fmt.Errorf("marshal release job: %w", err)
	}

	if err := d.queue.Enqueue(ctx, QueueName, payload); err != nil {
		if claimed && d.idempotencyStore != nil {
			_ = d.idempotencyStore.Forget(ctx, claimedKey)
		}
		return Record{}, fmt.Errorf("enqueue release run %d: %w", record.ID, err)
	}

	queued, err := d.store.MarkQueued(ctx, record.ID)
	if err != nil {
		return Record{}, fmt.Errorf("mark release run queued: %w", err)
	}

	return queued, nil
}

// dispatchLockKey builds the coordination lock key for one release run.
func dispatchLockKey(releaseRunID int64) string {
	return fmt.Sprintf("release-run:%d:dispatch", releaseRunID)
}

// dispatchIdempotencyKey builds the idempotency key for one release queue
// handoff.
func dispatchIdempotencyKey(releaseRunID int64) string {
	return fmt.Sprintf("release-run:%d:queued", releaseRunID)
}

// MarshalJob encodes one release-run record into the downstream job contract.
func MarshalJob(record Record) ([]byte, error) {
	encoded, err := json.Marshal(Job{
		ReleaseRunID:  record.ID,
		RepositoryID:  record.RepositoryID,
		GitTag:        record.GitTag,
		GitCommit:     record.GitCommit,
		TriggerSource: record.TriggerSource,
		TriggerRuleID: record.TriggerRuleID,
	})
	if err != nil {
		return nil, err
	}

	return encoded, nil
}

// UnmarshalJob decodes one queued release job payload.
func UnmarshalJob(payload []byte) (Job, error) {
	var job Job
	if err := json.Unmarshal(payload, &job); err != nil {
		return Job{}, fmt.Errorf("%w: decode release job payload: %v", ErrInvalid, err)
	}

	job.GitTag = strings.TrimSpace(job.GitTag)
	job.TriggerSource = strings.TrimSpace(job.TriggerSource)
	if job.GitCommit != nil {
		trimmed := strings.TrimSpace(*job.GitCommit)
		job.GitCommit = &trimmed
	}

	if job.ReleaseRunID <= 0 {
		return Job{}, fmt.Errorf("%w: release_run_id must be greater than zero", ErrInvalid)
	}
	if job.RepositoryID <= 0 {
		return Job{}, fmt.Errorf("%w: repository_id must be greater than zero", ErrInvalid)
	}
	if job.GitTag == "" {
		return Job{}, fmt.Errorf("%w: git_tag must not be empty", ErrInvalid)
	}
	if job.TriggerSource == "" {
		return Job{}, fmt.Errorf("%w: trigger_source must not be empty", ErrInvalid)
	}

	return job, nil
}
