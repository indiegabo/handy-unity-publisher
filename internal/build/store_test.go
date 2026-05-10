package build_test

import (
	"context"
	"database/sql"
	"errors"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"

	"github.com/indiegabo/handy-unity-bulder/internal/build"
	"github.com/indiegabo/handy-unity-bulder/internal/config"
	"github.com/indiegabo/handy-unity-bulder/internal/db"
	"github.com/indiegabo/handy-unity-bulder/internal/release"
	"github.com/indiegabo/handy-unity-bulder/internal/repository"
)

func TestStoreTargetCRUD(t *testing.T) {
	t.Parallel()

	ctx := context.Background()
	database := newTestDatabase(t)
	repositoryStore := repository.NewStore(database)
	buildStore := build.NewStore(database)

	repo, err := repositoryStore.Create(ctx, repository.CreateInput{
		Name:    "buildable-repo",
		RepoURL: "https://example.com/org/buildable.git",
	})
	if err != nil {
		t.Fatalf("create repository: %v", err)
	}

	target, err := buildStore.CreateTarget(ctx, build.CreateTargetInput{
		RepositoryID:       repo.ID,
		Name:               "linux-player",
		Platform:           "linux",
		RunnerType:         "gameci",
		BuildMethod:        "Builder.PerformLinux",
		OutputKind:         "directory",
		OutputPathTemplate: "Builds/Linux",
		TimeoutSeconds:     7200,
		ConfigJSON:         `{"compression":"lz4"}`,
	})
	if err != nil {
		t.Fatalf("create build target: %v", err)
	}

	if target.ID == 0 {
		t.Fatal("expected build target id to be set")
	}

	loaded, err := buildStore.GetTarget(ctx, target.ID)
	if err != nil {
		t.Fatalf("get build target: %v", err)
	}

	if loaded.Name != target.Name {
		t.Fatalf("expected build target name %q, got %q", target.Name, loaded.Name)
	}

	updated, err := buildStore.UpdateTarget(ctx, target.ID, build.UpdateTargetInput{
		Name:               "linux-player-v2",
		Platform:           "linux",
		RunnerType:         "gameci",
		BuildMethod:        "Builder.PerformLinuxV2",
		OutputKind:         "archive",
		OutputPathTemplate: "Builds/LinuxV2",
		TimeoutSeconds:     5400,
		Enabled:            false,
		ConfigJSON:         `{"compression":"zip"}`,
	})
	if err != nil {
		t.Fatalf("update build target: %v", err)
	}

	if updated.Name != "linux-player-v2" {
		t.Fatalf("expected updated build target name, got %q", updated.Name)
	}

	if updated.Enabled {
		t.Fatal("expected updated build target to be disabled")
	}

	targets, err := buildStore.ListTargetsByRepository(ctx, repo.ID)
	if err != nil {
		t.Fatalf("list build targets: %v", err)
	}

	if len(targets) != 1 {
		t.Fatalf("expected one build target, got %d", len(targets))
	}

	if err := buildStore.DeleteTarget(ctx, target.ID); err != nil {
		t.Fatalf("delete build target: %v", err)
	}

	_, err = buildStore.GetTarget(ctx, target.ID)
	if !errors.Is(err, build.ErrNotFound) {
		t.Fatalf("expected build target not found after delete, got %v", err)
	}
}

func TestStoreCreateTargetRejectsArchiveZipOutputPathTemplate(t *testing.T) {
	t.Parallel()

	ctx := context.Background()
	database := newTestDatabase(t)
	repositoryStore := repository.NewStore(database)
	buildStore := build.NewStore(database)

	repo, err := repositoryStore.Create(ctx, repository.CreateInput{
		Name:    "buildable-repo",
		RepoURL: "https://example.com/org/buildable.git",
	})
	if err != nil {
		t.Fatalf("create repository: %v", err)
	}

	_, err = buildStore.CreateTarget(ctx, build.CreateTargetInput{
		RepositoryID:       repo.ID,
		Name:               "webgl-player",
		Platform:           "webgl",
		RunnerType:         "gameci",
		BuildMethod:        "Builder.PerformWebGL",
		OutputKind:         "archive",
		OutputPathTemplate: "Builds/WebGL.zip",
		TimeoutSeconds:     3600,
	})
	if !errors.Is(err, build.ErrInvalid) {
		t.Fatalf("expected invalid build target error, got %v", err)
	}
	if !strings.Contains(err.Error(), "remove the .zip suffix") {
		t.Fatalf("expected archive zip suffix guidance, got %v", err)
	}
}

func TestStorePlanReleaseCreatesQueuedBuildRuns(t *testing.T) {
	t.Parallel()

	ctx := context.Background()
	database := newTestDatabase(t)
	repositoryStore := repository.NewStore(database)
	releaseStore := release.NewStore(database)
	buildStore := build.NewStore(database)
	repositoryPath := newUnityTaggedRepository(t, "2022.3.14f1", "v1.0.0")

	repo, err := repositoryStore.Create(ctx, repository.CreateInput{
		Name:    "planned-repo",
		RepoURL: repositoryPath,
	})
	if err != nil {
		t.Fatalf("create repository: %v", err)
	}

	if _, err := buildStore.CreateTarget(ctx, build.CreateTargetInput{
		RepositoryID:   repo.ID,
		Name:           "windows-player",
		Platform:       "windows",
		TimeoutSeconds: 3600,
	}); err != nil {
		t.Fatalf("create first build target: %v", err)
	}

	disabled := false
	if _, err := buildStore.CreateTarget(ctx, build.CreateTargetInput{
		RepositoryID:   repo.ID,
		Name:           "disabled-player",
		Platform:       "linux",
		TimeoutSeconds: 3600,
		Enabled:        &disabled,
	}); err != nil {
		t.Fatalf("create disabled build target: %v", err)
	}

	if _, err := buildStore.CreateTarget(ctx, build.CreateTargetInput{
		RepositoryID:   repo.ID,
		Name:           "webgl-player",
		Platform:       "webgl",
		TimeoutSeconds: 3600,
	}); err != nil {
		t.Fatalf("create second build target: %v", err)
	}

	releaseRun, err := releaseStore.CreateManualDispatch(ctx, release.ManualDispatchInput{
		RepositoryID: repo.ID,
		GitTag:       "v1.0.0",
	})
	if err != nil {
		t.Fatalf("create release run: %v", err)
	}

	releaseRun, err = releaseStore.MarkQueued(ctx, releaseRun.ID)
	if err != nil {
		t.Fatalf("mark release queued: %v", err)
	}

	runs, err := buildStore.PlanRelease(ctx, releaseRun.ID)
	if err != nil {
		t.Fatalf("plan release: %v", err)
	}

	if len(runs) != 2 {
		t.Fatalf("expected two queued build runs, got %d", len(runs))
	}

	for _, run := range runs {
		if run.Status != build.StatusQueued {
			t.Fatalf("expected build run status %q, got %q", build.StatusQueued, run.Status)
		}
	}

	if runs[0].UnityVersion == nil || *runs[0].UnityVersion != "2022.3.14f1" {
		t.Fatalf("expected build run unity version 2022.3.14f1, got %#v", runs[0].UnityVersion)
	}

	if runs[0].ImageRef == nil || *runs[0].ImageRef != "unityci/editor:ubuntu-2022.3.14f1-windows-mono-3" {
		t.Fatalf("expected windows image ref, got %#v", runs[0].ImageRef)
	}

	if runs[1].UnityVersion == nil || *runs[1].UnityVersion != "2022.3.14f1" {
		t.Fatalf("expected build run unity version 2022.3.14f1, got %#v", runs[1].UnityVersion)
	}

	if runs[1].ImageRef == nil || *runs[1].ImageRef != "unityci/editor:ubuntu-2022.3.14f1-webgl-3" {
		t.Fatalf("expected webgl image ref, got %#v", runs[1].ImageRef)
	}

	loadedRelease, err := releaseStore.Get(ctx, releaseRun.ID)
	if err != nil {
		t.Fatalf("get planned release: %v", err)
	}

	if loadedRelease.UnityVersion == nil || *loadedRelease.UnityVersion != "2022.3.14f1" {
		t.Fatalf("expected persisted unity version 2022.3.14f1, got %#v", loadedRelease.UnityVersion)
	}
}

func TestStorePlanReleaseIsIdempotent(t *testing.T) {
	t.Parallel()

	ctx := context.Background()
	database := newTestDatabase(t)
	repositoryStore := repository.NewStore(database)
	releaseStore := release.NewStore(database)
	buildStore := build.NewStore(database)
	repositoryPath := newUnityTaggedRepository(t, "2021.3.18f1", "v1.0.1")

	repo, err := repositoryStore.Create(ctx, repository.CreateInput{
		Name:    "idempotent-repo",
		RepoURL: repositoryPath,
	})
	if err != nil {
		t.Fatalf("create repository: %v", err)
	}

	target, err := buildStore.CreateTarget(ctx, build.CreateTargetInput{
		RepositoryID:         repo.ID,
		Name:                 "linux-player",
		Platform:             "linux",
		UnityVersionOverride: "2021.3.18f1",
		TimeoutSeconds:       3600,
	})
	if err != nil {
		t.Fatalf("create build target: %v", err)
	}

	releaseRun, err := releaseStore.CreateManualDispatch(ctx, release.ManualDispatchInput{
		RepositoryID: repo.ID,
		GitTag:       "v1.0.1",
	})
	if err != nil {
		t.Fatalf("create release run: %v", err)
	}

	if _, err := releaseStore.MarkQueued(ctx, releaseRun.ID); err != nil {
		t.Fatalf("mark release queued: %v", err)
	}

	firstRuns, err := buildStore.PlanRelease(ctx, releaseRun.ID)
	if err != nil {
		t.Fatalf("first plan release: %v", err)
	}

	secondRuns, err := buildStore.PlanRelease(ctx, releaseRun.ID)
	if err != nil {
		t.Fatalf("second plan release: %v", err)
	}

	if len(firstRuns) != 1 || len(secondRuns) != 1 {
		t.Fatalf("expected one build run from repeated planning, got %d and %d", len(firstRuns), len(secondRuns))
	}

	if firstRuns[0].BuildTargetID != target.ID || secondRuns[0].BuildTargetID != target.ID {
		t.Fatalf("expected planned build target id %d, got %d and %d", target.ID, firstRuns[0].BuildTargetID, secondRuns[0].BuildTargetID)
	}

	if firstRuns[0].ID != secondRuns[0].ID {
		t.Fatalf("expected same build run id across repeated planning, got %d and %d", firstRuns[0].ID, secondRuns[0].ID)
	}

	if firstRuns[0].UnityVersion == nil || *firstRuns[0].UnityVersion != "2021.3.18f1" {
		t.Fatalf("expected build run unity version 2021.3.18f1, got %#v", firstRuns[0].UnityVersion)
	}

	if firstRuns[0].ImageRef == nil || *firstRuns[0].ImageRef != "unityci/editor:ubuntu-2021.3.18f1-base-3" {
		t.Fatalf("expected linux base image ref, got %#v", firstRuns[0].ImageRef)
	}

	loadedRelease, err := releaseStore.Get(ctx, releaseRun.ID)
	if err != nil {
		t.Fatalf("get planned release: %v", err)
	}

	if loadedRelease.UnityVersion == nil || *loadedRelease.UnityVersion != "2021.3.18f1" {
		t.Fatalf("expected persisted unity version 2021.3.18f1, got %#v", loadedRelease.UnityVersion)
	}
}

func TestStorePlanReleaseRejectsReleaseThatIsNotQueued(t *testing.T) {
	t.Parallel()

	ctx := context.Background()
	database := newTestDatabase(t)
	repositoryStore := repository.NewStore(database)
	releaseStore := release.NewStore(database)
	buildStore := build.NewStore(database)

	repo, err := repositoryStore.Create(ctx, repository.CreateInput{
		Name:    "not-queued-repo",
		RepoURL: "https://example.com/org/not-queued.git",
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
		GitTag:       "v1.0.2",
	})
	if err != nil {
		t.Fatalf("create release run: %v", err)
	}

	_, err = buildStore.PlanRelease(ctx, releaseRun.ID)
	if !errors.Is(err, build.ErrReleaseNotQueued) {
		t.Fatalf("expected release not queued error, got %v", err)
	}
}

func TestStoreRunLifecycleTransitions(t *testing.T) {
	t.Parallel()

	ctx := context.Background()
	database := newTestDatabase(t)
	repositoryStore := repository.NewStore(database)
	releaseStore := release.NewStore(database)
	buildStore := build.NewStore(database)
	repositoryPath := newUnityTaggedRepository(t, "2022.3.14f1", "v2.0.0")

	repo, err := repositoryStore.Create(ctx, repository.CreateInput{
		Name:    "run-lifecycle-repo",
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
		GitTag:       "v2.0.0",
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

	running, err := buildStore.StartRun(ctx, runs[0].ID, build.StartRunInput{
		WorkspacePath:    "/data/workspaces/build-1",
		LogPath:          "/data/logs/build-1.log",
		ArtifactRootPath: "/data/artifacts/build-1",
	})
	if err != nil {
		t.Fatalf("start build run: %v", err)
	}

	if running.Status != build.StatusRunning {
		t.Fatalf("expected running status, got %q", running.Status)
	}

	if running.StartedAt == nil || *running.StartedAt == "" {
		t.Fatalf("expected started_at to be set, got %#v", running.StartedAt)
	}

	if running.LogPath == nil || *running.LogPath != "/data/logs/build-1.log" {
		t.Fatalf("expected log path to be persisted, got %#v", running.LogPath)
	}

	completed, err := buildStore.CompleteRun(ctx, runs[0].ID, build.CompleteRunInput{})
	if err != nil {
		t.Fatalf("complete build run: %v", err)
	}

	if completed.Status != build.StatusSucceeded {
		t.Fatalf("expected succeeded status, got %q", completed.Status)
	}

	if completed.FinishedAt == nil || *completed.FinishedAt == "" {
		t.Fatalf("expected finished_at to be set, got %#v", completed.FinishedAt)
	}

	_, err = buildStore.StartRun(ctx, runs[0].ID, build.StartRunInput{})
	if !errors.Is(err, build.ErrRunNotQueued) {
		t.Fatalf("expected run not queued error after completion, got %v", err)
	}
}

func TestStoreFailRunRequiresRunningState(t *testing.T) {
	t.Parallel()

	ctx := context.Background()
	database := newTestDatabase(t)
	repositoryStore := repository.NewStore(database)
	releaseStore := release.NewStore(database)
	buildStore := build.NewStore(database)
	repositoryPath := newUnityTaggedRepository(t, "2022.3.14f1", "v2.0.1")

	repo, err := repositoryStore.Create(ctx, repository.CreateInput{
		Name:    "failed-run-repo",
		RepoURL: repositoryPath,
	})
	if err != nil {
		t.Fatalf("create repository: %v", err)
	}

	if _, err := buildStore.CreateTarget(ctx, build.CreateTargetInput{
		RepositoryID:   repo.ID,
		Name:           "webgl-player",
		Platform:       "webgl",
		TimeoutSeconds: 3600,
	}); err != nil {
		t.Fatalf("create build target: %v", err)
	}

	releaseRun, err := releaseStore.CreateManualDispatch(ctx, release.ManualDispatchInput{
		RepositoryID: repo.ID,
		GitTag:       "v2.0.1",
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

	_, err = buildStore.FailRun(ctx, runs[0].ID, build.FailRunInput{ErrorMessage: "boom"})
	if !errors.Is(err, build.ErrRunNotRunning) {
		t.Fatalf("expected run not running error before claim, got %v", err)
	}

	if _, err := buildStore.StartRun(ctx, runs[0].ID, build.StartRunInput{}); err != nil {
		t.Fatalf("start build run: %v", err)
	}

	failed, err := buildStore.FailRun(ctx, runs[0].ID, build.FailRunInput{
		LogPath:      "/data/logs/build-failed.log",
		ErrorMessage: "unity build failed",
	})
	if err != nil {
		t.Fatalf("fail build run: %v", err)
	}

	if failed.Status != build.StatusFailed {
		t.Fatalf("expected failed status, got %q", failed.Status)
	}

	if failed.ErrorMessage == nil || *failed.ErrorMessage != "unity build failed" {
		t.Fatalf("expected error message to be persisted, got %#v", failed.ErrorMessage)
	}

	if failed.FinishedAt == nil || *failed.FinishedAt == "" {
		t.Fatalf("expected finished_at to be set, got %#v", failed.FinishedAt)
	}
}

func TestStoreGetExecutionPlanLoadsJoinedExecutionData(t *testing.T) {
	t.Parallel()

	ctx := context.Background()
	database := newTestDatabase(t)
	repositoryStore := repository.NewStore(database)
	releaseStore := release.NewStore(database)
	buildStore := build.NewStore(database)
	repositoryPath := newUnityTaggedRepository(t, "2022.3.14f1", "v7.0.0")

	repo, err := repositoryStore.Create(ctx, repository.CreateInput{
		Name:    "execution-plan-repo",
		RepoURL: "file://" + repositoryPath,
	})
	if err != nil {
		t.Fatalf("create repository: %v", err)
	}

	if _, err := buildStore.CreateTarget(ctx, build.CreateTargetInput{
		RepositoryID:       repo.ID,
		Name:               "linux-player",
		Platform:           "linux",
		BuildMethod:        "Builder.PerformLinux",
		OutputKind:         "directory",
		OutputPathTemplate: "Builds/Linux",
		TimeoutSeconds:     3600,
	}); err != nil {
		t.Fatalf("create build target: %v", err)
	}

	releaseRun, err := releaseStore.CreateManualDispatch(ctx, release.ManualDispatchInput{
		RepositoryID: repo.ID,
		GitTag:       "v7.0.0",
		GitCommit:    "abcdef12",
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

	plan, err := buildStore.GetExecutionPlan(ctx, runs[0].ID)
	if err != nil {
		t.Fatalf("get execution plan: %v", err)
	}

	if plan.RepositoryURL != "file://"+repositoryPath {
		t.Fatalf("expected repository url %q, got %q", "file://"+repositoryPath, plan.RepositoryURL)
	}

	if plan.RepositoryName != "execution-plan-repo" {
		t.Fatalf("expected repository name %q, got %q", "execution-plan-repo", plan.RepositoryName)
	}

	if plan.GitTag != "v7.0.0" {
		t.Fatalf("expected git tag v7.0.0, got %q", plan.GitTag)
	}

	if plan.BuildMethod == nil || *plan.BuildMethod != "Builder.PerformLinux" {
		t.Fatalf("expected build method Builder.PerformLinux, got %#v", plan.BuildMethod)
	}

	if plan.OutputPathTemplate == nil || *plan.OutputPathTemplate != "Builds/Linux" {
		t.Fatalf("expected output path template Builds/Linux, got %#v", plan.OutputPathTemplate)
	}

	if plan.ImageRef != "unityci/editor:ubuntu-2022.3.14f1-base-3" {
		t.Fatalf("expected image ref unityci/editor:ubuntu-2022.3.14f1-base-3, got %q", plan.ImageRef)
	}

	if plan.UnityVersion != "2022.3.14f1" {
		t.Fatalf("expected unity version 2022.3.14f1, got %q", plan.UnityVersion)
	}
}

func TestStoreReplaceArtifactsReplacesPreviousRows(t *testing.T) {
	t.Parallel()

	ctx := context.Background()
	database := newTestDatabase(t)
	repositoryStore := repository.NewStore(database)
	releaseStore := release.NewStore(database)
	buildStore := build.NewStore(database)
	repositoryPath := newUnityTaggedRepository(t, "2022.3.14f1", "v8.0.0")

	repo, err := repositoryStore.Create(ctx, repository.CreateInput{
		Name:    "artifact-repo",
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
		GitTag:       "v8.0.0",
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

	firstSize := int64(12)
	if _, err := buildStore.ReplaceArtifacts(ctx, runs[0].ID, []build.CreateArtifactInput{{
		Name:      "Builds/linux.zip",
		Kind:      "archive",
		Path:      "Builds/linux.zip",
		SizeBytes: &firstSize,
	}}); err != nil {
		t.Fatalf("register first artifact set: %v", err)
	}

	secondSize := int64(21)
	artifacts, err := buildStore.ReplaceArtifacts(ctx, runs[0].ID, []build.CreateArtifactInput{{
		Name:      "Builds/linux/player.txt",
		Kind:      "file",
		Path:      "Builds/linux/player.txt",
		SizeBytes: &secondSize,
	}})
	if err != nil {
		t.Fatalf("replace artifact set: %v", err)
	}

	if len(artifacts) != 1 {
		t.Fatalf("expected one replaced artifact, got %d", len(artifacts))
	}

	if artifacts[0].Path != "Builds/linux/player.txt" {
		t.Fatalf("expected replaced artifact path, got %q", artifacts[0].Path)
	}

	listed, err := buildStore.ListArtifactsByBuildRun(ctx, runs[0].ID)
	if err != nil {
		t.Fatalf("list build artifacts: %v", err)
	}

	if len(listed) != 1 {
		t.Fatalf("expected one listed artifact, got %d", len(listed))
	}

	if listed[0].SizeBytes == nil || *listed[0].SizeBytes != secondSize {
		t.Fatalf("expected persisted artifact size %d, got %#v", secondSize, listed[0].SizeBytes)
	}
}

func newTestDatabase(t *testing.T) *sql.DB {
	t.Helper()

	dataDir := t.TempDir()
	cfg := config.Config{
		DataDir:      dataDir,
		DatabasePath: filepath.Join(dataDir, "test.db"),
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

func newUnityTaggedRepository(
	t *testing.T,
	unityVersion string,
	gitTag string,
) string {
	t.Helper()

	repositoryPath := t.TempDir()
	runGit(t, repositoryPath, "init")
	runGit(t, repositoryPath, "config", "user.name", "Build Store Tests")
	runGit(t, repositoryPath, "config", "user.email", "build-store-tests@example.com")

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
	runGit(t, repositoryPath, "tag", gitTag)

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
