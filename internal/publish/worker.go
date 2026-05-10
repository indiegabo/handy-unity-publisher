package publish

import (
	"context"
	"errors"
	"fmt"
	"time"

	"github.com/indiegabo/handy-unity-bulder/internal/worker"
)

// defaultWorkerDequeueWait is the default blocking interval used while waiting
// for queued publish jobs.
const defaultWorkerDequeueWait = 5 * time.Second

// Worker consumes queued publish jobs and drives publish-run status
// transitions.
type Worker struct {
	store       ExecutionStore
	queue       worker.Queue
	processor   Processor
	dequeueWait time.Duration
}

// NewWorker creates a publish worker over a store, queue, and processor.
func NewWorker(store ExecutionStore, queue worker.Queue, processor Processor) *Worker {
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

// RunOnce consumes at most one queued publish job.
func (w *Worker) RunOnce(ctx context.Context) (bool, error) {
	if w.store == nil {
		return false, fmt.Errorf("%w: publish worker store is required", ErrInvalid)
	}
	if w.queue == nil {
		return false, fmt.Errorf("%w: publish worker queue is required", ErrInvalid)
	}
	if w.processor == nil {
		return false, fmt.Errorf("%w: publish worker processor is required", ErrInvalid)
	}

	payload, err := w.queue.Dequeue(ctx, QueueName, w.dequeueWait)
	if err != nil {
		return false, fmt.Errorf("dequeue publish job: %w", err)
	}
	if payload == nil {
		return false, nil
	}

	job, err := UnmarshalJob(payload)
	if err != nil {
		return true, fmt.Errorf("unmarshal publish job: %w", err)
	}

	run, err := w.store.StartRun(ctx, job.PublishRunID, StartRunInput{})
	if err != nil {
		if errors.Is(err, ErrRunNotQueued) {
			return true, nil
		}

		return true, fmt.Errorf("start publish run %d: %w", job.PublishRunID, err)
	}

	var result ExecutionResult
	plan, err := w.store.GetExecutionPlan(ctx, run.ID)
	if err == nil {
		result, err = w.processor.Process(ctx, WorkItem{
			Job:  job,
			Run:  run,
			Plan: plan,
		})
	}

	if err != nil {
		if _, failErr := w.store.FailRun(ctx, run.ID, FailRunInput{
			DestinationRef: result.DestinationRef,
			ErrorMessage:   err.Error(),
		}); failErr != nil {
			return true, fmt.Errorf("persist failed publish run %d: %w", run.ID, failErr)
		}

		return true, nil
	}

	if _, err := w.store.CompleteRun(ctx, run.ID, CompleteRunInput{
		DestinationRef: result.DestinationRef,
	}); err != nil {
		return true, fmt.Errorf("complete publish run %d: %w", run.ID, err)
	}

	return true, nil
}