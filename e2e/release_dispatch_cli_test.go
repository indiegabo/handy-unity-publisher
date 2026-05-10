package e2e_test

import (
	"context"
	"encoding/json"
	"path/filepath"
	"strconv"
	"testing"
	"time"

	miniredis "github.com/alicebob/miniredis/v2"
	"github.com/indiegabo/handy-unity-bulder/internal/config"
	"github.com/indiegabo/handy-unity-bulder/internal/db"
	"github.com/indiegabo/handy-unity-bulder/internal/release"
	"github.com/indiegabo/handy-unity-bulder/internal/repository"
	workerredis "github.com/indiegabo/handy-unity-bulder/internal/worker/redis"
	redisv9 "github.com/redis/go-redis/v9"
)

func TestReleaseDispatchCLIEndToEndFlow(t *testing.T) {
	repoRoot := repositoryRoot(t)
	dataDir := t.TempDir()
	redisServer := miniredis.RunT(t)
	t.Cleanup(redisServer.Close)
	redisClient := redisv9.NewClient(&redisv9.Options{Addr: redisServer.Addr()})
	t.Cleanup(func() { _ = redisClient.Close() })
	seedE2ERepository(t, dataDir, "e2e-dispatch-repo", "https://example.com/org/e2e-dispatch-repo.git")

	var repoRecord repository.Record
	repoRecord.ID = 1

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
		"v3.0.0",
		"--git-commit",
		"facefeed",
	)

	if releaseRecord.Status != release.StatusQueued {
		t.Fatalf("expected queued release status, got %q", releaseRecord.Status)
	}

	queue := workerredis.NewQueue(redisClient)
	payload, err := queue.Dequeue(context.Background(), release.QueueName, time.Millisecond)
	if err != nil {
		t.Fatalf("dequeue release job: %v", err)
	}
	if payload == nil {
		t.Fatal("expected release job payload in redis queue")
	}

	var job release.Job
	if err := json.Unmarshal(payload, &job); err != nil {
		t.Fatalf("decode release job: %v", err)
	}

	if job.ReleaseRunID != releaseRecord.ID {
		t.Fatalf("expected job release run id %d, got %d", releaseRecord.ID, job.ReleaseRunID)
	}

	if job.RepositoryID != repoRecord.ID {
		t.Fatalf("expected job repository id %d, got %d", repoRecord.ID, job.RepositoryID)
	}

	if releaseRecord.GitCommit == nil || *releaseRecord.GitCommit != "facefeed" {
		t.Fatalf("expected git commit facefeed, got %#v", releaseRecord.GitCommit)
	}

	if job.GitCommit == nil || *job.GitCommit != "facefeed" {
		t.Fatalf("expected queued job git commit facefeed, got %#v", job.GitCommit)
	}
}

func seedE2ERepository(t *testing.T, dataDir string, name string, repoURL string) {
	t.Helper()

	database, err := db.Open(context.Background(), config.Config{
		DataDir:      dataDir,
		DatabasePath: filepath.Join(dataDir, "app.db"),
	})
	if err != nil {
		t.Fatalf("open e2e database: %v", err)
	}
	defer database.Close()

	enabled := true
	if _, err := repository.NewStore(database).Create(context.Background(), repository.CreateInput{
		Name:                   name,
		RepoURL:                repoURL,
		PollingIntervalSeconds: 300,
		Enabled:                &enabled,
	}); err != nil {
		t.Fatalf("seed repository: %v", err)
	}
}
