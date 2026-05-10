package build

import (
	"context"
	"fmt"
	"os"
	"path/filepath"
	"strings"
)

// ExecuteRequest describes one prepared build execution handed to an executor.
type ExecuteRequest struct {
	Plan      ExecutionPlan
	Workspace PreparedWorkspace
}

// Executor runs one prepared build execution and returns the captured logs.
type Executor interface {
	Execute(ctx context.Context, request ExecuteRequest) ([]byte, error)
}

// executionPlanner loads the joined execution metadata for one build run.
type executionPlanner interface {
	GetExecutionPlan(ctx context.Context, buildRunID int64) (ExecutionPlan, error)
}

// preparedWorkspaceProvider materializes the isolated filesystem layout needed
// before the executor can run.
type preparedWorkspaceProvider interface {
	Prepare(ctx context.Context, input WorkspacePreparationInput) (PreparedWorkspace, error)
}

// ExecutionProcessor prepares one isolated workspace and delegates the actual
// build execution to an injected executor.
type ExecutionProcessor struct {
	plans    executionPlanner
	preparer preparedWorkspaceProvider
	executor Executor
}

// NewExecutionProcessor creates the default processor used by build workers.
func NewExecutionProcessor(
	plans executionPlanner,
	preparer preparedWorkspaceProvider,
	executor Executor,
) *ExecutionProcessor {
	return &ExecutionProcessor{
		plans:    plans,
		preparer: preparer,
		executor: executor,
	}
}

// Process prepares the build workspace, delegates execution, and writes the
// captured logs to the durable filesystem path.
func (p *ExecutionProcessor) Process(
	ctx context.Context,
	item WorkItem,
) (ExecutionResult, error) {
	if p.plans == nil {
		return ExecutionResult{}, fmt.Errorf("%w: execution plan store is required", ErrInvalid)
	}
	if p.preparer == nil {
		return ExecutionResult{}, fmt.Errorf("%w: workspace preparer is required", ErrInvalid)
	}
	if p.executor == nil {
		return ExecutionResult{}, fmt.Errorf("%w: build executor is required", ErrInvalid)
	}

	plan, err := p.plans.GetExecutionPlan(ctx, item.Run.ID)
	if err != nil {
		return ExecutionResult{}, fmt.Errorf(
			"load execution plan for build run %d: %w",
			item.Run.ID,
			err,
		)
	}

	plan.OutputPathTemplate = plainStringPointer(artifactOutputRelativePath(plan))

	prepared, err := p.preparer.Prepare(ctx, WorkspacePreparationInput{
		BuildRunID:              plan.BuildRunID,
		RepositoryName:          plan.RepositoryName,
		RepositoryURL:           plan.RepositoryURL,
		RepositoryCredentialsID: plan.RepositoryCredentialsID,
		GitTag:                  plan.GitTag,
	})
	if err != nil {
		return ExecutionResult{}, err
	}

	result := ExecutionResult{
		WorkspacePath:    prepared.RootPath,
		LogPath:          prepared.LogPath,
		ArtifactRootPath: prepared.ArtifactRootPath,
	}

	if err := cleanupPreviousArtifactOutput(prepared.ArtifactRootPath, plan.OutputPathTemplate); err != nil {
		return result, fmt.Errorf("prepare artifact output path: %w", err)
	}

	output, execErr := p.executor.Execute(ctx, ExecuteRequest{
		Plan:      plan,
		Workspace: prepared,
	})
	logErr := os.WriteFile(prepared.LogPath, output, 0o644)
	if logErr != nil {
		if execErr != nil {
			return result, fmt.Errorf(
				"%v; write build log %q: %w",
				execErr,
				prepared.LogPath,
				logErr,
			)
		}

		return result, fmt.Errorf(
			"write build log %q: %w",
			prepared.LogPath,
			logErr,
		)
	}

	if execErr != nil {
		return result, execErr
	}

	return result, nil
}

// cleanupPreviousArtifactOutput removes any stale canonical output path before
// a rerun writes new files into the artifact root.
func cleanupPreviousArtifactOutput(artifactRootPath string, outputPathTemplate *string) error {
	outputPath, err := resolveArtifactOutputPath(artifactRootPath, outputPathTemplate)
	if err != nil {
		return err
	}

	if err := os.RemoveAll(outputPath); err != nil {
		return fmt.Errorf("remove previous artifact output %q: %w", outputPath, err)
	}

	return nil
}

// resolveArtifactOutputPath turns the plan output template into an absolute
// filesystem path anchored inside the prepared artifact root.
func resolveArtifactOutputPath(artifactRootPath string, outputPathTemplate *string) (string, error) {
	rootPath := filepath.Clean(strings.TrimSpace(artifactRootPath))
	if rootPath == "" || rootPath == "." {
		return "", fmt.Errorf("%w: artifact root path must not be empty", ErrInvalid)
	}

	relativePath := filepath.Clean(strings.TrimSpace(pointerString(outputPathTemplate)))
	if relativePath == "" || relativePath == "." {
		return rootPath, nil
	}

	resolvedPath := filepath.Join(rootPath, filepath.FromSlash(relativePath))
	relativeToRoot, err := filepath.Rel(rootPath, resolvedPath)
	if err != nil {
		return "", fmt.Errorf("resolve artifact output path: %w", err)
	}
	if relativeToRoot == ".." || strings.HasPrefix(relativeToRoot, ".."+string(filepath.Separator)) {
		return "", fmt.Errorf("%w: artifact output path must stay within the artifact root", ErrInvalid)
	}

	return resolvedPath, nil
}
