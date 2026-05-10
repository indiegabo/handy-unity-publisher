package e2e_test

import (
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"strconv"
	"testing"
	"time"

	miniredis "github.com/alicebob/miniredis/v2"
	"github.com/indiegabo/handy-unity-bulder/internal/build"
	"github.com/indiegabo/handy-unity-bulder/internal/config"
	"github.com/indiegabo/handy-unity-bulder/internal/db"
	"github.com/indiegabo/handy-unity-bulder/internal/release"
	"github.com/indiegabo/handy-unity-bulder/internal/repository"
	workerredis "github.com/indiegabo/handy-unity-bulder/internal/worker/redis"
	redisv9 "github.com/redis/go-redis/v9"
)

func TestReleasePlanCLIEndToEndFlow(t *testing.T) {
	repoRoot := repositoryRoot(t)
	dataDir := t.TempDir()
	redisServer := miniredis.RunT(t)
	t.Cleanup(redisServer.Close)
	redisClient := redisv9.NewClient(&redisv9.Options{Addr: redisServer.Addr()})
	t.Cleanup(func() { _ = redisClient.Close() })
	repositoryPath := newUnityPlanningRepository(t, "2022.3.14f1", "v4.0.0")
	seedPlanningRepository(t, dataDir, repositoryPath)
	seedPlanningBuildTarget(t, dataDir, 1)

	var repoRecord repository.Record
	repoRecord.ID = 1

	var target build.Target
	target.ID = 1

	var releaseRecord release.Record
	runHGBAndDecodeWithEnv(
		t,
		repoRoot,
		dataDir,
		map[string]string{"REDIS_ADDR": redisServer.Addr()},
		&releaseRecord,
		"releases",
		"dispatch",
		"manual",
		"--repository-id",
		strconv.FormatInt(repoRecord.ID, 10),
		"--git-tag",
		"v4.0.0",
	)

	var firstPlan []build.Run
	runHGBAndDecodeWithEnv(
		t,
		repoRoot,
		dataDir,
		map[string]string{"REDIS_ADDR": redisServer.Addr()},
		&firstPlan,
		"releases",
		"plan",
		"--release-run-id",
		strconv.FormatInt(releaseRecord.ID, 10),
	)

	if len(firstPlan) != 1 {
		t.Fatalf("expected one planned build run, got %d", len(firstPlan))
	}

	if firstPlan[0].BuildTargetID != target.ID {
		t.Fatalf("expected build target id %d, got %d", target.ID, firstPlan[0].BuildTargetID)
	}

	if firstPlan[0].Status != build.StatusQueued {
		t.Fatalf("expected build run status %q, got %q", build.StatusQueued, firstPlan[0].Status)
	}

	if firstPlan[0].UnityVersion == nil || *firstPlan[0].UnityVersion != "2022.3.14f1" {
		t.Fatalf("expected build run unity version 2022.3.14f1, got %#v", firstPlan[0].UnityVersion)
	}

	if firstPlan[0].ImageRef == nil || *firstPlan[0].ImageRef != "unityci/editor:ubuntu-2022.3.14f1-base-3" {
		t.Fatalf("expected build run image unityci/editor:ubuntu-2022.3.14f1-base-3, got %#v", firstPlan[0].ImageRef)
	}

	queue := workerredis.NewQueue(redisClient)
	payload, err := queue.Dequeue(context.Background(), build.QueueName, time.Millisecond)
	if err != nil {
		t.Fatalf("dequeue build job: %v", err)
	}
	if payload == nil {
		t.Fatal("expected build job payload in redis queue")
	}

	var firstJob build.Job
	if err := json.Unmarshal(payload, &firstJob); err != nil {
		t.Fatalf("decode build job: %v", err)
	}

	if firstJob.BuildRunID != firstPlan[0].ID {
		t.Fatalf("expected job build run id %d, got %d", firstPlan[0].ID, firstJob.BuildRunID)
	}

	if firstJob.ImageRef != "unityci/editor:ubuntu-2022.3.14f1-base-3" {
		t.Fatalf("expected queued image ref unityci/editor:ubuntu-2022.3.14f1-base-3, got %q", firstJob.ImageRef)
	}

	var secondPlan []build.Run
	runHGBAndDecodeWithEnv(
		t,
		repoRoot,
		dataDir,
		map[string]string{"REDIS_ADDR": redisServer.Addr()},
		&secondPlan,
		"releases",
		"plan",
		"--release-run-id",
		strconv.FormatInt(releaseRecord.ID, 10),
	)

	if len(secondPlan) != 1 {
		t.Fatalf("expected one build run after repeated planning, got %d", len(secondPlan))
	}

	if secondPlan[0].ID != firstPlan[0].ID {
		t.Fatalf("expected repeated planning to reuse build run id %d, got %d", firstPlan[0].ID, secondPlan[0].ID)
	}

	if secondPlan[0].ImageRef == nil || *secondPlan[0].ImageRef != "unityci/editor:ubuntu-2022.3.14f1-base-3" {
		t.Fatalf("expected repeated planning to keep image ref, got %#v", secondPlan[0].ImageRef)
	}

	payload, err = queue.Dequeue(context.Background(), build.QueueName, time.Millisecond)
	if err != nil {
		t.Fatalf("dequeue repeated build job: %v", err)
	}
	if payload != nil {
		t.Fatalf("expected repeated planning not to enqueue duplicate build job, got %q", string(payload))
	}

	database, err := db.Open(context.Background(), config.Config{
		DataDir:      dataDir,
		DatabasePath: filepath.Join(dataDir, "app.db"),
	})
	if err != nil {
		t.Fatalf("open e2e database: %v", err)
	}
	defer database.Close()

	var unityVersion string
	if err := database.QueryRowContext(
		context.Background(),
		`SELECT unity_version FROM release_runs WHERE id = ?`,
		releaseRecord.ID,
	).Scan(&unityVersion); err != nil {
		t.Fatalf("query planned release unity version: %v", err)
	}

	if unityVersion != "2022.3.14f1" {
		t.Fatalf("expected persisted unity version 2022.3.14f1, got %q", unityVersion)
	}
}

func seedPlanningRepository(t *testing.T, dataDir string, repoURL string) {
	t.Helper()

	database, err := db.Open(context.Background(), config.Config{
		DataDir:      dataDir,
		DatabasePath: filepath.Join(dataDir, "app.db"),
	})
	if err != nil {
		t.Fatalf("open planning database: %v", err)
	}
	defer database.Close()

	enabled := true
	if _, err := repository.NewStore(database).Create(context.Background(), repository.CreateInput{
		Name:                   "e2e-build-plan-repo",
		RepoURL:                repoURL,
		PollingIntervalSeconds: 300,
		Enabled:                &enabled,
	}); err != nil {
		t.Fatalf("seed planning repository: %v", err)
	}
}

func seedPlanningBuildTarget(t *testing.T, dataDir string, repositoryID int64) {
	t.Helper()

	database, err := db.Open(context.Background(), config.Config{
		DataDir:      dataDir,
		DatabasePath: filepath.Join(dataDir, "app.db"),
	})
	if err != nil {
		t.Fatalf("open planning database: %v", err)
	}
	defer database.Close()

	enabled := true
	if _, err := build.NewStore(database).CreateTarget(context.Background(), build.CreateTargetInput{
		RepositoryID:   repositoryID,
		Name:           "linux-player",
		Platform:       "linux",
		BuildMethod:    "Builder.PerformLinux",
		RunnerType:     build.DefaultRunnerType,
		TimeoutSeconds: build.DefaultTimeoutSeconds,
		Enabled:        &enabled,
		ConfigJSON:     `{}`,
	}); err != nil {
		t.Fatalf("seed planning build target: %v", err)
	}
}

func newUnityPlanningRepository(
	t *testing.T,
	unityVersion string,
	gitTag string,
) string {
	t.Helper()

	repositoryPath := t.TempDir()
	runGit(t, repositoryPath, "init")
	runGit(t, repositoryPath, "config", "user.name", "E2E Planning Tests")
	runGit(t, repositoryPath, "config", "user.email", "e2e-planning@example.com")

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
