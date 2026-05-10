package build

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestExecutionProcessorPreparesWorkspaceExecutesAndWritesLog(t *testing.T) {
	t.Parallel()

	logPath := filepath.Join(t.TempDir(), "build.log")
	preparer := &workspacePreparerStub{prepared: PreparedWorkspace{
		RootPath:             "/data/workspaces/build-run-9",
		SourcePath:           "/data/workspaces/build-run-9/source",
		HostRootPath:         "/host/data/workspaces/build-run-9",
		HostSourcePath:       "/host/data/workspaces/build-run-9/source",
		LogPath:              logPath,
		ArtifactRootPath:     "/data/artifacts/revolutions.v1.0.0",
		HostArtifactRootPath: "/host/data/artifacts/revolutions.v1.0.0",
	}}
	executor := &executorStub{output: []byte("build succeeded\n")}
	processor := NewExecutionProcessor(
		&executionPlannerStub{plan: ExecutionPlan{
			BuildRunID:     9,
			RepositoryName: "revolutions",
			RepositoryURL:  "file:///tmp/repo.git",
			GitTag:         "v1.0.0",
			TargetName:     "webgl",
			OutputKind:     plainStringPointer("archive"),
		}},
		preparer,
		executor,
	)

	result, err := processor.Process(context.Background(), WorkItem{Run: Run{ID: 9}})
	if err != nil {
		t.Fatalf("process build: %v", err)
	}

	if result.LogPath != logPath {
		t.Fatalf("expected log path %q, got %q", logPath, result.LogPath)
	}

	contents, err := os.ReadFile(logPath)
	if err != nil {
		t.Fatalf("read written log file: %v", err)
	}

	if string(contents) != "build succeeded\n" {
		t.Fatalf("expected log file contents %q, got %q", "build succeeded\n", string(contents))
	}

	if preparer.input.RepositoryName != "revolutions" {
		t.Fatalf("expected workspace preparer repository name %q, got %q", "revolutions", preparer.input.RepositoryName)
	}

	if executor.request.Plan.OutputPathTemplate == nil || *executor.request.Plan.OutputPathTemplate != "revolutions.v1.0.0.webgl.zip" {
		t.Fatalf("expected canonical output path template, got %#v", executor.request.Plan.OutputPathTemplate)
	}
}

func TestExecutionProcessorReturnsPathsWhenExecutorFails(t *testing.T) {
	t.Parallel()

	logPath := filepath.Join(t.TempDir(), "failed.log")
	preparer := &workspacePreparerStub{prepared: PreparedWorkspace{
		RootPath:             "/data/workspaces/build-run-10",
		SourcePath:           "/data/workspaces/build-run-10/source",
		HostRootPath:         "/host/data/workspaces/build-run-10",
		HostSourcePath:       "/host/data/workspaces/build-run-10/source",
		LogPath:              logPath,
		ArtifactRootPath:     "/data/artifacts/revolutions.v2.0.0",
		HostArtifactRootPath: "/host/data/artifacts/revolutions.v2.0.0",
	}}
	processor := NewExecutionProcessor(
		&executionPlannerStub{plan: ExecutionPlan{
			BuildRunID:     10,
			RepositoryName: "revolutions",
			RepositoryURL:  "file:///tmp/repo.git",
			GitTag:         "v2.0.0",
			TargetName:     "linux",
			OutputKind:     plainStringPointer("archive"),
		}},
		preparer,
		&executorStub{output: []byte("build failed\n"), err: errors.New("container exited 1")},
	)

	result, err := processor.Process(context.Background(), WorkItem{Run: Run{ID: 10}})
	if err == nil {
		t.Fatal("expected execution error")
	}

	if result.ArtifactRootPath != "/data/artifacts/revolutions.v2.0.0" {
		t.Fatalf("expected artifact root path to be preserved, got %q", result.ArtifactRootPath)
	}

	contents, readErr := os.ReadFile(logPath)
	if readErr != nil {
		t.Fatalf("read written failure log file: %v", readErr)
	}

	if string(contents) != "build failed\n" {
		t.Fatalf("expected failure log contents %q, got %q", "build failed\n", string(contents))
	}
}

func TestExecutionProcessorEnrichesFailureWithDetectedSummary(t *testing.T) {
	t.Parallel()

	logPath := filepath.Join(t.TempDir(), "license-failed.log")
	preparer := &workspacePreparerStub{prepared: PreparedWorkspace{
		RootPath:             "/data/workspaces/build-run-12",
		SourcePath:           "/data/workspaces/build-run-12/source",
		HostRootPath:         "/host/data/workspaces/build-run-12",
		HostSourcePath:       "/host/data/workspaces/build-run-12/source",
		LogPath:              logPath,
		ArtifactRootPath:     "/data/artifacts/revolutions.v2.1.0",
		HostArtifactRootPath: "/host/data/artifacts/revolutions.v2.1.0",
	}}
	processor := NewExecutionProcessor(
		&executionPlannerStub{plan: ExecutionPlan{
			BuildRunID:     12,
			RepositoryName: "revolutions",
			RepositoryURL:  "file:///tmp/repo.git",
			GitTag:         "v2.1.0",
			TargetName:     "windows",
			OutputKind:     plainStringPointer("archive"),
		}},
		preparer,
		&executorStub{
			output: []byte(
				"Unity Editor version: 6000.4.3f1\n" +
					"[Licensing::Module] Licensing is initialized (took 0.47s).\n" +
					"No valid Unity Editor license found. Please activate your license.\n",
			),
			err: errors.New("run GameCI build container: exit status 198"),
		},
	)

	_, err := processor.Process(context.Background(), WorkItem{Run: Run{ID: 12}})
	if err == nil {
		t.Fatal("expected execution error")
	}
	if !strings.Contains(err.Error(), "No valid Unity Editor license found. Please activate your license.") {
		t.Fatalf("expected enriched failure summary, got %v", err)
	}
}

func TestExecutionProcessorRemovesPreviousCanonicalOutputBeforeExecution(t *testing.T) {
	t.Parallel()

	artifactRoot := t.TempDir()
	staleDirectory := filepath.Join(artifactRoot, "revolutions.v3.0.0.webgl")
	if err := os.MkdirAll(staleDirectory, 0o755); err != nil {
		t.Fatalf("create stale artifact directory: %v", err)
	}
	if err := os.WriteFile(filepath.Join(staleDirectory, "stale.txt"), []byte("old"), 0o644); err != nil {
		t.Fatalf("write stale artifact file: %v", err)
	}

	logPath := filepath.Join(t.TempDir(), "cleanup.log")
	processor := NewExecutionProcessor(
		&executionPlannerStub{plan: ExecutionPlan{
			BuildRunID:         11,
			RepositoryName:     "revolutions",
			RepositoryURL:      "file:///tmp/repo.git",
			GitTag:             "v3.0.0",
			TargetName:         "webgl",
			OutputPathTemplate: plainStringPointer("Builds/WebGL"),
		}},
		&workspacePreparerStub{prepared: PreparedWorkspace{
			RootPath:             "/data/workspaces/build-run-11",
			SourcePath:           "/data/workspaces/build-run-11/source",
			HostRootPath:         "/host/data/workspaces/build-run-11",
			HostSourcePath:       "/host/data/workspaces/build-run-11/source",
			LogPath:              logPath,
			ArtifactRootPath:     artifactRoot,
			HostArtifactRootPath: artifactRoot,
		}},
		&executorStub{output: []byte("build succeeded\n")},
	)

	if _, err := processor.Process(context.Background(), WorkItem{Run: Run{ID: 11}}); err != nil {
		t.Fatalf("process build: %v", err)
	}

	if _, err := os.Stat(staleDirectory); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("expected stale artifact directory to be removed, got %v", err)
	}
}

type executionPlannerStub struct {
	plan ExecutionPlan
	err  error
}

func (s *executionPlannerStub) GetExecutionPlan(
	_ context.Context,
	_ int64,
) (ExecutionPlan, error) {
	if s.err != nil {
		return ExecutionPlan{}, s.err
	}

	return s.plan, nil
}

type workspacePreparerStub struct {
	prepared PreparedWorkspace
	err      error
	input    WorkspacePreparationInput
}

func (s *workspacePreparerStub) Prepare(
	_ context.Context,
	input WorkspacePreparationInput,
) (PreparedWorkspace, error) {
	s.input = input
	if s.err != nil {
		return PreparedWorkspace{}, s.err
	}

	return s.prepared, nil
}

type executorStub struct {
	output  []byte
	err     error
	request ExecuteRequest
}

func (s *executorStub) Execute(
	_ context.Context,
	request ExecuteRequest,
) ([]byte, error) {
	s.request = request
	return append([]byte(nil), s.output...), s.err
}
