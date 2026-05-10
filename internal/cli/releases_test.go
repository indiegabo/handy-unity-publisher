package cli

import (
	"bytes"
	"context"
	"encoding/json"
	"path/filepath"
	"strings"
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

func TestReleaseDispatchManualCommand(t *testing.T) {
	dataDir := t.TempDir()
	redisServer := miniredis.RunT(t)
	t.Cleanup(redisServer.Close)
	redisClient := redisv9.NewClient(&redisv9.Options{Addr: redisServer.Addr()})
	t.Cleanup(func() { _ = redisClient.Close() })
	t.Setenv("DATA_DIR", dataDir)
	t.Setenv("APP_DB_PATH", filepath.Join(dataDir, "app.db"))
	t.Setenv("REDIS_ADDR", redisServer.Addr())
	seedReleaseRepository(t, dataDir, "dispatch-repo", "https://example.com/org/dispatch-repo.git")

	var stdout bytes.Buffer
	var stderr bytes.Buffer
	if exitCode := Run([]string{
		"releases",
		"dispatch",
		"manual",
		"--repository-id",
		"1",
		"--git-tag",
		"v1.0.0",
		"--git-commit",
		"deadbeef",
	}, &stdout, &stderr); exitCode != 0 {
		t.Fatalf("expected manual dispatch command to succeed, got exit code %d: %s", exitCode, stderr.String())
	}

	var created release.Record
	if err := json.Unmarshal(stdout.Bytes(), &created); err != nil {
		t.Fatalf("decode manual dispatch output: %v", err)
	}
	if created.TriggerSource != release.TriggerSourceManual {
		t.Fatalf("expected trigger source %q, got %q", release.TriggerSourceManual, created.TriggerSource)
	}
	if created.Status != release.StatusQueued {
		t.Fatalf("expected release status %q, got %q", release.StatusQueued, created.Status)
	}

	queue := workerredis.NewQueue(redisClient)
	payload, err := queue.Dequeue(context.Background(), release.QueueName, time.Millisecond)
	if err != nil {
		t.Fatalf("dequeue release job: %v", err)
	}
	if payload == nil {
		t.Fatal("expected queued release payload")
	}

	var job release.Job
	if err := json.Unmarshal(payload, &job); err != nil {
		t.Fatalf("decode queued release job: %v", err)
	}
	if job.ReleaseRunID != created.ID {
		t.Fatalf("expected job release run id %d, got %d", created.ID, job.ReleaseRunID)
	}
}

func TestReleaseDispatchManualRejectsDuplicateTag(t *testing.T) {
	dataDir := t.TempDir()
	redisServer := miniredis.RunT(t)
	t.Cleanup(redisServer.Close)
	t.Setenv("DATA_DIR", dataDir)
	t.Setenv("APP_DB_PATH", filepath.Join(dataDir, "app.db"))
	t.Setenv("REDIS_ADDR", redisServer.Addr())
	seedReleaseRepository(t, dataDir, "duplicate-dispatch", "https://example.com/org/duplicate-dispatch.git")

	var stdout bytes.Buffer
	var stderr bytes.Buffer
	args := []string{
		"releases",
		"dispatch",
		"manual",
		"--repository-id",
		"1",
		"--git-tag",
		"v2.0.0",
	}

	if exitCode := Run(args, &stdout, &stderr); exitCode != 0 {
		t.Fatalf("expected first manual dispatch to succeed, got exit code %d: %s", exitCode, stderr.String())
	}

	stdout.Reset()
	stderr.Reset()

	if exitCode := Run(args, &stdout, &stderr); exitCode == 0 {
		t.Fatalf("expected duplicate manual dispatch to fail")
	}
	if !strings.Contains(stderr.String(), release.ErrConflict.Error()) {
		t.Fatalf("expected conflict error in stderr, got %q", stderr.String())
	}
}

func seedReleaseRepository(t *testing.T, dataDir string, name string, repoURL string) {
	t.Helper()

	database, err := db.Open(context.Background(), config.Config{
		DataDir:      dataDir,
		DatabasePath: filepath.Join(dataDir, "app.db"),
	})
	if err != nil {
		t.Fatalf("db.Open() error = %v", err)
	}
	defer database.Close()

	enabled := true
	if _, err := repository.NewStore(database).Create(context.Background(), repository.CreateInput{
		Name:                   name,
		RepoURL:                repoURL,
		PollingIntervalSeconds: 300,
		Enabled:                &enabled,
	}); err != nil {
		t.Fatalf("repository.Create() error = %v", err)
	}
}
