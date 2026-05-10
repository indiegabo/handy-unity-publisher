package credentials_test

import (
	"context"
	"errors"
	"path/filepath"
	"testing"

	"github.com/indiegabo/handy-unity-bulder/internal/config"
	"github.com/indiegabo/handy-unity-bulder/internal/credentials"
	"github.com/indiegabo/handy-unity-bulder/internal/db"
)

func TestStoreCRUD(t *testing.T) {
	t.Parallel()

	ctx := context.Background()
	store := newTestStore(t)

	created, err := store.Create(ctx, credentials.CreateInput{
		Name:       "github-token",
		Kind:       credentials.KindGitHTTPBasic,
		ConfigJSON: `{"username":"git","password":"token"}`,
	})
	if err != nil {
		t.Fatalf("create credentials: %v", err)
	}
	if created.ID == 0 {
		t.Fatal("expected credentials id to be set")
	}

	loaded, err := store.Get(ctx, created.ID)
	if err != nil {
		t.Fatalf("get credentials: %v", err)
	}
	if loaded.Name != created.Name {
		t.Fatalf("expected credentials name %q, got %q", created.Name, loaded.Name)
	}

	updated, err := store.Update(ctx, created.ID, credentials.UpdateInput{
		Name:       "github-token-stable",
		Kind:       credentials.KindGitHTTPBasic,
		ConfigJSON: `{"username":"git","password":"token-v2"}`,
	})
	if err != nil {
		t.Fatalf("update credentials: %v", err)
	}
	if updated.Name != "github-token-stable" {
		t.Fatalf("expected updated credentials name, got %q", updated.Name)
	}

	records, err := store.List(ctx)
	if err != nil {
		t.Fatalf("list credentials: %v", err)
	}
	if len(records) != 1 {
		t.Fatalf("expected one credentials record, got %d", len(records))
	}

	if err := store.Delete(ctx, created.ID); err != nil {
		t.Fatalf("delete credentials: %v", err)
	}

	_, err = store.Get(ctx, created.ID)
	if !errors.Is(err, credentials.ErrNotFound) {
		t.Fatalf("expected credentials not found after delete, got %v", err)
	}
}

func newTestStore(t *testing.T) credentials.Store {
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

	return credentials.NewStore(database)
}