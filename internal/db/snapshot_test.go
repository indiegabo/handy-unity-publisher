package db

import (
	"bytes"
	"context"
	"database/sql"
	"errors"
	"os"
	"path/filepath"
	"testing"
)

func TestCreateSnapshotProducesReadableCopy(t *testing.T) {
	t.Parallel()

	ctx := context.Background()
	databasePath := filepath.Join(t.TempDir(), "source.db")
	seedTestSQLite(t, databasePath, []string{
		`CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT NOT NULL);`,
		`INSERT INTO items (name) VALUES ('alpha');`,
	})

	snapshotPath, err := CreateSnapshot(ctx, databasePath)
	if err != nil {
		t.Fatalf("CreateSnapshot() error = %v", err)
	}
	t.Cleanup(func() { _ = os.Remove(snapshotPath) })

	loaded := openTestSQLite(t, snapshotPath)
	defer loaded.Close()

	var count int
	if err := loaded.QueryRowContext(ctx, `SELECT COUNT(*) FROM items WHERE name = 'alpha';`).Scan(&count); err != nil {
		t.Fatalf("query snapshot row count: %v", err)
	}
	if count != 1 {
		t.Fatalf("expected snapshot row count 1, got %d", count)
	}
}

func TestReplaceWithSnapshotOverwritesDatabase(t *testing.T) {
	t.Parallel()

	ctx := context.Background()
	tempDir := t.TempDir()
	targetPath := filepath.Join(tempDir, "target.db")
	sourcePath := filepath.Join(tempDir, "source.db")

	seedTestSQLite(t, targetPath, []string{
		`CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT NOT NULL);`,
		`INSERT INTO items (name) VALUES ('old');`,
	})
	seedTestSQLite(t, sourcePath, []string{
		`CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT NOT NULL);`,
		`INSERT INTO items (name) VALUES ('new');`,
	})

	snapshotBytes, err := os.ReadFile(sourcePath)
	if err != nil {
		t.Fatalf("ReadFile(sourcePath) error = %v", err)
	}

	if err := ReplaceWithSnapshot(ctx, targetPath, bytes.NewReader(snapshotBytes)); err != nil {
		t.Fatalf("ReplaceWithSnapshot() error = %v", err)
	}

	loaded := openTestSQLite(t, targetPath)
	defer loaded.Close()

	var name string
	if err := loaded.QueryRowContext(ctx, `SELECT name FROM items LIMIT 1;`).Scan(&name); err != nil {
		t.Fatalf("query imported row: %v", err)
	}
	if name != "new" {
		t.Fatalf("expected imported name %q, got %q", "new", name)
	}
}

func TestReplaceWithSnapshotRejectsInvalidFile(t *testing.T) {
	t.Parallel()

	err := ReplaceWithSnapshot(
		context.Background(),
		filepath.Join(t.TempDir(), "target.db"),
		bytes.NewBufferString("not-a-sqlite-database"),
	)
	if !errors.Is(err, ErrInvalidSnapshot) {
		t.Fatalf("expected ErrInvalidSnapshot, got %v", err)
	}
}

func openTestSQLite(t *testing.T, path string) *sql.DB {
	t.Helper()

	database, err := sql.Open(driverName, path)
	if err != nil {
		t.Fatalf("sql.Open(%q) error = %v", path, err)
	}
	if err := database.PingContext(context.Background()); err != nil {
		_ = database.Close()
		t.Fatalf("PingContext(%q) error = %v", path, err)
	}

	return database
}

func seedTestSQLite(t *testing.T, path string, statements []string) {
	t.Helper()

	database := openTestSQLite(t, path)
	defer database.Close()

	for _, statement := range statements {
		if _, err := database.ExecContext(context.Background(), statement); err != nil {
			t.Fatalf("ExecContext(%q) error = %v", statement, err)
		}
	}
}
