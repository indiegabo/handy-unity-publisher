package trigger_test

import (
	"context"
	"errors"
	"path/filepath"
	"testing"

	"github.com/indiegabo/handy-unity-bulder/internal/config"
	"github.com/indiegabo/handy-unity-bulder/internal/db"
	"github.com/indiegabo/handy-unity-bulder/internal/repository"
	"github.com/indiegabo/handy-unity-bulder/internal/trigger"
)

func TestStoreCRUD(t *testing.T) {
	t.Parallel()

	ctx := context.Background()
	repositoryStore, triggerStore := newTestStores(t)

	repo, err := repositoryStore.Create(ctx, repository.CreateInput{
		Name:    "triggered-repo",
		RepoURL: "https://example.com/org/triggered.git",
	})
	if err != nil {
		t.Fatalf("create repository: %v", err)
	}

	rule, err := triggerStore.Create(ctx, trigger.CreateInput{
		RepositoryID: repo.ID,
		Name:         "manual-default",
		Source:       trigger.SourceManual,
		ConfigJSON:   `{"requested_via":"cli"}`,
	})
	if err != nil {
		t.Fatalf("create trigger rule: %v", err)
	}

	if rule.ID == 0 {
		t.Fatalf("expected trigger rule id to be set")
	}

	loaded, err := triggerStore.Get(ctx, rule.ID)
	if err != nil {
		t.Fatalf("get trigger rule: %v", err)
	}

	if loaded.Source != trigger.SourceManual {
		t.Fatalf("expected source %q, got %q", trigger.SourceManual, loaded.Source)
	}

	updated, err := triggerStore.Update(ctx, rule.ID, trigger.UpdateInput{
		Name:       "poll-default",
		Source:     trigger.SourcePoll,
		Enabled:    false,
		ConfigJSON: `{"interval_seconds":300}`,
	})
	if err != nil {
		t.Fatalf("update trigger rule: %v", err)
	}

	if updated.Enabled {
		t.Fatalf("expected updated trigger rule to be disabled")
	}

	if updated.Source != trigger.SourcePoll {
		t.Fatalf("expected updated source %q, got %q", trigger.SourcePoll, updated.Source)
	}

	rules, err := triggerStore.ListByRepository(ctx, repo.ID)
	if err != nil {
		t.Fatalf("list trigger rules: %v", err)
	}

	if len(rules) != 1 {
		t.Fatalf("expected one trigger rule, got %d", len(rules))
	}

	if err := triggerStore.Delete(ctx, rule.ID); err != nil {
		t.Fatalf("delete trigger rule: %v", err)
	}

	_, err = triggerStore.Get(ctx, rule.ID)
	if !errors.Is(err, trigger.ErrNotFound) {
		t.Fatalf("expected not found after delete, got %v", err)
	}
}

func TestStoreRejectsDuplicateRuleNamePerRepository(t *testing.T) {
	t.Parallel()

	ctx := context.Background()
	repositoryStore, triggerStore := newTestStores(t)

	repo, err := repositoryStore.Create(ctx, repository.CreateInput{
		Name:    "trigger-dupe",
		RepoURL: "https://example.com/org/trigger-dupe.git",
	})
	if err != nil {
		t.Fatalf("create repository: %v", err)
	}

	_, err = triggerStore.Create(ctx, trigger.CreateInput{
		RepositoryID: repo.ID,
		Name:         "default",
		Source:       trigger.SourceManual,
	})
	if err != nil {
		t.Fatalf("seed trigger rule: %v", err)
	}

	_, err = triggerStore.Create(ctx, trigger.CreateInput{
		RepositoryID: repo.ID,
		Name:         "default",
		Source:       trigger.SourcePoll,
	})
	if !errors.Is(err, trigger.ErrConflict) {
		t.Fatalf("expected duplicate trigger rule conflict, got %v", err)
	}
}

func TestStoreRejectsInvalidInputAndUnknownRepository(t *testing.T) {
	t.Parallel()

	ctx := context.Background()
	_, triggerStore := newTestStores(t)

	_, err := triggerStore.Create(ctx, trigger.CreateInput{
		RepositoryID: 999,
		Name:         "manual-default",
		Source:       trigger.SourceManual,
	})
	if !errors.Is(err, trigger.ErrRepositoryNotFound) {
		t.Fatalf("expected repository not found error, got %v", err)
	}

	_, err = triggerStore.Create(ctx, trigger.CreateInput{
		RepositoryID: 1,
		Name:         "bad-config",
		Source:       "cron",
		ConfigJSON:   `[1,2,3]`,
	})
	if !errors.Is(err, trigger.ErrInvalid) {
		t.Fatalf("expected invalid trigger rule error, got %v", err)
	}
}

func TestStoreListEnabledBySource(t *testing.T) {
	t.Parallel()

	ctx := context.Background()
	repositoryStore, triggerStore := newTestStores(t)

	repo, err := repositoryStore.Create(ctx, repository.CreateInput{
		Name:    "poll-list-repo",
		RepoURL: "https://example.com/org/poll-list.git",
	})
	if err != nil {
		t.Fatalf("create repository: %v", err)
	}

	if _, err := triggerStore.Create(ctx, trigger.CreateInput{
		RepositoryID: repo.ID,
		Name:         "poll-enabled",
		Source:       trigger.SourcePoll,
	}); err != nil {
		t.Fatalf("create enabled poll trigger: %v", err)
	}

	if _, err := triggerStore.Create(ctx, trigger.CreateInput{
		RepositoryID: repo.ID,
		Name:         "manual-enabled",
		Source:       trigger.SourceManual,
	}); err != nil {
		t.Fatalf("create enabled manual trigger: %v", err)
	}

	enabled := false
	if _, err := triggerStore.Create(ctx, trigger.CreateInput{
		RepositoryID: repo.ID,
		Name:         "poll-disabled",
		Source:       trigger.SourcePoll,
		Enabled:      &enabled,
	}); err != nil {
		t.Fatalf("create disabled poll trigger: %v", err)
	}

	rules, err := triggerStore.ListEnabledBySource(ctx, trigger.SourcePoll)
	if err != nil {
		t.Fatalf("list enabled poll rules: %v", err)
	}

	if len(rules) != 1 {
		t.Fatalf("expected one enabled poll rule, got %d", len(rules))
	}

	if rules[0].Name != "poll-enabled" {
		t.Fatalf("expected enabled poll rule, got %q", rules[0].Name)
	}
}

func newTestStores(t *testing.T) (repository.Store, trigger.Store) {
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

	return repository.NewStore(database), trigger.NewStore(database)
}