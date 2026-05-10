package publish

import (
	"context"
	"errors"
	"fmt"
)

var (
	// ErrUnsupportedTargetKind reports execution attempts for publish target
	// kinds that do not yet have a concrete publisher implementation.
	ErrUnsupportedTargetKind = errors.New("unsupported publish target kind")
)

// WorkItem is the resolved unit of publish work handed to one processor.
type WorkItem struct {
	Job  Job
	Run  Run
	Plan ExecutionPlan
}

// ExecutionResult captures the filesystem destination produced by one publish
// execution.
type ExecutionResult struct {
	DestinationRef string
}

// Processor executes one claimed publish run.
type Processor interface {
	Process(ctx context.Context, item WorkItem) (ExecutionResult, error)
}

// ProcessorFunc adapts a function into a publish processor.
type ProcessorFunc func(context.Context, WorkItem) (ExecutionResult, error)

// Process executes one publish work item through the wrapped function.
func (fn ProcessorFunc) Process(
	ctx context.Context,
	item WorkItem,
) (ExecutionResult, error) {
	return fn(ctx, item)
}

// artifactPublisher abstracts concrete publisher implementations behind the
// execution processor.
type artifactPublisher interface {
	Publish(ctx context.Context, plan ExecutionPlan) (ExecutionResult, error)
}

// executionProcessor selects the concrete publisher implementation for one
// publish run.
type executionProcessor struct {
	filesystem artifactPublisher
}

// NewExecutionProcessor creates the publish processor used by the dedicated
// publish worker runtime.
func NewExecutionProcessor() Processor {
	return &executionProcessor{filesystem: NewFilesystemPublisher()}
}

// Process executes one publish run against the publisher resolved from the
// target kind.
func (p *executionProcessor) Process(
	ctx context.Context,
	item WorkItem,
) (ExecutionResult, error) {
	switch item.Plan.PublishTargetKind {
	case KindFilesystem:
		return p.filesystem.Publish(ctx, item.Plan)
	default:
		return ExecutionResult{}, fmt.Errorf(
			"%w: publish target kind %q",
			ErrUnsupportedTargetKind,
			item.Plan.PublishTargetKind,
		)
	}
}