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
		return result, enrichExecutionError(execErr, output)
	}

	return result, nil
}

// enrichExecutionError appends the most relevant failure detail recovered from
// the captured executor output so downstream persistence and worker logs expose
// the real Unity/GameCI cause instead of only the container exit status.
func enrichExecutionError(execErr error, output []byte) error {
	summary := summarizeExecutionFailure(output)
	if summary == "" {
		return execErr
	}
	if strings.Contains(
		strings.ToLower(execErr.Error()),
		strings.ToLower(summary),
	) {
		return execErr
	}

	return fmt.Errorf("%w: %s", execErr, summary)
}

// summarizeExecutionFailure scans captured build output for the highest-signal
// failure line so operators can see the likely root cause without opening the
// raw nested container log.
func summarizeExecutionFailure(output []byte) string {
	bestLine := ""
	bestScore := 0

	for _, rawLine := range strings.Split(string(output), "\n") {
		line := normalizeFailureSummaryLine(rawLine)
		if line == "" {
			continue
		}

		score := failureSummaryScore(line)
		if score == 0 {
			continue
		}
		if score >= bestScore {
			bestLine = line
			bestScore = score
		}
	}

	return bestLine
}

// normalizeFailureSummaryLine trims one raw log line into a compact operator
// summary shape and strips common timestamp prefixes emitted by container log
// surfaces.
func normalizeFailureSummaryLine(rawLine string) string {
	line := strings.TrimSpace(rawLine)
	if line == "" {
		return ""
	}

	if parts := strings.SplitN(line, "|", 2); len(parts) == 2 {
		left := strings.TrimSpace(parts[0])
		right := strings.TrimSpace(parts[1])
		if right != "" && strings.ContainsAny(left, ":-/.T") {
			line = right
		}
	}

	return strings.Join(strings.Fields(line), " ")
}

// failureSummaryScore ranks log lines by how likely they are to describe the
// actionable root cause of a failed build.
func failureSummaryScore(line string) int {
	lowerLine := strings.ToLower(strings.TrimSpace(line))

	switch {
	case strings.Contains(lowerLine, "no valid unity editor license found"):
		return 100
	case strings.Contains(lowerLine, "please activate your license"):
		return 95
	case strings.Contains(lowerLine, "unauthorizedaccessexception"):
		return 90
	case strings.Contains(lowerLine, "access to the path"):
		return 85
	case strings.Contains(lowerLine, "permission denied"):
		return 80
	case strings.Contains(lowerLine, "licensing initialization failed"):
		return 75
	case strings.Contains(lowerLine, "error:"):
		return 70
	case strings.Contains(lowerLine, "exception"):
		return 65
	case strings.Contains(lowerLine, "failed"):
		return 60
	default:
		return 0
	}
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
