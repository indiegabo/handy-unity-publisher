package publish_test

import (
	"context"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"testing"
	"time"

	miniredis "github.com/alicebob/miniredis/v2"
	"github.com/indiegabo/handy-unity-bulder/internal/build"
	"github.com/indiegabo/handy-unity-bulder/internal/publish"
	"github.com/indiegabo/handy-unity-bulder/internal/repository"
	redisv9 "github.com/redis/go-redis/v9"
	workerredis "github.com/indiegabo/handy-unity-bulder/internal/worker/redis"
)

func TestWorkerRunOncePublishesFilesystemArtifact(t *testing.T) {
	t.Parallel()

	ctx := context.Background()
	database, repositoryStore, buildStore, publishStore := newTestStores(t)
	redisServer := miniredis.RunT(t)
	redisClient := redisv9.NewClient(&redisv9.Options{Addr: redisServer.Addr()})
	t.Cleanup(func() { _ = redisClient.Close() })

	repositoryPath := newUnityTaggedRepository(t, "2022.3.14f1", "v1.2.3")
	repo, err := repositoryStore.Create(ctx, repository.CreateInput{
		Name:    "publish-worker-repo",
		RepoURL: repositoryPath,
	})
	if err != nil {
		t.Fatalf("create repository: %v", err)
	}

	buildTarget, err := buildStore.CreateTarget(ctx, build.CreateTargetInput{
		RepositoryID:   repo.ID,
		Name:           "linux-player",
		Platform:       "linux",
		TimeoutSeconds: 3600,
	})
	if err != nil {
		t.Fatalf("create build target: %v", err)
	}

	destinationRoot := t.TempDir()
	publishTarget, err := publishStore.CreateTarget(ctx, publish.CreateTargetInput{
		RepositoryID: repo.ID,
		Name:         "filesystem-release",
		Kind:         publish.KindFilesystem,
		ConfigJSON:   `{"root_path":` + strconv.Quote(destinationRoot) + `}`,
	})
	if err != nil {
		t.Fatalf("create publish target: %v", err)
	}

	if _, err := publishStore.CreateBinding(ctx, publish.CreateBindingInput{
		BuildTargetID:   buildTarget.ID,
		PublishTargetID: publishTarget.ID,
	}); err != nil {
		t.Fatalf("create binding: %v", err)
	}

	run := createQueuedBuildRun(t, ctx, database, buildStore, repo.ID)
	artifactRoot := t.TempDir()
	artifactPath := filepath.Join(artifactRoot, "Builds", "linux-player.zip")
	if err := os.MkdirAll(filepath.Dir(artifactPath), 0o755); err != nil {
		t.Fatalf("create artifact directory: %v", err)
	}
	if err := os.WriteFile(artifactPath, []byte("artifact payload"), 0o644); err != nil {
		t.Fatalf("write artifact file: %v", err)
	}

	artifactSize := int64(len("artifact payload"))
	if _, err := buildStore.ReplaceArtifacts(ctx, run.ID, []build.CreateArtifactInput{{
		Name:      "Builds/linux-player.zip",
		Kind:      "archive",
		Path:      "Builds/linux-player.zip",
		SizeBytes: &artifactSize,
	}}); err != nil {
		t.Fatalf("register artifact: %v", err)
	}

	if _, err := buildStore.StartRun(ctx, run.ID, build.StartRunInput{}); err != nil {
		t.Fatalf("start build run: %v", err)
	}

	if _, err := buildStore.CompleteRun(ctx, run.ID, build.CompleteRunInput{
		ArtifactRootPath: artifactRoot,
	}); err != nil {
		t.Fatalf("complete build run: %v", err)
	}

	coordinator := publish.NewBuildResultDispatcher(
		publishStore,
		publish.NewDispatcher(workerredis.NewQueue(redisClient)).WithCoordination(
			workerredis.NewLockManager(redisClient),
			workerredis.NewIdempotencyStore(redisClient),
		),
	)
	if err := coordinator.PlanBuildRun(ctx, run.ID); err != nil {
		t.Fatalf("plan and dispatch publish run: %v", err)
	}

	publishRuns, err := publishStore.ListRunsByBuildRun(ctx, run.ID)
	if err != nil {
		t.Fatalf("list publish runs: %v", err)
	}
	if len(publishRuns) != 1 {
		t.Fatalf("expected one publish run, got %d", len(publishRuns))
	}

	executionStore := publish.NewExecutionStore(database)
	worker := publish.NewWorker(
		executionStore,
		workerredis.NewQueue(redisClient),
		publish.NewExecutionProcessor(),
	).WithDequeueWait(time.Millisecond)

	processed, err := worker.RunOnce(ctx)
	if err != nil {
		t.Fatalf("run publish worker once: %v", err)
	}
	if !processed {
		t.Fatal("expected publish worker to process one queued job")
	}

	updated, err := executionStore.GetRun(ctx, publishRuns[0].ID)
	if err != nil {
		t.Fatalf("get updated publish run: %v", err)
	}
	if updated.Status != publish.StatusSucceeded {
		t.Fatalf("expected succeeded publish status, got %q", updated.Status)
	}
	if updated.DestinationRef == nil || *updated.DestinationRef == "" {
		t.Fatalf("expected destination ref to be recorded, got %#v", updated.DestinationRef)
	}

	expectedDestination := filepath.Join(
		destinationRoot,
		repo.Name,
		"v1.2.3",
		"Builds",
		"linux-player.zip",
	)
	if *updated.DestinationRef != expectedDestination {
		t.Fatalf("expected destination ref %q, got %q", expectedDestination, *updated.DestinationRef)
	}

	publishedPayload, err := os.ReadFile(expectedDestination)
	if err != nil {
		t.Fatalf("read published file: %v", err)
	}
	if string(publishedPayload) != "artifact payload" {
		t.Fatalf("expected published payload to match source, got %q", string(publishedPayload))
	}
}

func TestWorkerRunOnceFailsPublishRunWhenFilesystemConfigIsInvalid(t *testing.T) {
	t.Parallel()

	ctx := context.Background()
	database, repositoryStore, buildStore, publishStore := newTestStores(t)
	redisServer := miniredis.RunT(t)
	redisClient := redisv9.NewClient(&redisv9.Options{Addr: redisServer.Addr()})
	t.Cleanup(func() { _ = redisClient.Close() })

	repositoryPath := newUnityTaggedRepository(t, "2022.3.14f1", "v1.2.3")
	repo, err := repositoryStore.Create(ctx, repository.CreateInput{
		Name:    "publish-worker-invalid-config",
		RepoURL: repositoryPath,
	})
	if err != nil {
		t.Fatalf("create repository: %v", err)
	}

	buildTarget, err := buildStore.CreateTarget(ctx, build.CreateTargetInput{
		RepositoryID:   repo.ID,
		Name:           "linux-player",
		Platform:       "linux",
		TimeoutSeconds: 3600,
	})
	if err != nil {
		t.Fatalf("create build target: %v", err)
	}

	publishTarget, err := publishStore.CreateTarget(ctx, publish.CreateTargetInput{
		RepositoryID: repo.ID,
		Name:         "filesystem-release",
		Kind:         publish.KindFilesystem,
		ConfigJSON:   `{"root_path":"relative/path"}`,
	})
	if err != nil {
		t.Fatalf("create publish target: %v", err)
	}

	if _, err := publishStore.CreateBinding(ctx, publish.CreateBindingInput{
		BuildTargetID:   buildTarget.ID,
		PublishTargetID: publishTarget.ID,
	}); err != nil {
		t.Fatalf("create binding: %v", err)
	}

	run := createQueuedBuildRun(t, ctx, database, buildStore, repo.ID)
	artifactRoot := t.TempDir()
	artifactPath := filepath.Join(artifactRoot, "Builds", "linux-player.zip")
	if err := os.MkdirAll(filepath.Dir(artifactPath), 0o755); err != nil {
		t.Fatalf("create artifact directory: %v", err)
	}
	if err := os.WriteFile(artifactPath, []byte("artifact payload"), 0o644); err != nil {
		t.Fatalf("write artifact file: %v", err)
	}

	artifactSize := int64(len("artifact payload"))
	if _, err := buildStore.ReplaceArtifacts(ctx, run.ID, []build.CreateArtifactInput{{
		Name:      "Builds/linux-player.zip",
		Kind:      "archive",
		Path:      "Builds/linux-player.zip",
		SizeBytes: &artifactSize,
	}}); err != nil {
		t.Fatalf("register artifact: %v", err)
	}

	if _, err := buildStore.StartRun(ctx, run.ID, build.StartRunInput{}); err != nil {
		t.Fatalf("start build run: %v", err)
	}

	if _, err := buildStore.CompleteRun(ctx, run.ID, build.CompleteRunInput{
		ArtifactRootPath: artifactRoot,
	}); err != nil {
		t.Fatalf("complete build run: %v", err)
	}

	coordinator := publish.NewBuildResultDispatcher(
		publishStore,
		publish.NewDispatcher(workerredis.NewQueue(redisClient)).WithCoordination(
			workerredis.NewLockManager(redisClient),
			workerredis.NewIdempotencyStore(redisClient),
		),
	)
	if err := coordinator.PlanBuildRun(ctx, run.ID); err != nil {
		t.Fatalf("plan and dispatch publish run: %v", err)
	}

	publishRuns, err := publishStore.ListRunsByBuildRun(ctx, run.ID)
	if err != nil {
		t.Fatalf("list publish runs: %v", err)
	}
	if len(publishRuns) != 1 {
		t.Fatalf("expected one publish run, got %d", len(publishRuns))
	}

	executionStore := publish.NewExecutionStore(database)
	worker := publish.NewWorker(
		executionStore,
		workerredis.NewQueue(redisClient),
		publish.NewExecutionProcessor(),
	).WithDequeueWait(time.Millisecond)

	processed, err := worker.RunOnce(ctx)
	if err != nil {
		t.Fatalf("run publish worker once: %v", err)
	}
	if !processed {
		t.Fatal("expected publish worker to process one queued job")
	}

	updated, err := executionStore.GetRun(ctx, publishRuns[0].ID)
	if err != nil {
		t.Fatalf("get updated publish run: %v", err)
	}
	if updated.Status != publish.StatusFailed {
		t.Fatalf("expected failed publish status, got %q", updated.Status)
	}
	if updated.ErrorMessage == nil || !strings.Contains(*updated.ErrorMessage, "root_path must be absolute") {
		t.Fatalf("expected invalid root_path failure, got %#v", updated.ErrorMessage)
	}
}

func TestExecutionStoreLifecyclePersistsDestinationRef(t *testing.T) {
	t.Parallel()

	ctx := context.Background()
	database, repositoryStore, buildStore, publishStore := newTestStores(t)
	repositoryPath := newUnityTaggedRepository(t, "2022.3.14f1", "v1.2.3")

	repo, err := repositoryStore.Create(ctx, repository.CreateInput{
		Name:    "publish-store-lifecycle",
		RepoURL: repositoryPath,
	})
	if err != nil {
		t.Fatalf("create repository: %v", err)
	}

	buildTarget, err := buildStore.CreateTarget(ctx, build.CreateTargetInput{
		RepositoryID:   repo.ID,
		Name:           "linux-player",
		Platform:       "linux",
		TimeoutSeconds: 3600,
	})
	if err != nil {
		t.Fatalf("create build target: %v", err)
	}

	publishTarget, err := publishStore.CreateTarget(ctx, publish.CreateTargetInput{
		RepositoryID: repo.ID,
		Name:         "filesystem-release",
		Kind:         publish.KindFilesystem,
		ConfigJSON:   `{"root_path":"/data/published"}`,
	})
	if err != nil {
		t.Fatalf("create publish target: %v", err)
	}

	if _, err := publishStore.CreateBinding(ctx, publish.CreateBindingInput{
		BuildTargetID:   buildTarget.ID,
		PublishTargetID: publishTarget.ID,
	}); err != nil {
		t.Fatalf("create binding: %v", err)
	}

	run := createQueuedBuildRun(t, ctx, database, buildStore, repo.ID)
	artifactRoot := t.TempDir()
	artifactPath := filepath.Join(artifactRoot, "Builds", "linux-player.zip")
	if err := os.MkdirAll(filepath.Dir(artifactPath), 0o755); err != nil {
		t.Fatalf("create artifact directory: %v", err)
	}
	if err := os.WriteFile(artifactPath, []byte("artifact payload"), 0o644); err != nil {
		t.Fatalf("write artifact file: %v", err)
	}

	artifactSize := int64(len("artifact payload"))
	if _, err := buildStore.ReplaceArtifacts(ctx, run.ID, []build.CreateArtifactInput{{
		Name:      "Builds/linux-player.zip",
		Kind:      "archive",
		Path:      "Builds/linux-player.zip",
		SizeBytes: &artifactSize,
	}}); err != nil {
		t.Fatalf("register artifact: %v", err)
	}

	if _, err := buildStore.StartRun(ctx, run.ID, build.StartRunInput{}); err != nil {
		t.Fatalf("start build run: %v", err)
	}

	if _, err := buildStore.CompleteRun(ctx, run.ID, build.CompleteRunInput{
		ArtifactRootPath: artifactRoot,
	}); err != nil {
		t.Fatalf("complete build run: %v", err)
	}

	if err := publishStore.PlanBuildRun(ctx, run.ID); err != nil {
		t.Fatalf("plan publish run: %v", err)
	}

	publishRuns, err := publishStore.ListRunsByBuildRun(ctx, run.ID)
	if err != nil {
		t.Fatalf("list publish runs: %v", err)
	}
	if len(publishRuns) != 1 {
		t.Fatalf("expected one publish run, got %d", len(publishRuns))
	}

	executionStore := publish.NewExecutionStore(database)
	started, err := executionStore.StartRun(ctx, publishRuns[0].ID, publish.StartRunInput{})
	if err != nil {
		t.Fatalf("start publish run: %v", err)
	}
	if started.Status != publish.StatusRunning {
		t.Fatalf("expected running status, got %q", started.Status)
	}

	plan, err := executionStore.GetExecutionPlan(ctx, publishRuns[0].ID)
	if err != nil {
		t.Fatalf("get publish execution plan: %v", err)
	}
	if plan.SourcePath != artifactPath {
		t.Fatalf("expected source path %q, got %q", artifactPath, plan.SourcePath)
	}

	completed, err := executionStore.CompleteRun(ctx, publishRuns[0].ID, publish.CompleteRunInput{
		DestinationRef: "/data/published/linux-player.zip",
	})
	if err != nil {
		t.Fatalf("complete publish run: %v", err)
	}
	if completed.Status != publish.StatusSucceeded {
		t.Fatalf("expected succeeded status, got %q", completed.Status)
	}
	if completed.DestinationRef == nil || *completed.DestinationRef != "/data/published/linux-player.zip" {
		t.Fatalf("expected destination ref to persist, got %#v", completed.DestinationRef)
	}
}