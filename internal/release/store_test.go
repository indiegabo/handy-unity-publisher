package release_test

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"path/filepath"
	"strings"
	"testing"

	"github.com/indiegabo/handy-unity-bulder/internal/build"
	"github.com/indiegabo/handy-unity-bulder/internal/config"
	"github.com/indiegabo/handy-unity-bulder/internal/db"
	"github.com/indiegabo/handy-unity-bulder/internal/publish"
	"github.com/indiegabo/handy-unity-bulder/internal/release"
	"github.com/indiegabo/handy-unity-bulder/internal/repository"
	"github.com/indiegabo/handy-unity-bulder/internal/trigger"
)

func TestStoreCreatesManualDispatch(t *testing.T) {
	t.Parallel()

	ctx := context.Background()
	database := newTestDatabase(t)
	repositoryStore := repository.NewStore(database)
	releaseStore := release.NewStore(database)

	repo, err := repositoryStore.Create(ctx, repository.CreateInput{
		Name:    "dispatchable",
		RepoURL: "https://example.com/org/dispatchable.git",
	})
	if err != nil {
		t.Fatalf("create repository: %v", err)
	}

	record, err := releaseStore.CreateManualDispatch(ctx, release.ManualDispatchInput{
		RepositoryID: repo.ID,
		GitTag:       "v1.2.3",
		GitCommit:    "abc123",
		RequestedVia: "cli",
	})
	if err != nil {
		t.Fatalf("create manual dispatch: %v", err)
	}

	if record.ID == 0 {
		t.Fatalf("expected release run id to be set")
	}

	if record.TriggerSource != release.TriggerSourceManual {
		t.Fatalf("expected trigger source %q, got %q", release.TriggerSourceManual, record.TriggerSource)
	}

	if record.Status != release.StatusDetected {
		t.Fatalf("expected release status %q, got %q", release.StatusDetected, record.Status)
	}

	var metadata map[string]string
	if err := json.Unmarshal([]byte(record.SourceMetadataJSON), &metadata); err != nil {
		t.Fatalf("decode release metadata: %v", err)
	}

	if metadata["requested_via"] != "cli" {
		t.Fatalf("expected requested_via metadata, got %#v", metadata)
	}

	loaded, err := releaseStore.Get(ctx, record.ID)
	if err != nil {
		t.Fatalf("get release run: %v", err)
	}

	if loaded.GitTag != record.GitTag {
		t.Fatalf("expected git tag %q, got %q", record.GitTag, loaded.GitTag)
	}
}

func TestStoreMarkQueuedTransitionsReleaseRun(t *testing.T) {
	t.Parallel()

	ctx := context.Background()
	database := newTestDatabase(t)
	repositoryStore := repository.NewStore(database)
	releaseStore := release.NewStore(database)

	repo, err := repositoryStore.Create(ctx, repository.CreateInput{
		Name:    "queue-transition",
		RepoURL: "https://example.com/org/queue-transition.git",
	})
	if err != nil {
		t.Fatalf("create repository: %v", err)
	}

	record, err := releaseStore.CreateManualDispatch(ctx, release.ManualDispatchInput{
		RepositoryID: repo.ID,
		GitTag:       "v1.2.4",
	})
	if err != nil {
		t.Fatalf("create manual dispatch: %v", err)
	}

	queued, err := releaseStore.MarkQueued(ctx, record.ID)
	if err != nil {
		t.Fatalf("mark queued: %v", err)
	}

	if queued.Status != release.StatusQueued {
		t.Fatalf("expected queued status, got %q", queued.Status)
	}
}

func TestStoreCreatesPollDispatch(t *testing.T) {
	t.Parallel()

	ctx := context.Background()
	database := newTestDatabase(t)
	repositoryStore := repository.NewStore(database)
	triggerStore := trigger.NewStore(database)
	releaseStore := release.NewStore(database)

	repo, err := repositoryStore.Create(ctx, repository.CreateInput{
		Name:    "polled-repo",
		RepoURL: "https://example.com/org/polled.git",
	})
	if err != nil {
		t.Fatalf("create repository: %v", err)
	}

	rule, err := triggerStore.Create(ctx, trigger.CreateInput{
		RepositoryID: repo.ID,
		Name:         "default-poll",
		Source:       trigger.SourcePoll,
	})
	if err != nil {
		t.Fatalf("create trigger rule: %v", err)
	}

	record, err := releaseStore.CreatePollDispatch(ctx, release.PollDispatchInput{
		RepositoryID:  repo.ID,
		TriggerRuleID: rule.ID,
		GitTag:        "v2.3.4",
		GitCommit:     "feedface",
		ObservedVia:   "poller",
	})
	if err != nil {
		t.Fatalf("create poll dispatch: %v", err)
	}

	if record.TriggerSource != release.TriggerSourcePoll {
		t.Fatalf("expected trigger source %q, got %q", release.TriggerSourcePoll, record.TriggerSource)
	}

	if record.TriggerRuleID == nil || *record.TriggerRuleID != rule.ID {
		t.Fatalf("expected trigger rule id %d, got %#v", rule.ID, record.TriggerRuleID)
	}

	var metadata map[string]string
	if err := json.Unmarshal([]byte(record.SourceMetadataJSON), &metadata); err != nil {
		t.Fatalf("decode release metadata: %v", err)
	}

	if metadata["observed_via"] != "poller" {
		t.Fatalf("expected observed_via metadata, got %#v", metadata)
	}
}

func TestStoreRejectsDuplicateManualDispatchByTag(t *testing.T) {
	t.Parallel()

	ctx := context.Background()
	database := newTestDatabase(t)
	repositoryStore := repository.NewStore(database)
	releaseStore := release.NewStore(database)

	repo, err := repositoryStore.Create(ctx, repository.CreateInput{
		Name:    "dupe-release",
		RepoURL: "https://example.com/org/dupe-release.git",
	})
	if err != nil {
		t.Fatalf("create repository: %v", err)
	}

	_, err = releaseStore.CreateManualDispatch(ctx, release.ManualDispatchInput{
		RepositoryID: repo.ID,
		GitTag:       "v2.0.0",
	})
	if err != nil {
		t.Fatalf("seed release run: %v", err)
	}

	_, err = releaseStore.CreateManualDispatch(ctx, release.ManualDispatchInput{
		RepositoryID: repo.ID,
		GitTag:       "v2.0.0",
	})
	if !errors.Is(err, release.ErrConflict) {
		t.Fatalf("expected conflict on duplicate manual dispatch, got %v", err)
	}
}

func TestStoreRejectsUnknownRepository(t *testing.T) {
	t.Parallel()

	ctx := context.Background()
	releaseStore := release.NewStore(newTestDatabase(t))

	_, err := releaseStore.CreateManualDispatch(ctx, release.ManualDispatchInput{
		RepositoryID: 999,
		GitTag:       "v0.1.0",
	})
	if !errors.Is(err, release.ErrRepositoryNotFound) {
		t.Fatalf("expected repository not found error, got %v", err)
	}
}

func TestStoreRejectsManualDispatchWhenRepositoryHasActiveBuildWork(t *testing.T) {
	t.Parallel()

	ctx := context.Background()
	database := newTestDatabase(t)
	repositoryStore := repository.NewStore(database)
	releaseStore := release.NewStore(database)
	buildStore := build.NewStore(database)

	repo, err := repositoryStore.Create(ctx, repository.CreateInput{
		Name:    "revolutions",
		RepoURL: "https://example.com/org/revolutions.git",
	})
	if err != nil {
		t.Fatalf("create repository: %v", err)
	}

	target, err := buildStore.CreateTarget(ctx, build.CreateTargetInput{
		RepositoryID:   repo.ID,
		Name:           "linux-player",
		Platform:       "linux",
		TimeoutSeconds: 3600,
	})
	if err != nil {
		t.Fatalf("create build target: %v", err)
	}

	firstRelease, err := releaseStore.CreateManualDispatch(ctx, release.ManualDispatchInput{
		RepositoryID: repo.ID,
		GitTag:       "v1.2.3",
	})
	if err != nil {
		t.Fatalf("create initial release: %v", err)
	}

	if _, err := releaseStore.MarkQueued(ctx, firstRelease.ID); err != nil {
		t.Fatalf("mark initial release queued: %v", err)
	}

	if _, err := database.ExecContext(
		ctx,
		`INSERT INTO build_runs (release_run_id, build_target_id, status) VALUES (?, ?, ?)`,
		firstRelease.ID,
		target.ID,
		build.StatusRunning,
	); err != nil {
		t.Fatalf("insert running build run: %v", err)
	}

	_, err = releaseStore.CreateManualDispatch(ctx, release.ManualDispatchInput{
		RepositoryID: repo.ID,
		GitTag:       "v1.2.4",
	})
	if !errors.Is(err, release.ErrBuildInProgress) {
		t.Fatalf("expected build in progress error, got %v", err)
	}
	if !strings.Contains(err.Error(), repo.Name) {
		t.Fatalf("expected repository name in error, got %q", err.Error())
	}
}

func TestStoreRejectsRepositoryPollDispatchWhenRepositoryHasActiveBuildWork(t *testing.T) {
	t.Parallel()

	ctx := context.Background()
	database := newTestDatabase(t)
	repositoryStore := repository.NewStore(database)
	releaseStore := release.NewStore(database)
	buildStore := build.NewStore(database)

	repo, err := repositoryStore.Create(ctx, repository.CreateInput{
		Name:    "poll-blocked-repo",
		RepoURL: "https://example.com/org/poll-blocked.git",
	})
	if err != nil {
		t.Fatalf("create repository: %v", err)
	}

	target, err := buildStore.CreateTarget(ctx, build.CreateTargetInput{
		RepositoryID:   repo.ID,
		Name:           "linux-player",
		Platform:       "linux",
		TimeoutSeconds: 3600,
	})
	if err != nil {
		t.Fatalf("create build target: %v", err)
	}

	firstRelease, err := releaseStore.CreateManualDispatch(ctx, release.ManualDispatchInput{
		RepositoryID: repo.ID,
		GitTag:       "v1.2.3",
	})
	if err != nil {
		t.Fatalf("create initial release: %v", err)
	}

	if _, err := releaseStore.MarkQueued(ctx, firstRelease.ID); err != nil {
		t.Fatalf("mark initial release queued: %v", err)
	}

	if _, err := database.ExecContext(
		ctx,
		`INSERT INTO build_runs (release_run_id, build_target_id, status) VALUES (?, ?, ?)`,
		firstRelease.ID,
		target.ID,
		build.StatusQueued,
	); err != nil {
		t.Fatalf("insert queued build run: %v", err)
	}

	_, err = releaseStore.CreateRepositoryPollDispatch(
		ctx,
		release.RepositoryPollDispatchInput{
			RepositoryID: repo.ID,
			GitTag:       "v1.2.4",
			GitCommit:    "222",
			ObservedVia:  "runtime-automation",
		},
	)
	if !errors.Is(err, release.ErrBuildInProgress) {
		t.Fatalf("expected build in progress error, got %v", err)
	}
	if !strings.Contains(err.Error(), repo.Name) {
		t.Fatalf("expected repository name in error, got %q", err.Error())
	}
}

func TestStoreRebuildManualDispatchResetsDerivedState(t *testing.T) {
	t.Parallel()

	ctx := context.Background()
	database := newTestDatabase(t)
	repositoryStore := repository.NewStore(database)
	releaseStore := release.NewStore(database)
	buildStore := build.NewStore(database)
	publishStore := publish.NewStore(database)

	repo, err := repositoryStore.Create(ctx, repository.CreateInput{
		Name:    "revolutions",
		RepoURL: "https://example.com/org/revolutions.git",
	})
	if err != nil {
		t.Fatalf("create repository: %v", err)
	}

	target, err := buildStore.CreateTarget(ctx, build.CreateTargetInput{
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
		Name:         "filesystem",
		Kind:         publish.KindFilesystem,
		ConfigJSON:   `{"root_path":"/tmp/releases"}`,
	})
	if err != nil {
		t.Fatalf("create publish target: %v", err)
	}

	record, err := releaseStore.CreateManualDispatch(ctx, release.ManualDispatchInput{
		RepositoryID: repo.ID,
		GitTag:       "v1.0.0",
		GitCommit:    "abc123",
		RequestedVia: "cli",
	})
	if err != nil {
		t.Fatalf("create initial release: %v", err)
	}

	if _, err := releaseStore.MarkQueued(ctx, record.ID); err != nil {
		t.Fatalf("mark release queued: %v", err)
	}

	if _, err := database.ExecContext(
		ctx,
		`UPDATE release_runs
		SET unity_version = ?, started_at = CURRENT_TIMESTAMP, finished_at = CURRENT_TIMESTAMP, error_message = ?, status = ?
		WHERE id = ?`,
		"2022.3.1f1",
		"old failure",
		"failed",
		record.ID,
	); err != nil {
		t.Fatalf("seed terminal release state: %v", err)
	}

	result, err := database.ExecContext(
		ctx,
		`INSERT INTO build_runs (release_run_id, build_target_id, status, workspace_path, log_path, artifact_root_path, finished_at, error_message)
		VALUES (?, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP, ?)`,
		record.ID,
		target.ID,
		build.StatusFailed,
		"/tmp/workspaces/build-run-1",
		"/tmp/logs/build-run-1.log",
		"/tmp/artifacts/revolutions.v1.0.0",
		"build failed",
	)
	if err != nil {
		t.Fatalf("insert build run: %v", err)
	}
	buildRunID, err := result.LastInsertId()
	if err != nil {
		t.Fatalf("read build run id: %v", err)
	}

	result, err = database.ExecContext(
		ctx,
		`INSERT INTO artifacts (build_run_id, name, kind, path)
		VALUES (?, ?, ?, ?)`,
		buildRunID,
		"linux-player",
		"archive",
		"revolutions.v1.0.0.linux-player.zip",
	)
	if err != nil {
		t.Fatalf("insert artifact: %v", err)
	}
	artifactID, err := result.LastInsertId()
	if err != nil {
		t.Fatalf("read artifact id: %v", err)
	}

	if _, err := database.ExecContext(
		ctx,
		`INSERT INTO publish_runs (release_run_id, build_run_id, publish_target_id, artifact_id, status, destination_ref, finished_at)
		VALUES (?, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP)`,
		record.ID,
		buildRunID,
		publishTarget.ID,
		artifactID,
		publish.StatusSucceeded,
		"/tmp/releases/revolutions/v1.0.0/revolutions.v1.0.0.linux-player.zip",
	); err != nil {
		t.Fatalf("insert publish run: %v", err)
	}

	rebuilt, err := releaseStore.RebuildManualDispatch(ctx, release.ManualDispatchInput{
		RepositoryID: repo.ID,
		GitTag:       "v1.0.0",
		GitCommit:    "def456",
		RequestedVia: "hub",
	})
	if err != nil {
		t.Fatalf("rebuild manual dispatch: %v", err)
	}

	if rebuilt.ID != record.ID {
		t.Fatalf("expected rebuild to reuse release id %d, got %d", record.ID, rebuilt.ID)
	}
	if rebuilt.Status != release.StatusDetected {
		t.Fatalf("expected detected status after rebuild reset, got %q", rebuilt.Status)
	}
	if rebuilt.GitCommit == nil || *rebuilt.GitCommit != "def456" {
		t.Fatalf("expected updated git commit, got %#v", rebuilt.GitCommit)
	}
	if rebuilt.TriggerSource != release.TriggerSourceManual {
		t.Fatalf("expected manual trigger source, got %q", rebuilt.TriggerSource)
	}
	if rebuilt.UnityVersion != nil {
		t.Fatalf("expected cleared unity version, got %#v", rebuilt.UnityVersion)
	}
	if rebuilt.StartedAt != nil || rebuilt.FinishedAt != nil || rebuilt.ErrorMessage != nil {
		t.Fatalf(
			"expected cleared terminal fields, got started=%#v finished=%#v error=%#v",
			rebuilt.StartedAt,
			rebuilt.FinishedAt,
			rebuilt.ErrorMessage,
		)
	}

	buildRuns, err := buildStore.ListBuildRunsByRelease(ctx, rebuilt.ID)
	if err != nil {
		t.Fatalf("list build runs by release: %v", err)
	}
	if len(buildRuns) != 0 {
		t.Fatalf("expected cleared build runs, got %d", len(buildRuns))
	}

	var artifactCount int
	if err := database.QueryRowContext(
		ctx,
		`SELECT COUNT(1) FROM artifacts WHERE build_run_id = ?`,
		buildRunID,
	).Scan(&artifactCount); err != nil {
		t.Fatalf("count artifacts: %v", err)
	}
	if artifactCount != 0 {
		t.Fatalf("expected cleared artifacts, got %d", artifactCount)
	}

	var publishRunCount int
	if err := database.QueryRowContext(
		ctx,
		`SELECT COUNT(1) FROM publish_runs WHERE release_run_id = ?`,
		rebuilt.ID,
	).Scan(&publishRunCount); err != nil {
		t.Fatalf("count publish runs: %v", err)
	}
	if publishRunCount != 0 {
		t.Fatalf("expected cleared publish runs, got %d", publishRunCount)
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
