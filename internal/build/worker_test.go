package build_test

import (
	"bytes"
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"log/slog"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"testing"
	"time"

	miniredis "github.com/alicebob/miniredis/v2"
	"github.com/indiegabo/handy-unity-bulder/internal/build"
	"github.com/indiegabo/handy-unity-bulder/internal/release"
	"github.com/indiegabo/handy-unity-bulder/internal/repository"
	workerredis "github.com/indiegabo/handy-unity-bulder/internal/worker/redis"
	redisv9 "github.com/redis/go-redis/v9"
)

func TestWorkerRunOnceCompletesQueuedBuildRun(t *testing.T) {
	t.Parallel()

	ctx := context.Background()
	database := newTestDatabase(t)
	redisServer := miniredis.RunT(t)
	redisClient := redisv9.NewClient(&redisv9.Options{Addr: redisServer.Addr()})
	t.Cleanup(func() { _ = redisClient.Close() })

	buildStore, runs := newQueuedBuildScenario(t, ctx, database, "v3.0.0")
	queue := workerredis.NewQueue(redisClient)
	dispatcher := build.NewDispatcher(queue).WithCoordination(
		workerredis.NewLockManager(redisClient),
		workerredis.NewIdempotencyStore(redisClient),
	)
	if err := dispatcher.DispatchMany(ctx, runs); err != nil {
		t.Fatalf("dispatch build runs: %v", err)
	}

	artifactRoot := t.TempDir()
	workspaceRoot := filepath.Join(t.TempDir(), "workspace-success")
	if err := os.MkdirAll(filepath.Join(workspaceRoot, "source"), 0o755); err != nil {
		t.Fatalf("create workspace directory: %v", err)
	}
	planner := &recordingPublishPlanner{}

	called := 0
	worker := build.NewWorker(
		buildStore,
		queue,
		build.ProcessorFunc(func(
			_ context.Context,
			item build.WorkItem,
		) (build.ExecutionResult, error) {
			called++
			if item.Job.BuildRunID != runs[0].ID {
				t.Fatalf("expected job build run id %d, got %d", runs[0].ID, item.Job.BuildRunID)
			}

			artifactPath := filepath.Join(artifactRoot, "Builds", "linux-player.zip")
			if err := os.MkdirAll(filepath.Dir(artifactPath), 0o755); err != nil {
				t.Fatalf("create artifact directory: %v", err)
			}
			if err := os.WriteFile(artifactPath, []byte("artifact payload"), 0o644); err != nil {
				t.Fatalf("write artifact file: %v", err)
			}

			return build.ExecutionResult{
				WorkspacePath:    workspaceRoot,
				LogPath:          "/data/logs/build-success.log",
				ArtifactRootPath: artifactRoot,
			}, nil
		}),
	).WithDequeueWait(time.Millisecond).WithPublishPlanner(planner)

	processed, err := worker.RunOnce(ctx)
	if err != nil {
		t.Fatalf("run worker once: %v", err)
	}
	if !processed {
		t.Fatal("expected worker to process one queued build job")
	}
	if called != 1 {
		t.Fatalf("expected processor to be called once, got %d", called)
	}

	updated, err := buildStore.GetRun(ctx, runs[0].ID)
	if err != nil {
		t.Fatalf("get updated build run: %v", err)
	}

	if updated.Status != build.StatusSucceeded {
		t.Fatalf("expected succeeded status, got %q", updated.Status)
	}

	if updated.LogPath == nil || *updated.LogPath != "/data/logs/build-success.log" {
		t.Fatalf("expected persisted log path, got %#v", updated.LogPath)
	}

	if updated.WorkspacePath == nil || *updated.WorkspacePath != workspaceRoot {
		t.Fatalf("expected persisted workspace path %q, got %#v", workspaceRoot, updated.WorkspacePath)
	}

	if updated.ArtifactRootPath == nil || *updated.ArtifactRootPath != artifactRoot {
		t.Fatalf("expected persisted artifact root %q, got %#v", artifactRoot, updated.ArtifactRootPath)
	}

	if _, err := os.Stat(workspaceRoot); !os.IsNotExist(err) {
		t.Fatalf("expected workspace %q to be removed after success, stat err=%v", workspaceRoot, err)
	}

	artifacts, err := buildStore.ListArtifactsByBuildRun(ctx, runs[0].ID)
	if err != nil {
		t.Fatalf("list persisted artifacts: %v", err)
	}

	if len(artifacts) != 1 {
		t.Fatalf("expected one persisted artifact, got %d", len(artifacts))
	}

	if artifacts[0].Path != "Builds/linux-player.zip" {
		t.Fatalf("expected persisted artifact path %q, got %q", "Builds/linux-player.zip", artifacts[0].Path)
	}

	if len(planner.plannedBuildRunIDs) != 1 || planner.plannedBuildRunIDs[0] != runs[0].ID {
		t.Fatalf("expected publish planner to receive build run id %d, got %#v", runs[0].ID, planner.plannedBuildRunIDs)
	}
}

func TestWorkerRunOncePersistsFailedBuildRun(t *testing.T) {
	t.Parallel()

	ctx := context.Background()
	database := newTestDatabase(t)
	redisServer := miniredis.RunT(t)
	redisClient := redisv9.NewClient(&redisv9.Options{Addr: redisServer.Addr()})
	t.Cleanup(func() { _ = redisClient.Close() })

	buildStore, runs := newQueuedBuildScenario(t, ctx, database, "v3.0.1")
	queue := workerredis.NewQueue(redisClient)
	dispatcher := build.NewDispatcher(queue).WithCoordination(
		workerredis.NewLockManager(redisClient),
		workerredis.NewIdempotencyStore(redisClient),
	)
	if err := dispatcher.DispatchMany(ctx, runs); err != nil {
		t.Fatalf("dispatch build runs: %v", err)
	}

	workspaceRoot := filepath.Join(t.TempDir(), "workspace-failed")
	if err := os.MkdirAll(filepath.Join(workspaceRoot, "source"), 0o755); err != nil {
		t.Fatalf("create workspace directory: %v", err)
	}

	worker := build.NewWorker(
		buildStore,
		queue,
		build.ProcessorFunc(func(
			_ context.Context,
			item build.WorkItem,
		) (build.ExecutionResult, error) {
			return build.ExecutionResult{
				WorkspacePath:    workspaceRoot,
				LogPath:          "/data/logs/build-failed.log",
				ArtifactRootPath: "/data/artifacts/build-failed",
			}, errors.New("unity build failed")
		}),
	).WithDequeueWait(time.Millisecond)

	processed, err := worker.RunOnce(ctx)
	if err != nil {
		t.Fatalf("run worker once: %v", err)
	}
	if !processed {
		t.Fatal("expected worker to process one queued build job")
	}

	updated, err := buildStore.GetRun(ctx, runs[0].ID)
	if err != nil {
		t.Fatalf("get updated build run: %v", err)
	}

	if updated.Status != build.StatusFailed {
		t.Fatalf("expected failed status, got %q", updated.Status)
	}

	if updated.ErrorMessage == nil || *updated.ErrorMessage != "unity build failed" {
		t.Fatalf("expected persisted failure message, got %#v", updated.ErrorMessage)
	}

	if updated.WorkspacePath == nil || *updated.WorkspacePath != workspaceRoot {
		t.Fatalf("expected persisted workspace path %q, got %#v", workspaceRoot, updated.WorkspacePath)
	}

	if updated.LogPath == nil || *updated.LogPath != "/data/logs/build-failed.log" {
		t.Fatalf("expected persisted log path, got %#v", updated.LogPath)
	}

	if _, err := os.Stat(workspaceRoot); !os.IsNotExist(err) {
		t.Fatalf("expected workspace %q to be removed after failure, stat err=%v", workspaceRoot, err)
	}
}

func TestWorkerRunOnceLogsFailedBuildRun(t *testing.T) {
	t.Parallel()

	ctx := context.Background()
	database := newTestDatabase(t)
	redisServer := miniredis.RunT(t)
	redisClient := redisv9.NewClient(&redisv9.Options{Addr: redisServer.Addr()})
	t.Cleanup(func() { _ = redisClient.Close() })

	buildStore, runs := newQueuedBuildScenario(t, ctx, database, "v3.0.1-log")
	queue := workerredis.NewQueue(redisClient)
	dispatcher := build.NewDispatcher(queue).WithCoordination(
		workerredis.NewLockManager(redisClient),
		workerredis.NewIdempotencyStore(redisClient),
	)
	if err := dispatcher.DispatchMany(ctx, runs); err != nil {
		t.Fatalf("dispatch build runs: %v", err)
	}

	workspaceRoot := filepath.Join(t.TempDir(), "workspace-failed-log")
	if err := os.MkdirAll(filepath.Join(workspaceRoot, "source"), 0o755); err != nil {
		t.Fatalf("create workspace directory: %v", err)
	}

	var buffer bytes.Buffer
	logger := slog.New(slog.NewJSONHandler(&buffer, nil))
	worker := build.NewWorker(
		buildStore,
		queue,
		build.ProcessorFunc(func(
			_ context.Context,
			_ build.WorkItem,
		) (build.ExecutionResult, error) {
			return build.ExecutionResult{
				WorkspacePath:    workspaceRoot,
				LogPath:          "/data/logs/build-failed.log",
				ArtifactRootPath: "/data/artifacts/build-failed",
			}, errors.New("unity build failed")
		}),
	).WithDequeueWait(time.Millisecond).WithLogger(logger)

	processed, err := worker.RunOnce(ctx)
	if err != nil {
		t.Fatalf("run worker once: %v", err)
	}
	if !processed {
		t.Fatal("expected worker to process one queued build job")
	}

	lines := strings.Split(strings.TrimSpace(buffer.String()), "\n")
	if len(lines) != 1 {
		t.Fatalf("expected one log record, got %d in %q", len(lines), buffer.String())
	}

	var record map[string]any
	if err := json.Unmarshal([]byte(lines[0]), &record); err != nil {
		t.Fatalf("decode log record: %v", err)
	}

	if record["msg"] != "build run failed" {
		t.Fatalf("expected log message %q, got %#v", "build run failed", record["msg"])
	}
	if record["error"] != "unity build failed" {
		t.Fatalf("expected log error %q, got %#v", "unity build failed", record["error"])
	}
	if record["log_path"] != "/data/logs/build-failed.log" {
		t.Fatalf("expected log_path %q, got %#v", "/data/logs/build-failed.log", record["log_path"])
	}
	if gotID, ok := record["build_run_id"].(float64); !ok || int64(gotID) != runs[0].ID {
		t.Fatalf("expected build_run_id %d, got %#v", runs[0].ID, record["build_run_id"])
	}
}

func TestWorkerRunOnceFailsBuildWhenPublishPlanningFails(t *testing.T) {
	t.Parallel()

	ctx := context.Background()
	database := newTestDatabase(t)
	redisServer := miniredis.RunT(t)
	redisClient := redisv9.NewClient(&redisv9.Options{Addr: redisServer.Addr()})
	t.Cleanup(func() { _ = redisClient.Close() })

	buildStore, runs := newQueuedBuildScenario(t, ctx, database, "v3.0.2")
	queue := workerredis.NewQueue(redisClient)
	dispatcher := build.NewDispatcher(queue).WithCoordination(
		workerredis.NewLockManager(redisClient),
		workerredis.NewIdempotencyStore(redisClient),
	)
	if err := dispatcher.DispatchMany(ctx, runs); err != nil {
		t.Fatalf("dispatch build runs: %v", err)
	}

	artifactRoot := t.TempDir()
	artifactPath := filepath.Join(artifactRoot, "Builds", "linux-player.zip")
	if err := os.MkdirAll(filepath.Dir(artifactPath), 0o755); err != nil {
		t.Fatalf("create artifact directory: %v", err)
	}
	if err := os.WriteFile(artifactPath, []byte("artifact payload"), 0o644); err != nil {
		t.Fatalf("write artifact file: %v", err)
	}

	planner := &recordingPublishPlanner{planErr: errors.New("planner exploded")}
	worker := build.NewWorker(
		buildStore,
		queue,
		build.ProcessorFunc(func(
			_ context.Context,
			item build.WorkItem,
		) (build.ExecutionResult, error) {
			return build.ExecutionResult{
				WorkspacePath:    "/data/workspaces/" + strconv.FormatInt(item.Run.ID, 10),
				LogPath:          "/data/logs/build-success.log",
				ArtifactRootPath: artifactRoot,
			}, nil
		}),
	).WithDequeueWait(time.Millisecond).WithPublishPlanner(planner)

	processed, err := worker.RunOnce(ctx)
	if err != nil {
		t.Fatalf("run worker once: %v", err)
	}
	if !processed {
		t.Fatal("expected worker to process one queued build job")
	}

	updated, err := buildStore.GetRun(ctx, runs[0].ID)
	if err != nil {
		t.Fatalf("get updated build run: %v", err)
	}

	if updated.Status != build.StatusFailed {
		t.Fatalf("expected failed status after planner error, got %q", updated.Status)
	}

	if updated.ErrorMessage == nil || !strings.Contains(*updated.ErrorMessage, "planner exploded") {
		t.Fatalf("expected persisted planner failure message, got %#v", updated.ErrorMessage)
	}
	if len(planner.plannedBuildRunIDs) != 1 || planner.plannedBuildRunIDs[0] != runs[0].ID {
		t.Fatalf("expected planner to be called for build run id %d, got %#v", runs[0].ID, planner.plannedBuildRunIDs)
	}
}

func newQueuedBuildScenario(
	t *testing.T,
	ctx context.Context,
	database *sql.DB,
	gitTag string,
) (build.Store, []build.Run) {
	t.Helper()

	repositoryStore := repository.NewStore(database)
	releaseStore := release.NewStore(database)
	buildStore := build.NewStore(database)
	repositoryPath := newUnityTaggedRepository(t, "2022.3.14f1", gitTag)

	repo, err := repositoryStore.Create(ctx, repository.CreateInput{
		Name:    "worker-repo-" + gitTag,
		RepoURL: repositoryPath,
	})
	if err != nil {
		t.Fatalf("create repository: %v", err)
	}

	if _, err := buildStore.CreateTarget(ctx, build.CreateTargetInput{
		RepositoryID:   repo.ID,
		Name:           "linux-player",
		Platform:       "linux",
		TimeoutSeconds: 3600,
	}); err != nil {
		t.Fatalf("create build target: %v", err)
	}

	releaseRun, err := releaseStore.CreateManualDispatch(ctx, release.ManualDispatchInput{
		RepositoryID: repo.ID,
		GitTag:       gitTag,
	})
	if err != nil {
		t.Fatalf("create release run: %v", err)
	}

	if _, err := releaseStore.MarkQueued(ctx, releaseRun.ID); err != nil {
		t.Fatalf("mark release queued: %v", err)
	}

	runs, err := buildStore.PlanRelease(ctx, releaseRun.ID)
	if err != nil {
		t.Fatalf("plan release: %v", err)
	}

	return buildStore, runs
}

type recordingPublishPlanner struct {
	plannedBuildRunIDs []int64
	planErr            error
}

func (p *recordingPublishPlanner) PlanBuildRun(
	_ context.Context,
	buildRunID int64,
) error {
	p.plannedBuildRunIDs = append(p.plannedBuildRunIDs, buildRunID)
	return p.planErr
}
