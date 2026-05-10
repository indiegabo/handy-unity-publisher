package publish_test

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
	"github.com/indiegabo/handy-unity-bulder/internal/publish"
	"github.com/indiegabo/handy-unity-bulder/internal/release"
	"github.com/indiegabo/handy-unity-bulder/internal/repository"
)

func TestStorePublishTargetCRUD(t *testing.T) {
	t.Parallel()

	ctx := context.Background()
	_, repositoryStore, _, publishStore := newTestStores(t)

	repo, err := repositoryStore.Create(ctx, repository.CreateInput{
		Name:    "publishable-repo",
		RepoURL: "https://example.com/org/publishable.git",
	})
	if err != nil {
		t.Fatalf("create repository: %v", err)
	}

	target, err := publishStore.CreateTarget(ctx, publish.CreateTargetInput{
		RepositoryID: repo.ID,
		Name:         "local-filesystem",
		Kind:         publish.KindFilesystem,
		ConfigJSON:   `{"root_path":"/exports"}`,
	})
	if err != nil {
		t.Fatalf("create publish target: %v", err)
	}

	if target.ID == 0 {
		t.Fatal("expected publish target id to be set")
	}

	loaded, err := publishStore.GetTarget(ctx, target.ID)
	if err != nil {
		t.Fatalf("get publish target: %v", err)
	}

	if loaded.Kind != publish.KindFilesystem {
		t.Fatalf("expected publish target kind %q, got %q", publish.KindFilesystem, loaded.Kind)
	}

	updated, err := publishStore.UpdateTarget(ctx, target.ID, publish.UpdateTargetInput{
		Name:       "local-artifacts",
		Kind:       publish.KindFilesystem,
		Enabled:    false,
		ConfigJSON: `{"root_path":"/exports/releases"}`,
	})
	if err != nil {
		t.Fatalf("update publish target: %v", err)
	}

	if updated.Enabled {
		t.Fatal("expected updated publish target to be disabled")
	}

	targets, err := publishStore.ListTargetsByRepository(ctx, repo.ID)
	if err != nil {
		t.Fatalf("list publish targets: %v", err)
	}

	if len(targets) != 1 {
		t.Fatalf("expected one publish target, got %d", len(targets))
	}

	if err := publishStore.DeleteTarget(ctx, target.ID); err != nil {
		t.Fatalf("delete publish target: %v", err)
	}

	_, err = publishStore.GetTarget(ctx, target.ID)
	if !errors.Is(err, publish.ErrNotFound) {
		t.Fatalf("expected publish target not found after delete, got %v", err)
	}
}

func TestStoreBindingCRUD(t *testing.T) {
	t.Parallel()

	ctx := context.Background()
	_, repositoryStore, buildStore, publishStore := newTestStores(t)

	repo, err := repositoryStore.Create(ctx, repository.CreateInput{
		Name:    "binding-repo",
		RepoURL: "https://example.com/org/binding.git",
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
		Name:         "filesystem-export",
		Kind:         publish.KindFilesystem,
	})
	if err != nil {
		t.Fatalf("create publish target: %v", err)
	}

	binding, err := publishStore.CreateBinding(ctx, publish.CreateBindingInput{
		BuildTargetID:   buildTarget.ID,
		PublishTargetID: publishTarget.ID,
		OptionsJSON:     `{"rename_template":"linux-{tag}.zip"}`,
	})
	if err != nil {
		t.Fatalf("create build publish binding: %v", err)
	}

	if binding.ID == 0 {
		t.Fatal("expected binding id to be set")
	}

	loaded, err := publishStore.GetBinding(ctx, binding.ID)
	if err != nil {
		t.Fatalf("get build publish binding: %v", err)
	}

	if loaded.PublishTargetID != publishTarget.ID {
		t.Fatalf("expected publish target id %d, got %d", publishTarget.ID, loaded.PublishTargetID)
	}

	updated, err := publishStore.UpdateBinding(ctx, binding.ID, publish.UpdateBindingInput{
		Enabled:     false,
		OptionsJSON: `{"rename_template":"linux-stable-{tag}.zip"}`,
	})
	if err != nil {
		t.Fatalf("update build publish binding: %v", err)
	}

	if updated.Enabled {
		t.Fatal("expected updated binding to be disabled")
	}

	bindings, err := publishStore.ListBindingsByBuildTarget(ctx, buildTarget.ID)
	if err != nil {
		t.Fatalf("list build publish bindings: %v", err)
	}

	if len(bindings) != 1 {
		t.Fatalf("expected one binding, got %d", len(bindings))
	}

	if err := publishStore.DeleteBinding(ctx, binding.ID); err != nil {
		t.Fatalf("delete build publish binding: %v", err)
	}

	_, err = publishStore.GetBinding(ctx, binding.ID)
	if !errors.Is(err, publish.ErrBindingNotFound) {
		t.Fatalf("expected binding not found after delete, got %v", err)
	}
}

func TestStoreBindingRejectsCrossRepositoryLinks(t *testing.T) {
	t.Parallel()

	ctx := context.Background()
	_, repositoryStore, buildStore, publishStore := newTestStores(t)

	repoA, err := repositoryStore.Create(ctx, repository.CreateInput{
		Name:    "repo-a",
		RepoURL: "https://example.com/org/repo-a.git",
	})
	if err != nil {
		t.Fatalf("create repository A: %v", err)
	}

	repoB, err := repositoryStore.Create(ctx, repository.CreateInput{
		Name:    "repo-b",
		RepoURL: "https://example.com/org/repo-b.git",
	})
	if err != nil {
		t.Fatalf("create repository B: %v", err)
	}

	buildTarget, err := buildStore.CreateTarget(ctx, build.CreateTargetInput{
		RepositoryID:   repoA.ID,
		Name:           "windows-player",
		Platform:       "windows",
		TimeoutSeconds: 3600,
	})
	if err != nil {
		t.Fatalf("create build target: %v", err)
	}

	publishTarget, err := publishStore.CreateTarget(ctx, publish.CreateTargetInput{
		RepositoryID: repoB.ID,
		Name:         "other-filesystem",
		Kind:         publish.KindFilesystem,
	})
	if err != nil {
		t.Fatalf("create publish target: %v", err)
	}

	_, err = publishStore.CreateBinding(ctx, publish.CreateBindingInput{
		BuildTargetID:   buildTarget.ID,
		PublishTargetID: publishTarget.ID,
	})
	if !errors.Is(err, publish.ErrRepositoryMismatch) {
		t.Fatalf("expected repository mismatch error, got %v", err)
	}
}

func TestStorePlanBuildRunCreatesQueuedPublishRunsIdempotently(t *testing.T) {
	t.Parallel()

	ctx := context.Background()
	database, repositoryStore, buildStore, publishStore := newTestStores(t)
	repositoryPath := newUnityTaggedRepository(t, "2022.3.14f1", "v1.2.3")

	repo, err := repositoryStore.Create(ctx, repository.CreateInput{
		Name:    "publish-plan-repo",
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
		Name:         "filesystem-default",
		Kind:         publish.KindFilesystem,
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

	artifactSize := int64(99)
	run := createQueuedBuildRun(t, ctx, database, buildStore, repo.ID)

	if _, err := buildStore.ReplaceArtifacts(ctx, run.ID, []build.CreateArtifactInput{{
		Name:      "Builds/linux-player.zip",
		Kind:      "archive",
		Path:      "Builds/linux-player.zip",
		SizeBytes: &artifactSize,
	}}); err != nil {
		t.Fatalf("register artifacts: %v", err)
	}

	if err := publishStore.PlanBuildRun(ctx, run.ID); err != nil {
		t.Fatalf("plan publish run: %v", err)
	}

	if err := publishStore.PlanBuildRun(ctx, run.ID); err != nil {
		t.Fatalf("replan publish run idempotently: %v", err)
	}

	publishRuns, err := publishStore.ListRunsByBuildRun(ctx, run.ID)
	if err != nil {
		t.Fatalf("list publish runs: %v", err)
	}

	if len(publishRuns) != 1 {
		t.Fatalf("expected one queued publish run, got %d", len(publishRuns))
	}

	if publishRuns[0].Status != publish.StatusQueued {
		t.Fatalf("expected queued publish run status, got %q", publishRuns[0].Status)
	}

	if publishRuns[0].PublishTargetID != publishTarget.ID {
		t.Fatalf("expected publish target id %d, got %d", publishTarget.ID, publishRuns[0].PublishTargetID)
	}

	if publishRuns[0].ArtifactID == nil {
		t.Fatal("expected planned publish run artifact id to be set")
	}
}

func createQueuedBuildRun(
	t *testing.T,
	ctx context.Context,
	database *sql.DB,
	buildStore build.Store,
	repositoryID int64,
) build.Run {
	t.Helper()

	releaseStore := release.NewStore(database)
	releaseRun, err := releaseStore.CreateManualDispatch(ctx, release.ManualDispatchInput{
		RepositoryID: repositoryID,
		GitTag:       "v1.2.3",
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

	if len(runs) != 1 {
		t.Fatalf("expected one build run, got %d", len(runs))
	}

	return runs[0]
}

func newTestStores(t *testing.T) (*sql.DB, repository.Store, build.Store, publish.Store) {
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

	return database, repository.NewStore(database), build.NewStore(database), publish.NewStore(database)
}

func newUnityTaggedRepository(
	t *testing.T,
	unityVersion string,
	gitTag string,
) string {
	t.Helper()

	repositoryPath := t.TempDir()
	runGit(t, repositoryPath, "init")
	runGit(t, repositoryPath, "config", "user.name", "Publish Store Tests")
	runGit(t, repositoryPath, "config", "user.email", "publish-store-tests@example.com")

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