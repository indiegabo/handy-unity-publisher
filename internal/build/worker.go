package build

import (
	"context"
	"errors"
	"fmt"
	"log/slog"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/indiegabo/handy-unity-bulder/internal/worker"
)

// defaultWorkerDequeueWait is the blocking dequeue duration used by build
// workers when no override is configured.
const defaultWorkerDequeueWait = 5 * time.Second

// WorkItem is the resolved unit of build work handed to one processor.
type WorkItem struct {
	Job Job
	Run Run
}

// ExecutionResult captures the filesystem outputs a processor discovered while
// handling one build run.
type ExecutionResult struct {
	WorkspacePath    string
	LogPath          string
	ArtifactRootPath string
}

// Processor executes one claimed build run.
type Processor interface {
	Process(ctx context.Context, item WorkItem) (ExecutionResult, error)
}

// ProcessorFunc adapts a function into a build processor.
type ProcessorFunc func(context.Context, WorkItem) (ExecutionResult, error)

// publishPlanner expands a successful build run into downstream publish work.
type publishPlanner interface {
	PlanBuildRun(ctx context.Context, buildRunID int64) error
}

// Process executes one claimed build run through the wrapped function.
func (fn ProcessorFunc) Process(
	ctx context.Context,
	item WorkItem,
) (ExecutionResult, error) {
	return fn(ctx, item)
}

// Worker consumes queued build jobs and drives build-run status transitions.
type Worker struct {
	store       Store
	queue       worker.Queue
	processor   Processor
	publishPlan publishPlanner
	logger      *slog.Logger
	dequeueWait time.Duration
}

// NewWorker creates a build worker over a store, queue, and processor.
func NewWorker(store Store, queue worker.Queue, processor Processor) *Worker {
	return &Worker{
		store:       store,
		queue:       queue,
		processor:   processor,
		dequeueWait: defaultWorkerDequeueWait,
	}
}

// WithDequeueWait overrides the queue blocking time used by RunOnce.
func (w *Worker) WithDequeueWait(wait time.Duration) *Worker {
	if wait > 0 {
		w.dequeueWait = wait
	}

	return w
}

// WithPublishPlanner attaches a publish-run planner that executes after
// artifacts are persisted and before the build run is finalized.
func (w *Worker) WithPublishPlanner(planner publishPlanner) *Worker {
	w.publishPlan = planner
	return w
}

// WithLogger attaches a structured logger used to report operator-visible
// build failures without requiring direct inspection of the nested GameCI
// container.
func (w *Worker) WithLogger(logger *slog.Logger) *Worker {
	w.logger = logger
	return w
}

// RunOnce consumes at most one queued build job.
func (w *Worker) RunOnce(ctx context.Context) (bool, error) {
	if w.store == nil {
		return false, fmt.Errorf("%w: build worker store is required", ErrInvalid)
	}
	if w.queue == nil {
		return false, fmt.Errorf("%w: build worker queue is required", ErrInvalid)
	}
	if w.processor == nil {
		return false, fmt.Errorf("%w: build worker processor is required", ErrInvalid)
	}

	payload, err := w.queue.Dequeue(ctx, QueueName, w.dequeueWait)
	if err != nil {
		return false, fmt.Errorf("dequeue build job: %w", err)
	}
	if payload == nil {
		return false, nil
	}

	job, err := UnmarshalJob(payload)
	if err != nil {
		return true, fmt.Errorf("unmarshal build job: %w", err)
	}

	run, err := w.store.StartRun(ctx, job.BuildRunID, StartRunInput{})
	if err != nil {
		if errors.Is(err, ErrRunNotQueued) {
			return true, nil
		}

		return true, fmt.Errorf("start build run %d: %w", job.BuildRunID, err)
	}

	result, err := w.processor.Process(ctx, WorkItem{Job: job, Run: run})
	if err == nil {
		artifacts, discoverErr := discoverArtifacts(result.ArtifactRootPath)
		if discoverErr != nil {
			err = discoverErr
		} else if _, registerErr := w.store.ReplaceArtifacts(
			ctx,
			run.ID,
			artifacts,
		); registerErr != nil {
			err = fmt.Errorf(
				"register artifacts for build run %d: %w",
				run.ID,
				registerErr,
			)
		}
	}
	if err == nil && w.publishPlan != nil {
		if planErr := w.publishPlan.PlanBuildRun(ctx, run.ID); planErr != nil {
			err = fmt.Errorf("plan publish runs for build run %d: %w", run.ID, planErr)
		}
	}

	if err != nil {
		failedRun, failErr := w.store.FailRun(ctx, run.ID, FailRunInput{
			WorkspacePath:    result.WorkspacePath,
			LogPath:          result.LogPath,
			ArtifactRootPath: result.ArtifactRootPath,
			ErrorMessage:     err.Error(),
		})
		if failErr != nil {
			return true, fmt.Errorf(
				"persist failed build run %d: %w",
				run.ID,
				failErr,
			)
		}

		if w.logger != nil {
			w.logger.Error(
				"build run failed",
				"build_run_id", failedRun.ID,
				"release_run_id", failedRun.ReleaseRunID,
				"build_target_id", failedRun.BuildTargetID,
				"error", pointerString(failedRun.ErrorMessage),
				"log_path", pointerString(failedRun.LogPath),
				"workspace_path", pointerString(failedRun.WorkspacePath),
				"artifact_root_path", pointerString(failedRun.ArtifactRootPath),
			)
		}

		if cleanupErr := cleanupWorkspace(result.WorkspacePath); cleanupErr != nil {
			return true, fmt.Errorf(
				"cleanup workspace for failed build run %d: %w",
				run.ID,
				cleanupErr,
			)
		}

		return true, nil
	}

	if _, err := w.store.CompleteRun(ctx, run.ID, CompleteRunInput{
		WorkspacePath:    result.WorkspacePath,
		LogPath:          result.LogPath,
		ArtifactRootPath: result.ArtifactRootPath,
	}); err != nil {
		return true, fmt.Errorf("complete build run %d: %w", run.ID, err)
	}

	if cleanupErr := cleanupWorkspace(result.WorkspacePath); cleanupErr != nil {
		return true, fmt.Errorf(
			"cleanup workspace for completed build run %d: %w",
			run.ID,
			cleanupErr,
		)
	}

	return true, nil
}

// cleanupWorkspace removes one finished build workspace directory after the
// run has reached a terminal persisted state.
func cleanupWorkspace(workspacePath string) error {
	cleanedPath := filepath.Clean(strings.TrimSpace(workspacePath))
	if cleanedPath == "" || cleanedPath == "." {
		return nil
	}
	if cleanedPath == string(filepath.Separator) {
		return fmt.Errorf("%w: refusing to remove filesystem root", ErrInvalid)
	}

	if err := os.RemoveAll(cleanedPath); err != nil {
		return fmt.Errorf("remove workspace %q: %w", cleanedPath, err)
	}

	return nil
}
