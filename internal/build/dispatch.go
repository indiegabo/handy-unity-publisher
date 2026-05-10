package build

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"strings"
	"time"

	"github.com/indiegabo/handy-unity-bulder/internal/worker"
)

// QueueName is the Redis queue used to hand off planned build work.
const QueueName = "build-runs"

// defaultDispatchLockTTL and defaultDispatchIdempotencyTTL bound Redis-backed
// coordination for build queue handoff.
const (
	defaultDispatchLockTTL        = 30 * time.Second
	defaultDispatchIdempotencyTTL = 24 * time.Hour
)

var (
	// ErrDispatchInProgress reports that another dispatcher currently owns the
	// queue handoff for the same build run.
	ErrDispatchInProgress = errors.New("build dispatch in progress")
	// ErrDispatchAlreadyClaimed reports that queue handoff was already claimed.
	ErrDispatchAlreadyClaimed = errors.New("build dispatch already claimed")
)

// Job is the serializable payload pushed to downstream build workers.
type Job struct {
	BuildRunID    int64  `json:"build_run_id"`
	ReleaseRunID  int64  `json:"release_run_id"`
	BuildTargetID int64  `json:"build_target_id"`
	UnityVersion  string `json:"unity_version"`
	ImageRef      string `json:"image_ref"`
}

// Dispatcher coordinates build-run queue handoff.
type Dispatcher struct {
	queue            worker.Queue
	lockManager      worker.LockManager
	idempotencyStore worker.IdempotencyStore
}

// NewDispatcher creates a build dispatcher over a queue.
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

// DispatchMany enqueues all planned build runs. Already-claimed runs are
// treated as no-ops so repeated planning stays idempotent.
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

// Dispatch hands off one queued build run to the shared queue-backed worker
// path.
func (d *Dispatcher) Dispatch(ctx context.Context, run Run) error {
	if run.ID <= 0 || run.Status != StatusQueued {
		return fmt.Errorf("%w: build run must be queued before dispatch", ErrInvalid)
	}

	if d.lockManager != nil {
		lock, ok, err := d.lockManager.Acquire(
			ctx,
			dispatchLockKey(run.ID),
			defaultDispatchLockTTL,
		)
		if err != nil {
			return fmt.Errorf("acquire build dispatch lock: %w", err)
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
			return fmt.Errorf("claim build dispatch idempotency: %w", err)
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
		return fmt.Errorf("marshal build job: %w", err)
	}

	if err := d.queue.Enqueue(ctx, QueueName, payload); err != nil {
		if claimed && d.idempotencyStore != nil {
			_ = d.idempotencyStore.Forget(ctx, claimedKey)
		}
		return fmt.Errorf("enqueue build run %d: %w", run.ID, err)
	}

	return nil
}

// dispatchLockKey builds the Redis lock key used to serialize queue handoff
// for one build run.
func dispatchLockKey(buildRunID int64) string {
	return fmt.Sprintf("build-run:%d:dispatch", buildRunID)
}

// dispatchIdempotencyKey builds the Redis key used to remember successful
// queue handoff for one build run. The creation timestamp is part of the key
// so rebuilds that recreate rows with recycled SQLite ids do not collide with
// stale idempotency claims from an older execution round.
func dispatchIdempotencyKey(buildRunID int64, createdAt string) string {
	buildRunToken := fmt.Sprintf("%d", buildRunID)
	createdAt = strings.TrimSpace(createdAt)
	if createdAt == "" {
		return fmt.Sprintf("build-run:%s:queued", buildRunToken)
	}

	createdAtToken := strings.NewReplacer(
		" ", "T",
		":", "-",
	).Replace(createdAt)

	return fmt.Sprintf("build-run:%s:%s:queued", buildRunToken, createdAtToken)
}

// MarshalJob encodes one build-run record into the downstream job contract.
func MarshalJob(run Run) ([]byte, error) {
	unityVersion := ""
	if run.UnityVersion != nil {
		unityVersion = strings.TrimSpace(*run.UnityVersion)
	}
	imageRef := ""
	if run.ImageRef != nil {
		imageRef = strings.TrimSpace(*run.ImageRef)
	}

	if unityVersion == "" || imageRef == "" {
		return nil, fmt.Errorf(
			"%w: build run %d is missing planned image metadata",
			ErrInvalid,
			run.ID,
		)
	}

	encoded, err := json.Marshal(Job{
		BuildRunID:    run.ID,
		ReleaseRunID:  run.ReleaseRunID,
		BuildTargetID: run.BuildTargetID,
		UnityVersion:  unityVersion,
		ImageRef:      imageRef,
	})
	if err != nil {
		return nil, err
	}

	return encoded, nil
}

// UnmarshalJob decodes one queued build job payload and validates the fields
// required by downstream workers.
func UnmarshalJob(payload []byte) (Job, error) {
	var job Job
	if err := json.Unmarshal(payload, &job); err != nil {
		return Job{}, fmt.Errorf("%w: decode build job payload: %v", ErrInvalid, err)
	}

	job.UnityVersion = strings.TrimSpace(job.UnityVersion)
	job.ImageRef = strings.TrimSpace(job.ImageRef)

	if job.BuildRunID <= 0 {
		return Job{}, fmt.Errorf("%w: build_run_id must be greater than zero", ErrInvalid)
	}
	if job.ReleaseRunID <= 0 {
		return Job{}, fmt.Errorf("%w: release_run_id must be greater than zero", ErrInvalid)
	}
	if job.BuildTargetID <= 0 {
		return Job{}, fmt.Errorf("%w: build_target_id must be greater than zero", ErrInvalid)
	}
	if job.UnityVersion == "" || job.ImageRef == "" {
		return Job{}, fmt.Errorf(
			"%w: build job %d is missing planned image metadata",
			ErrInvalid,
			job.BuildRunID,
		)
	}

	return job, nil
}
