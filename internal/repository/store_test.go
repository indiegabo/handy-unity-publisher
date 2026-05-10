package repository_test

import (
	"context"
	"errors"
	"path/filepath"
	"testing"

	"github.com/indiegabo/handy-unity-bulder/internal/config"
	"github.com/indiegabo/handy-unity-bulder/internal/db"
	"github.com/indiegabo/handy-unity-bulder/internal/repository"
)

func TestStoreCRUD(t *testing.T) {
	t.Parallel()

	ctx := context.Background()
	store := newTestStore(t)

	record, err := store.Create(ctx, repository.CreateInput{
		Name:                   "core-pipeline",
		RepoURL:                "https://example.com/org/core.git",
		DefaultBranch:          "main",
		PollingIntervalSeconds: 120,
	})
	if err != nil {
		t.Fatalf("create repository: %v", err)
	}

	if record.ID == 0 {
		t.Fatalf("expected created repository id to be set")
	}

	if !record.Enabled {
		t.Fatalf("expected created repository to default to enabled")
	}

	loaded, err := store.Get(ctx, record.ID)
	if err != nil {
		t.Fatalf("get repository: %v", err)
	}

	if loaded.Name != record.Name {
		t.Fatalf("expected name %q, got %q", record.Name, loaded.Name)
	}

	updated, err := store.Update(ctx, record.ID, repository.UpdateInput{
		Name:                   "core-pipeline-v2",
		RepoURL:                "https://example.com/org/core-v2.git",
		DefaultBranch:          "stable",
		PollingIntervalSeconds: 600,
		Enabled:                false,
	})
	if err != nil {
		t.Fatalf("update repository: %v", err)
	}

	if updated.Name != "core-pipeline-v2" {
		t.Fatalf("expected updated name, got %q", updated.Name)
	}

	if updated.Enabled {
		t.Fatalf("expected updated repository to be disabled")
	}

	records, err := store.List(ctx)
	if err != nil {
		t.Fatalf("list repositories: %v", err)
	}

	if len(records) != 1 {
		t.Fatalf("expected one repository, got %d", len(records))
	}

	if err := store.Delete(ctx, record.ID); err != nil {
		t.Fatalf("delete repository: %v", err)
	}

	_, err = store.Get(ctx, record.ID)
	if !errors.Is(err, repository.ErrNotFound) {
		t.Fatalf("expected not found after delete, got %v", err)
	}
	}

func TestStoreRejectsDuplicateRepositoryIdentity(t *testing.T) {
	t.Parallel()

	ctx := context.Background()
	store := newTestStore(t)

	_, err := store.Create(ctx, repository.CreateInput{
		Name:    "dupe",
		RepoURL: "https://example.com/org/dupe.git",
	})
	if err != nil {
		t.Fatalf("seed repository: %v", err)
	}

	_, err = store.Create(ctx, repository.CreateInput{
		Name:    "dupe",
		RepoURL: "https://example.com/org/other.git",
	})
	if !errors.Is(err, repository.ErrConflict) {
		t.Fatalf("expected duplicate name conflict, got %v", err)
	}

	_, err = store.Create(ctx, repository.CreateInput{
		Name:    "other",
		RepoURL: "https://example.com/org/dupe.git",
	})
	if !errors.Is(err, repository.ErrConflict) {
		t.Fatalf("expected duplicate repo url conflict, got %v", err)
	}
}

func TestStoreRejectsInvalidInput(t *testing.T) {
	t.Parallel()

	ctx := context.Background()
	store := newTestStore(t)

	_, err := store.Create(ctx, repository.CreateInput{})
	if !errors.Is(err, repository.ErrInvalid) {
		t.Fatalf("expected invalid create error, got %v", err)
	}

	_, err = store.Update(ctx, 99, repository.UpdateInput{
		Name:                   "repo",
		RepoURL:                "https://example.com/org/repo.git",
		PollingIntervalSeconds: 0,
		Enabled:                true,
	})
	if !errors.Is(err, repository.ErrInvalid) {
		t.Fatalf("expected invalid update error, got %v", err)
	}
}

func TestStoreUpdatesLastSeenTag(t *testing.T) {
	t.Parallel()

	ctx := context.Background()
	store := newTestStore(t)

	record, err := store.Create(ctx, repository.CreateInput{
		Name:    "tagged-repo",
		RepoURL: "https://example.com/org/tagged.git",
	})
	if err != nil {
		t.Fatalf("create repository: %v", err)
	}

	updated, err := store.UpdateLastSeenTag(ctx, record.ID, "v1.2.3")
	if err != nil {
		t.Fatalf("update last seen tag: %v", err)
	}

	if updated.LastSeenTag == nil || *updated.LastSeenTag != "v1.2.3" {
		t.Fatalf("expected last seen tag v1.2.3, got %#v", updated.LastSeenTag)
	}
}

func newTestStore(t *testing.T) repository.Store {
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

	return repository.NewStore(database)
}