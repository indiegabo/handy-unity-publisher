package automation

import (
	"context"
	"database/sql"
	"io"
	"log/slog"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"sync"
	"testing"
	"time"

	miniredis "github.com/alicebob/miniredis/v2"
	"github.com/indiegabo/handy-unity-bulder/internal/build"
	"github.com/indiegabo/handy-unity-bulder/internal/config"
	"github.com/indiegabo/handy-unity-bulder/internal/credentials"
	"github.com/indiegabo/handy-unity-bulder/internal/db"
	internalgit "github.com/indiegabo/handy-unity-bulder/internal/git"
	"github.com/indiegabo/handy-unity-bulder/internal/release"
	"github.com/indiegabo/handy-unity-bulder/internal/repository"
	"github.com/indiegabo/handy-unity-bulder/internal/worker"
	workerredis "github.com/indiegabo/handy-unity-bulder/internal/worker/redis"
	redisv9 "github.com/redis/go-redis/v9"
)

func TestCoordinatorSweepRepositoriesQueuesDetectedTagsSequentially(t *testing.T) {
	t.Parallel()

	ctx := context.Background()
	database := newAutomationTestDatabase(t)
	repoStore := repository.NewStore(database)
	buildStore := build.NewStore(database)
	credentialStore := credentials.NewStore(database)
	releaseStore := release.NewStore(database)
	queue := &queueStub{entries: make(map[string][][]byte)}
	repositoryPath := newUnityTaggedRepositoryWithTags(
		t,
		"2022.3.14f1",
		"v1.0.0",
		"v1.1.0",
		"v1.2.0",
	)

	repo, err := repoStore.Create(ctx, repository.CreateInput{
		Name:                   "revolutions",
		RepoURL:                repositoryPath,
		PollingIntervalSeconds: 300,
	})
	if err != nil {
		t.Fatalf("create repository: %v", err)
	}

	_, err = buildStore.CreateTarget(ctx, build.CreateTargetInput{
		RepositoryID:       repo.ID,
		Name:               "webgl-player",
		Platform:           "webgl",
		RunnerType:         build.DefaultRunnerType,
		BuildMethod:        "Builder.PerformWebGL",
		OutputKind:         "directory",
		OutputPathTemplate: "Builds/WebGL",
		TimeoutSeconds:     3600,
		ConfigJSON:         `{}`,
	})
	if err != nil {
		t.Fatalf("create build target: %v", err)
	}

	_, err = buildStore.CreateTarget(ctx, build.CreateTargetInput{
		RepositoryID:       repo.ID,
		Name:               "linux-player",
		Platform:           "linux",
		RunnerType:         build.DefaultRunnerType,
		BuildMethod:        "Builder.PerformLinux",
		OutputKind:         "archive",
		OutputPathTemplate: "Builds/linux-player",
		TimeoutSeconds:     3600,
		ConfigJSON:         `{}`,
	})
	if err != nil {
		t.Fatalf("create second build target: %v", err)
	}

	coordinator := NewCoordinator(
		testLogger(),
		repoStore,
		buildStore,
		credentialStore,
		releaseStore,
		release.NewDispatcher(releaseStore, queue),
		build.NewDispatcher(queue),
		queue,
		tagSourceStub{tags: []internalgit.Tag{{Name: "v1.0.0", Commit: "111"}, {Name: "v1.1.0", Commit: "222"}, {Name: "v1.2.0", Commit: "333"}}},
	)

	if err := coordinator.sweepRepositories(ctx, time.Now()); err != nil {
		t.Fatalf("sweepRepositories() error = %v", err)
	}

	queuedReleases, err := releaseStore.ListByStatus(ctx, release.StatusQueued)
	if err != nil {
		t.Fatalf("ListByStatus(queued) error = %v", err)
	}
	if len(queuedReleases) != 3 {
		t.Fatalf("expected three queued releases, got %d", len(queuedReleases))
	}
	if queuedReleases[0].GitTag != "v1.0.0" || queuedReleases[1].GitTag != "v1.1.0" || queuedReleases[2].GitTag != "v1.2.0" {
		t.Fatalf("expected queued tags v1.0.0, v1.1.0, v1.2.0, got %#v", queuedReleases)
	}

	runs, err := buildStore.ListBuildRunsByRelease(ctx, queuedReleases[0].ID)
	if err != nil {
		t.Fatalf("ListBuildRunsByRelease() error = %v", err)
	}
	if len(runs) != 2 {
		t.Fatalf("expected two build runs for the first queued release, got %d", len(runs))
	}
	for _, run := range runs {
		if run.Status != build.StatusQueued {
			t.Fatalf("expected build run status %q, got %q", build.StatusQueued, run.Status)
		}
	}

	secondRuns, err := buildStore.ListBuildRunsByRelease(ctx, queuedReleases[1].ID)
	if err != nil {
		t.Fatalf("ListBuildRunsByRelease(second) error = %v", err)
	}
	if len(secondRuns) != 0 {
		t.Fatalf("expected no planned build runs for second queued release yet, got %d", len(secondRuns))
	}

	thirdRuns, err := buildStore.ListBuildRunsByRelease(ctx, queuedReleases[2].ID)
	if err != nil {
		t.Fatalf("ListBuildRunsByRelease(third) error = %v", err)
	}
	if len(thirdRuns) != 0 {
		t.Fatalf("expected no planned build runs for third queued release yet, got %d", len(thirdRuns))
	}

	loadedRepo, err := repoStore.Get(ctx, repo.ID)
	if err != nil {
		t.Fatalf("Get(repository) error = %v", err)
	}
	if loadedRepo.LastSeenTag == nil || *loadedRepo.LastSeenTag != "v1.2.0" {
		t.Fatalf("expected last seen tag v1.2.0, got %#v", loadedRepo.LastSeenTag)
	}

	if len(queue.entries[release.QueueName]) != 3 {
		t.Fatalf("expected three queued release jobs, got %d", len(queue.entries[release.QueueName]))
	}
	if len(queue.entries[build.QueueName]) != 2 {
		t.Fatalf("expected two queued build jobs for the first release, got %d", len(queue.entries[build.QueueName]))
	}
}

func TestCoordinatorSweepRepositoriesPausesPollingWhileBuildsAreInProgress(t *testing.T) {
	t.Parallel()

	ctx := context.Background()
	database := newAutomationTestDatabase(t)
	repoStore := repository.NewStore(database)
	buildStore := build.NewStore(database)
	credentialStore := credentials.NewStore(database)
	releaseStore := release.NewStore(database)
	queue := &queueStub{entries: make(map[string][][]byte)}
	repositoryPath := newUnityTaggedRepositoryWithTags(t, "2022.3.14f1", "v1.0.0", "v1.1.0")

	repo, err := repoStore.Create(ctx, repository.CreateInput{
		Name:                   "revolutions",
		RepoURL:                repositoryPath,
		PollingIntervalSeconds: 300,
	})
	if err != nil {
		t.Fatalf("create repository: %v", err)
	}

	_, err = buildStore.CreateTarget(ctx, build.CreateTargetInput{
		RepositoryID:       repo.ID,
		Name:               "webgl-player",
		Platform:           "webgl",
		RunnerType:         build.DefaultRunnerType,
		BuildMethod:        "Builder.PerformWebGL",
		OutputKind:         "directory",
		OutputPathTemplate: "Builds/WebGL",
		TimeoutSeconds:     3600,
		ConfigJSON:         `{}`,
	})
	if err != nil {
		t.Fatalf("create build target: %v", err)
	}

	record, err := releaseStore.CreateRepositoryPollDispatch(
		ctx,
		release.RepositoryPollDispatchInput{
			RepositoryID: repo.ID,
			GitTag:       "v1.0.0",
			GitCommit:    "111",
			ObservedVia:  "test",
		},
	)
	if err != nil {
		t.Fatalf("create queued release: %v", err)
	}
	if _, err := releaseStore.MarkQueued(ctx, record.ID); err != nil {
		t.Fatalf("mark release queued: %v", err)
	}
	if _, err := buildStore.PlanRelease(ctx, record.ID); err != nil {
		t.Fatalf("plan release: %v", err)
	}

	tags := &countingTagSourceStub{tags: []internalgit.Tag{{Name: "v1.0.0", Commit: "111"}, {Name: "v1.1.0", Commit: "222"}}}
	coordinator := NewCoordinator(
		testLogger(),
		repoStore,
		buildStore,
		credentialStore,
		releaseStore,
		release.NewDispatcher(releaseStore, queue),
		build.NewDispatcher(queue),
		queue,
		tags,
	)

	if err := coordinator.sweepRepositories(ctx, time.Now()); err != nil {
		t.Fatalf("sweepRepositories() error = %v", err)
	}

	if tags.calls != 0 {
		t.Fatalf("expected no remote tag polling while builds are active, got %d calls", tags.calls)
	}

	queuedReleases, err := releaseStore.ListByStatus(ctx, release.StatusQueued)
	if err != nil {
		t.Fatalf("ListByStatus(queued) error = %v", err)
	}
	if len(queuedReleases) != 1 {
		t.Fatalf("expected one queued release to remain active, got %d", len(queuedReleases))
	}
}

func TestCoordinatorPollRepositoryTreatsBuildInProgressAsNonFatal(t *testing.T) {
	t.Parallel()

	ctx := context.Background()
	database := newAutomationTestDatabase(t)
	repoStore := repository.NewStore(database)
	buildStore := build.NewStore(database)
	credentialStore := credentials.NewStore(database)
	releaseStore := release.NewStore(database)
	queue := &queueStub{entries: make(map[string][][]byte)}
	repositoryPath := newUnityTaggedRepositoryWithTags(t, "2022.3.14f1", "v1.0.0", "v1.1.0")

	repo, err := repoStore.Create(ctx, repository.CreateInput{
		Name:                   "revolutions",
		RepoURL:                repositoryPath,
		PollingIntervalSeconds: 300,
	})
	if err != nil {
		t.Fatalf("create repository: %v", err)
	}

	target, err := buildStore.CreateTarget(ctx, build.CreateTargetInput{
		RepositoryID:       repo.ID,
		Name:               "webgl-player",
		Platform:           "webgl",
		RunnerType:         build.DefaultRunnerType,
		BuildMethod:        "Builder.PerformWebGL",
		OutputKind:         "directory",
		OutputPathTemplate: "Builds/WebGL",
		TimeoutSeconds:     3600,
		ConfigJSON:         `{}`,
	})
	if err != nil {
		t.Fatalf("create build target: %v", err)
	}

	record, err := releaseStore.CreateManualDispatch(ctx, release.ManualDispatchInput{
		RepositoryID: repo.ID,
		GitTag:       "v1.0.0",
	})
	if err != nil {
		t.Fatalf("create queued release: %v", err)
	}
	if _, err := releaseStore.MarkQueued(ctx, record.ID); err != nil {
		t.Fatalf("mark release queued: %v", err)
	}
	if _, err := database.ExecContext(
		ctx,
		`INSERT INTO build_runs (release_run_id, build_target_id, status) VALUES (?, ?, ?)`,
		record.ID,
		target.ID,
		build.StatusRunning,
	); err != nil {
		t.Fatalf("insert running build run: %v", err)
	}

	tags := &countingTagSourceStub{tags: []internalgit.Tag{{Name: "v1.1.0", Commit: "222"}}}
	coordinator := NewCoordinator(
		testLogger(),
		repoStore,
		buildStore,
		credentialStore,
		releaseStore,
		release.NewDispatcher(releaseStore, queue),
		build.NewDispatcher(queue),
		queue,
		tags,
	)

	queuedCount, err := coordinator.pollRepository(ctx, repo)
	if err != nil {
		t.Fatalf("pollRepository() error = %v", err)
	}
	if queuedCount != 0 {
		t.Fatalf("expected no queued releases while build work is active, got %d", queuedCount)
	}
	if len(queue.entries[release.QueueName]) != 0 {
		t.Fatalf("expected no queued release jobs, got %d", len(queue.entries[release.QueueName]))
	}

	loadedRepo, err := repoStore.Get(ctx, repo.ID)
	if err != nil {
		t.Fatalf("get repository: %v", err)
	}
	if loadedRepo.LastSeenTag != nil {
		t.Fatalf("expected repository last seen tag to remain unset, got %#v", loadedRepo.LastSeenTag)
	}
}

func TestCoordinatorSweepRepositoriesAdvancesNextQueuedReleaseAfterCurrentBuildsFinish(t *testing.T) {
	t.Parallel()

	ctx := context.Background()
	database := newAutomationTestDatabase(t)
	repoStore := repository.NewStore(database)
	buildStore := build.NewStore(database)
	credentialStore := credentials.NewStore(database)
	releaseStore := release.NewStore(database)
	queue := &queueStub{entries: make(map[string][][]byte)}
	repositoryPath := newUnityTaggedRepositoryWithTags(t, "2022.3.14f1", "v1.0.0", "v1.1.0")

	repo, err := repoStore.Create(ctx, repository.CreateInput{
		Name:                   "revolutions",
		RepoURL:                repositoryPath,
		PollingIntervalSeconds: 300,
	})
	if err != nil {
		t.Fatalf("create repository: %v", err)
	}

	_, err = buildStore.CreateTarget(ctx, build.CreateTargetInput{
		RepositoryID:       repo.ID,
		Name:               "webgl-player",
		Platform:           "webgl",
		RunnerType:         build.DefaultRunnerType,
		BuildMethod:        "Builder.PerformWebGL",
		OutputKind:         "directory",
		OutputPathTemplate: "Builds/WebGL",
		TimeoutSeconds:     3600,
		ConfigJSON:         `{}`,
	})
	if err != nil {
		t.Fatalf("create first build target: %v", err)
	}

	_, err = buildStore.CreateTarget(ctx, build.CreateTargetInput{
		RepositoryID:       repo.ID,
		Name:               "linux-player",
		Platform:           "linux",
		RunnerType:         build.DefaultRunnerType,
		BuildMethod:        "Builder.PerformLinux",
		OutputKind:         "archive",
		OutputPathTemplate: "Builds/linux-player",
		TimeoutSeconds:     3600,
		ConfigJSON:         `{}`,
	})
	if err != nil {
		t.Fatalf("create second build target: %v", err)
	}

	firstRelease, err := releaseStore.CreateRepositoryPollDispatch(
		ctx,
		release.RepositoryPollDispatchInput{
			RepositoryID: repo.ID,
			GitTag:       "v1.0.0",
			GitCommit:    "111",
			ObservedVia:  "test",
		},
	)
	if err != nil {
		t.Fatalf("create first queued release: %v", err)
	}
	if _, err := releaseStore.MarkQueued(ctx, firstRelease.ID); err != nil {
		t.Fatalf("mark first release queued: %v", err)
	}

	secondRelease, err := releaseStore.CreateRepositoryPollDispatch(
		ctx,
		release.RepositoryPollDispatchInput{
			RepositoryID: repo.ID,
			GitTag:       "v1.1.0",
			GitCommit:    "222",
			ObservedVia:  "test",
		},
	)
	if err != nil {
		t.Fatalf("create second queued release: %v", err)
	}
	if _, err := releaseStore.MarkQueued(ctx, secondRelease.ID); err != nil {
		t.Fatalf("mark second release queued: %v", err)
	}

	firstRuns, err := buildStore.PlanRelease(ctx, firstRelease.ID)
	if err != nil {
		t.Fatalf("plan first release: %v", err)
	}
	for index, run := range firstRuns {
		if _, err := buildStore.StartRun(ctx, run.ID, build.StartRunInput{
			WorkspacePath:    "/tmp/workspace",
			LogPath:          "/tmp/build.log",
			ArtifactRootPath: "/tmp/artifacts",
		}); err != nil {
			t.Fatalf("start build run %d: %v", run.ID, err)
		}

		if index == 0 {
			if _, err := buildStore.CompleteRun(ctx, run.ID, build.CompleteRunInput{
				WorkspacePath:    "/tmp/workspace",
				LogPath:          "/tmp/build.log",
				ArtifactRootPath: "/tmp/artifacts",
			}); err != nil {
				t.Fatalf("complete build run %d: %v", run.ID, err)
			}
			continue
		}

		if _, err := buildStore.FailRun(ctx, run.ID, build.FailRunInput{
			WorkspacePath:    "/tmp/workspace",
			LogPath:          "/tmp/build.log",
			ArtifactRootPath: "/tmp/artifacts",
			ErrorMessage:     "simulated failure",
		}); err != nil {
			t.Fatalf("fail build run %d: %v", run.ID, err)
		}
	}

	tags := &countingTagSourceStub{tags: []internalgit.Tag{{Name: "v1.0.0", Commit: "111"}, {Name: "v1.1.0", Commit: "222"}, {Name: "v1.2.0", Commit: "333"}}}
	coordinator := NewCoordinator(
		testLogger(),
		repoStore,
		buildStore,
		credentialStore,
		releaseStore,
		release.NewDispatcher(releaseStore, queue),
		build.NewDispatcher(queue),
		queue,
		tags,
	)

	if err := coordinator.sweepRepositories(ctx, time.Now()); err != nil {
		t.Fatalf("sweepRepositories() error = %v", err)
	}

	if tags.calls != 0 {
		t.Fatalf("expected polling to stay paused while repository backlog advances, got %d calls", tags.calls)
	}

	secondRuns, err := buildStore.ListBuildRunsByRelease(ctx, secondRelease.ID)
	if err != nil {
		t.Fatalf("ListBuildRunsByRelease(second) error = %v", err)
	}
	if len(secondRuns) != 2 {
		t.Fatalf("expected two build runs for the next queued release, got %d", len(secondRuns))
	}
	for _, run := range secondRuns {
		if run.Status != build.StatusQueued {
			t.Fatalf("expected next release build run status %q, got %q", build.StatusQueued, run.Status)
		}
	}
	if len(queue.entries[build.QueueName]) != 2 {
		t.Fatalf("expected two dispatched build jobs for the next queued release, got %d", len(queue.entries[build.QueueName]))
	}
}

func TestCoordinatorSnapshotReportsPausedReleaseBacklog(t *testing.T) {
	t.Parallel()

	ctx := context.Background()
	database := newAutomationTestDatabase(t)
	repoStore := repository.NewStore(database)
	buildStore := build.NewStore(database)
	credentialStore := credentials.NewStore(database)
	releaseStore := release.NewStore(database)
	queue := &queueStub{entries: make(map[string][][]byte)}
	repositoryPath := newUnityTaggedRepositoryWithTags(t, "2022.3.14f1", "v1.0.0", "v1.1.0")

	repo, err := repoStore.Create(ctx, repository.CreateInput{
		Name:                   "revolutions",
		RepoURL:                repositoryPath,
		PollingIntervalSeconds: 300,
	})
	if err != nil {
		t.Fatalf("create repository: %v", err)
	}

	for _, target := range []build.CreateTargetInput{
		{
			RepositoryID:       repo.ID,
			Name:               "webgl-player",
			Platform:           "webgl",
			RunnerType:         build.DefaultRunnerType,
			BuildMethod:        "Builder.PerformWebGL",
			OutputKind:         "directory",
			OutputPathTemplate: "Builds/WebGL",
			TimeoutSeconds:     3600,
			ConfigJSON:         `{}`,
		},
		{
			RepositoryID:       repo.ID,
			Name:               "linux-player",
			Platform:           "linux",
			RunnerType:         build.DefaultRunnerType,
			BuildMethod:        "Builder.PerformLinux",
			OutputKind:         "archive",
			OutputPathTemplate: "Builds/linux-player",
			TimeoutSeconds:     3600,
			ConfigJSON:         `{}`,
		},
	} {
		if _, err := buildStore.CreateTarget(ctx, target); err != nil {
			t.Fatalf("create build target %q: %v", target.Name, err)
		}
	}

	coordinator := NewCoordinator(
		testLogger(),
		repoStore,
		buildStore,
		credentialStore,
		releaseStore,
		release.NewDispatcher(releaseStore, queue),
		build.NewDispatcher(queue),
		queue,
		tagSourceStub{tags: []internalgit.Tag{{Name: "v1.0.0", Commit: "111"}, {Name: "v1.1.0", Commit: "222"}}},
	)

	if err := coordinator.sweepRepositories(ctx, time.Now()); err != nil {
		t.Fatalf("sweepRepositories() error = %v", err)
	}

	report, err := coordinator.Snapshot(ctx)
	if err != nil {
		t.Fatalf("Snapshot() error = %v", err)
	}
	if len(report.Repositories) != 1 {
		t.Fatalf("expected one repository in runtime report, got %d", len(report.Repositories))
	}

	status := report.Repositories[0]
	if status.PollState != PollStatePaused {
		t.Fatalf("expected poll state %q, got %q", PollStatePaused, status.PollState)
	}
	if status.Reason == nil || *status.Reason != PollStateReasonActiveReleaseBacklog {
		t.Fatalf("expected pause reason %q, got %#v", PollStateReasonActiveReleaseBacklog, status.Reason)
	}
	if status.PendingReleaseCount != 2 {
		t.Fatalf("expected two pending releases, got %d", status.PendingReleaseCount)
	}
	if len(status.ReleaseQueue) != 2 {
		t.Fatalf("expected two release queue entries, got %#v", status.ReleaseQueue)
	}
	if status.ReleaseQueue[0].GitTag != "v1.0.0" || status.ReleaseQueue[1].GitTag != "v1.1.0" {
		t.Fatalf("expected queued releases v1.0.0 then v1.1.0, got %#v", status.ReleaseQueue)
	}
	if !status.ReleaseQueue[0].Planned || !status.ReleaseQueue[0].BuildProcessActive {
		t.Fatalf("expected first queued release to be planned and active, got %#v", status.ReleaseQueue[0])
	}
	if status.ReleaseQueue[0].QueuedBuildRuns != 2 {
		t.Fatalf("expected two queued build runs for the active release, got %#v", status.ReleaseQueue[0])
	}
	if status.ReleaseQueue[1].Planned || status.ReleaseQueue[1].TotalBuildRuns != 0 {
		t.Fatalf("expected second queued release to remain unplanned, got %#v", status.ReleaseQueue[1])
	}
}

func TestCoordinatorSnapshotReportsScheduledRepositoryWhenIdle(t *testing.T) {
	t.Parallel()

	ctx := context.Background()
	database := newAutomationTestDatabase(t)
	repoStore := repository.NewStore(database)
	buildStore := build.NewStore(database)
	credentialStore := credentials.NewStore(database)
	releaseStore := release.NewStore(database)
	queue := &queueStub{entries: make(map[string][][]byte)}
	repositoryPath := newUnityTaggedRepositoryWithTags(t, "2022.3.14f1", "v1.0.0")

	repo, err := repoStore.Create(ctx, repository.CreateInput{
		Name:                   "revolutions",
		RepoURL:                repositoryPath,
		PollingIntervalSeconds: 300,
	})
	if err != nil {
		t.Fatalf("create repository: %v", err)
	}

	if _, err := buildStore.CreateTarget(ctx, build.CreateTargetInput{
		RepositoryID:       repo.ID,
		Name:               "webgl-player",
		Platform:           "webgl",
		RunnerType:         build.DefaultRunnerType,
		BuildMethod:        "Builder.PerformWebGL",
		OutputKind:         "directory",
		OutputPathTemplate: "Builds/WebGL",
		TimeoutSeconds:     3600,
		ConfigJSON:         `{}`,
	}); err != nil {
		t.Fatalf("create build target: %v", err)
	}

	coordinator := NewCoordinator(
		testLogger(),
		repoStore,
		buildStore,
		credentialStore,
		releaseStore,
		release.NewDispatcher(releaseStore, queue),
		build.NewDispatcher(queue),
		queue,
		tagSourceStub{tags: []internalgit.Tag{}},
	)

	if err := coordinator.sweepRepositories(ctx, time.Now()); err != nil {
		t.Fatalf("sweepRepositories() error = %v", err)
	}

	report, err := coordinator.Snapshot(ctx)
	if err != nil {
		t.Fatalf("Snapshot() error = %v", err)
	}
	if len(report.Repositories) != 1 {
		t.Fatalf("expected one repository in runtime report, got %d", len(report.Repositories))
	}

	status := report.Repositories[0]
	if status.PollState != PollStateScheduled {
		t.Fatalf("expected poll state %q, got %q", PollStateScheduled, status.PollState)
	}
	if status.NextPollAt == nil || *status.NextPollAt == "" {
		t.Fatalf("expected next poll deadline in runtime report, got %#v", status.NextPollAt)
	}
	if status.PendingReleaseCount != 0 {
		t.Fatalf("expected no pending releases, got %d", status.PendingReleaseCount)
	}
}

func TestCoordinatorRecoverQueuedReleasesPlansBuildRunsOnStartup(t *testing.T) {
	t.Parallel()

	ctx := context.Background()
	database := newAutomationTestDatabase(t)
	repoStore := repository.NewStore(database)
	buildStore := build.NewStore(database)
	credentialStore := credentials.NewStore(database)
	releaseStore := release.NewStore(database)
	queue := &queueStub{entries: make(map[string][][]byte)}
	repositoryPath := newUnityTaggedRepository(t, "2022.3.14f1", "v2.0.0")

	repo, err := repoStore.Create(ctx, repository.CreateInput{
		Name:                   "revolutions",
		RepoURL:                repositoryPath,
		PollingIntervalSeconds: 300,
	})
	if err != nil {
		t.Fatalf("create repository: %v", err)
	}

	_, err = buildStore.CreateTarget(ctx, build.CreateTargetInput{
		RepositoryID:       repo.ID,
		Name:               "linux-player",
		Platform:           "linux",
		RunnerType:         build.DefaultRunnerType,
		BuildMethod:        "Builder.PerformLinux",
		OutputKind:         "archive",
		OutputPathTemplate: "Builds/linux-player",
		TimeoutSeconds:     3600,
		ConfigJSON:         `{}`,
	})
	if err != nil {
		t.Fatalf("create build target: %v", err)
	}

	record, err := releaseStore.CreateRepositoryPollDispatch(
		ctx,
		release.RepositoryPollDispatchInput{
			RepositoryID: repo.ID,
			GitTag:       "v2.0.0",
			GitCommit:    "def456",
			ObservedVia:  "test",
		},
	)
	if err != nil {
		t.Fatalf("CreateRepositoryPollDispatch() error = %v", err)
	}
	if _, err := releaseStore.MarkQueued(ctx, record.ID); err != nil {
		t.Fatalf("MarkQueued() error = %v", err)
	}

	coordinator := NewCoordinator(
		testLogger(),
		repoStore,
		buildStore,
		credentialStore,
		releaseStore,
		release.NewDispatcher(releaseStore, queue),
		build.NewDispatcher(queue),
		queue,
		tagSourceStub{},
	)

	if err := coordinator.recoverQueuedReleases(ctx); err != nil {
		t.Fatalf("recoverQueuedReleases() error = %v", err)
	}

	runs, err := buildStore.ListBuildRunsByRelease(ctx, record.ID)
	if err != nil {
		t.Fatalf("ListBuildRunsByRelease() error = %v", err)
	}
	if len(runs) != 1 {
		t.Fatalf("expected one build run, got %d", len(runs))
	}
	if runs[0].Status != build.StatusQueued {
		t.Fatalf("expected build run status %q, got %q", build.StatusQueued, runs[0].Status)
	}
	if len(queue.entries[build.QueueName]) != 1 {
		t.Fatalf("expected one queued build job, got %d", len(queue.entries[build.QueueName]))
	}
}

func TestCoordinatorRunStartsReleasePlannerBeforeInitialSweep(t *testing.T) {
	t.Parallel()

	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()

	database := newAutomationTestDatabase(t)
	repoStore := repository.NewStore(database)
	buildStore := build.NewStore(database)
	credentialStore := credentials.NewStore(database)
	releaseStore := release.NewStore(database)
	repositoryPath := newUnityTaggedRepository(t, "2022.3.14f1", "v3.0.0")

	repo, err := repoStore.Create(ctx, repository.CreateInput{
		Name:                   "revolutions",
		RepoURL:                repositoryPath,
		PollingIntervalSeconds: 300,
	})
	if err != nil {
		t.Fatalf("create repository: %v", err)
	}

	if _, err := buildStore.CreateTarget(ctx, build.CreateTargetInput{
		RepositoryID:       repo.ID,
		Name:               "linux-player",
		Platform:           "linux",
		RunnerType:         build.DefaultRunnerType,
		BuildMethod:        "Builder.PerformLinux",
		OutputKind:         "archive",
		OutputPathTemplate: "Builds/linux-player",
		TimeoutSeconds:     3600,
		ConfigJSON:         `{}`,
	}); err != nil {
		t.Fatalf("create build target: %v", err)
	}

	redisServer := miniredis.RunT(t)
	redisClient := redisv9.NewClient(&redisv9.Options{Addr: redisServer.Addr()})
	t.Cleanup(func() { _ = redisClient.Close() })

	queue := workerredis.NewQueue(redisClient)
	releaseDispatcher := release.NewDispatcher(releaseStore, queue)
	buildDispatcher := build.NewDispatcher(queue)
	tagSource := &blockingTagSource{release: make(chan struct{})}

	coordinator := NewCoordinator(
		testLogger(),
		repoStore,
		buildStore,
		credentialStore,
		releaseStore,
		releaseDispatcher,
		buildDispatcher,
		queue,
		tagSource,
	)

	runDone := make(chan error, 1)
	go func() {
		runDone <- coordinator.Run(ctx)
	}()

	queuedRelease, err := releaseDispatcher.DispatchManual(ctx, release.ManualDispatchInput{
		RepositoryID: repo.ID,
		GitTag:       "v3.0.0",
		RequestedVia: "test",
	})
	if err != nil {
		close(tagSource.release)
		cancel()
		t.Fatalf("dispatch manual release: %v", err)
	}

	deadline := time.Now().Add(5 * time.Second)
	for {
		runs, err := buildStore.ListBuildRunsByRelease(ctx, queuedRelease.ID)
		if err != nil {
			close(tagSource.release)
			cancel()
			t.Fatalf("list build runs by release: %v", err)
		}
		if len(runs) > 0 {
			break
		}
		if time.Now().After(deadline) {
			close(tagSource.release)
			cancel()
			t.Fatal("expected planner loop to plan build runs before initial sweep completed")
		}
		time.Sleep(50 * time.Millisecond)
	}

	close(tagSource.release)
	cancel()

	select {
	case err := <-runDone:
		if err != nil {
			t.Fatalf("coordinator run returned error: %v", err)
		}
	case <-time.After(5 * time.Second):
		t.Fatal("expected coordinator run to stop after context cancellation")
	}
}

func TestCoordinatorAdvanceRepositoryReleaseQueueAvoidsDuplicatePlanning(
	t *testing.T,
) {
	t.Parallel()

	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()

	database := newAutomationTestDatabase(t)
	repoStore := repository.NewStore(database)
	credentialStore := credentials.NewStore(database)
	releaseStore := release.NewStore(database)
	buildStore := &blockingPlanBuildStore{
		planStarted: make(chan struct{}, 2),
		releasePlan: make(chan struct{}),
	}
	queue := &queueStub{entries: make(map[string][][]byte)}
	releaseDispatcher := release.NewDispatcher(releaseStore, queue)

	repo, err := repoStore.Create(ctx, repository.CreateInput{
		Name:    "revolutions",
		RepoURL: "https://example.com/org/revolutions.git",
	})
	if err != nil {
		t.Fatalf("create repository: %v", err)
	}

	if _, err := releaseDispatcher.DispatchManual(ctx, release.ManualDispatchInput{
		RepositoryID: repo.ID,
		GitTag:       "v1.0.0",
		RequestedVia: "test",
	}); err != nil {
		t.Fatalf("dispatch manual release: %v", err)
	}

	coordinator := NewCoordinator(
		testLogger(),
		repoStore,
		buildStore,
		credentialStore,
		releaseStore,
		releaseDispatcher,
		build.NewDispatcher(queue),
		queue,
		tagSourceStub{},
	).WithCoordination(newMemoryLockManager())

	type result struct {
		busy bool
		err  error
	}
	results := make(chan result, 2)

	for range 2 {
		go func() {
			busy, err := coordinator.advanceRepositoryReleaseQueue(ctx, repo.ID)
			results <- result{busy: busy, err: err}
		}()
	}

	select {
	case <-buildStore.planStarted:
	case <-time.After(5 * time.Second):
		t.Fatal("expected one planning attempt to start")
	}

	select {
	case <-buildStore.planStarted:
		t.Fatal("expected only one planning attempt while the first is locked")
	case <-time.After(200 * time.Millisecond):
	}

	close(buildStore.releasePlan)

	busyResults := make([]bool, 0, 2)
	for len(busyResults) < 2 {
		select {
		case result := <-results:
			if result.err != nil {
				t.Fatalf("advanceRepositoryReleaseQueue() error = %v", result.err)
			}
			busyResults = append(busyResults, result.busy)
		case <-time.After(5 * time.Second):
			t.Fatal("expected both planning attempts to complete")
		}
	}

	if buildStore.planCalls != 1 {
		t.Fatalf("expected one build planning call, got %d", buildStore.planCalls)
	}

	busyCount := 0
	for _, busy := range busyResults {
		if busy {
			busyCount++
		}
	}
	if busyCount != 1 {
		t.Fatalf("expected one busy result from lock contention, got %d", busyCount)
	}
}

type queueStub struct {
	entries map[string][][]byte
}

func (q *queueStub) Enqueue(_ context.Context, name string, payload []byte) error {
	q.entries[name] = append(q.entries[name], append([]byte(nil), payload...))
	return nil
}

func (q *queueStub) Dequeue(_ context.Context, _ string, _ time.Duration) ([]byte, error) {
	return nil, nil
}

type tagSourceStub struct {
	tags []internalgit.Tag
	err  error
}

func (s tagSourceStub) ListTags(
	_ context.Context,
	_ string,
	_ internalgit.AuthOptions,
) ([]internalgit.Tag, error) {
	if s.err != nil {
		return nil, s.err
	}

	return append([]internalgit.Tag(nil), s.tags...), nil
}

type blockingTagSource struct {
	once    sync.Once
	release chan struct{}
}

func (s *blockingTagSource) ListTags(
	ctx context.Context,
	_ string,
	_ internalgit.AuthOptions,
) ([]internalgit.Tag, error) {
	select {
	case <-ctx.Done():
		return nil, ctx.Err()
	case <-s.release:
		return []internalgit.Tag{}, nil
	}
}

type countingTagSourceStub struct {
	tags  []internalgit.Tag
	err   error
	calls int
}

func (s *countingTagSourceStub) ListTags(
	_ context.Context,
	_ string,
	_ internalgit.AuthOptions,
) ([]internalgit.Tag, error) {
	s.calls++
	if s.err != nil {
		return nil, s.err
	}

	return append([]internalgit.Tag(nil), s.tags...), nil
}

type blockingPlanBuildStore struct {
	mu          sync.Mutex
	planCalls   int
	planStarted chan struct{}
	releasePlan chan struct{}
}

func (s *blockingPlanBuildStore) CreateTarget(
	context.Context,
	build.CreateTargetInput,
) (build.Target, error) {
	panic("unexpected call to CreateTarget")
}

func (s *blockingPlanBuildStore) GetTarget(
	context.Context,
	int64,
) (build.Target, error) {
	panic("unexpected call to GetTarget")
}

func (s *blockingPlanBuildStore) ListTargetsByRepository(
	context.Context,
	int64,
) ([]build.Target, error) {
	panic("unexpected call to ListTargetsByRepository")
}

func (s *blockingPlanBuildStore) ListEnabledTargetsByRepository(
	context.Context,
	int64,
) ([]build.Target, error) {
	return nil, nil
}

func (s *blockingPlanBuildStore) UpdateTarget(
	context.Context,
	int64,
	build.UpdateTargetInput,
) (build.Target, error) {
	panic("unexpected call to UpdateTarget")
}

func (s *blockingPlanBuildStore) DeleteTarget(context.Context, int64) error {
	panic("unexpected call to DeleteTarget")
}

func (s *blockingPlanBuildStore) PlanRelease(
	ctx context.Context,
	releaseRunID int64,
) ([]build.Run, error) {
	s.mu.Lock()
	s.planCalls++
	s.mu.Unlock()

	select {
	case s.planStarted <- struct{}{}:
	default:
	}

	select {
	case <-ctx.Done():
		return nil, ctx.Err()
	case <-s.releasePlan:
	}

	return nil, nil
}

func (s *blockingPlanBuildStore) GetRun(context.Context, int64) (build.Run, error) {
	panic("unexpected call to GetRun")
}

func (s *blockingPlanBuildStore) GetExecutionPlan(
	context.Context,
	int64,
) (build.ExecutionPlan, error) {
	panic("unexpected call to GetExecutionPlan")
}

func (s *blockingPlanBuildStore) StartRun(
	context.Context,
	int64,
	build.StartRunInput,
) (build.Run, error) {
	panic("unexpected call to StartRun")
}

func (s *blockingPlanBuildStore) CompleteRun(
	context.Context,
	int64,
	build.CompleteRunInput,
) (build.Run, error) {
	panic("unexpected call to CompleteRun")
}

func (s *blockingPlanBuildStore) FailRun(
	context.Context,
	int64,
	build.FailRunInput,
) (build.Run, error) {
	panic("unexpected call to FailRun")
}

func (s *blockingPlanBuildStore) ReplaceArtifacts(
	context.Context,
	int64,
	[]build.CreateArtifactInput,
) ([]build.Artifact, error) {
	panic("unexpected call to ReplaceArtifacts")
}

func (s *blockingPlanBuildStore) ListArtifactsByBuildRun(
	context.Context,
	int64,
) ([]build.Artifact, error) {
	panic("unexpected call to ListArtifactsByBuildRun")
}

func (s *blockingPlanBuildStore) ListBuildRunsByRelease(
	context.Context,
	int64,
) ([]build.Run, error) {
	return nil, nil
}

type memoryLockManager struct {
	mu    sync.Mutex
	locks map[string]string
}

func newMemoryLockManager() *memoryLockManager {
	return &memoryLockManager{locks: make(map[string]string)}
}

func (m *memoryLockManager) Acquire(
	_ context.Context,
	name string,
	_ time.Duration,
) (worker.Lock, bool, error) {
	m.mu.Lock()
	defer m.mu.Unlock()

	if _, ok := m.locks[name]; ok {
		return nil, false, nil
	}

	m.locks[name] = name
	return &memoryLock{manager: m, name: name}, true, nil
}

type memoryLock struct {
	manager *memoryLockManager
	name    string
}

func (l *memoryLock) Key() string {
	return l.name
}

func (l *memoryLock) Token() string {
	return l.name
}

func (l *memoryLock) Release(_ context.Context) error {
	l.manager.mu.Lock()
	defer l.manager.mu.Unlock()

	delete(l.manager.locks, l.name)
	return nil
}

func newAutomationTestDatabase(t *testing.T) *sql.DB {
	t.Helper()

	dataDir := t.TempDir()
	cfg := config.Config{
		HTTPAddr:     ":0",
		DataDir:      dataDir,
		HostDataDir:  dataDir,
		DatabasePath: filepath.Join(dataDir, "app.db"),
	}

	database, err := db.Open(context.Background(), cfg)
	if err != nil {
		t.Fatalf("open test database: %v", err)
	}
	t.Cleanup(func() {
		if err := database.Close(); err != nil {
			t.Fatalf("close test database: %v", err)
		}
	})

	return database
}

func testLogger() *slog.Logger {
	return slog.New(slog.NewTextHandler(io.Discard, nil))
}

func newUnityTaggedRepository(
	t *testing.T,
	unityVersion string,
	gitTag string,
) string {
	t.Helper()

	return newUnityTaggedRepositoryWithTags(t, unityVersion, gitTag)
}

func newUnityTaggedRepositoryWithTags(
	t *testing.T,
	unityVersion string,
	gitTags ...string,
) string {
	t.Helper()

	repositoryPath := t.TempDir()
	runGit(t, repositoryPath, "init")
	runGit(t, repositoryPath, "config", "user.name", "Automation Tests")
	runGit(t, repositoryPath, "config", "user.email", "automation-tests@example.com")

	projectSettingsDir := filepath.Join(repositoryPath, "ProjectSettings")
	if err := os.MkdirAll(projectSettingsDir, 0o755); err != nil {
		t.Fatalf("create ProjectSettings directory: %v", err)
	}

	projectVersionPath := filepath.Join(projectSettingsDir, "ProjectVersion.txt")
	if err := os.WriteFile(
		projectVersionPath,
		[]byte(
			"m_EditorVersion: "+unityVersion+"\n"+
				"m_EditorVersionWithRevision: "+unityVersion+" (revision)\n",
		),
		0o644,
	); err != nil {
		t.Fatalf("write ProjectVersion.txt: %v", err)
	}

	runGit(t, repositoryPath, "add", ".")
	runGit(t, repositoryPath, "commit", "-m", "add unity project version")
	for _, gitTag := range gitTags {
		runGit(t, repositoryPath, "tag", gitTag)
	}

	return repositoryPath
}

func runGit(t *testing.T, repositoryPath string, args ...string) string {
	t.Helper()

	command := exec.Command("git", args...)
	command.Dir = repositoryPath
	output, err := command.CombinedOutput()
	if err != nil {
		t.Fatalf("run git %v: %v\n%s", args, err, string(output))
	}

	return strings.TrimSpace(string(output))
}
