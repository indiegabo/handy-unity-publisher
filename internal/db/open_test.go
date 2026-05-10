package db

import (
	"context"
	"os"
	"path/filepath"
	"testing"

	"github.com/indiegabo/handy-unity-bulder/internal/config"
)

func TestOpenCreatesConfiguredDatabasePathAndRuntimeDirs(t *testing.T) {
	tempDir := t.TempDir()
	cfg := config.Config{
		Env:          "test",
		HTTPAddr:     ":0",
		DataDir:      filepath.Join(tempDir, "runtime-data"),
		DatabasePath: filepath.Join(tempDir, "state", "custom.db"),
		LogLevel:     "debug",
	}

	database, err := Open(context.Background(), cfg)
	if err != nil {
		t.Fatalf("Open() error = %v", err)
	}
	t.Cleanup(func() {
		_ = database.Close()
	})

	for _, path := range []string{
		cfg.DataDir,
		cfg.LogsDir(),
		cfg.ArtifactsDir(),
		cfg.WorkspacesDir(),
		filepath.Dir(cfg.DBPath()),
		cfg.DBPath(),
	} {
		if _, err := os.Stat(path); err != nil {
			t.Fatalf("os.Stat(%q) error = %v", path, err)
		}
	}
}

func TestOpenConfiguresSingleConnectionPoolAndPragmas(t *testing.T) {
	t.Parallel()

	tempDir := t.TempDir()
	cfg := config.Config{
		Env:          "test",
		HTTPAddr:     ":0",
		DataDir:      filepath.Join(tempDir, "runtime-data"),
		DatabasePath: filepath.Join(tempDir, "state", "lock.db"),
		LogLevel:     "debug",
	}

	database, err := Open(context.Background(), cfg)
	if err != nil {
		t.Fatalf("Open() initial error = %v", err)
	}
	t.Cleanup(func() {
		_ = database.Close()
	})

	if got := database.Stats().MaxOpenConnections; got != 1 {
		t.Fatalf("database.Stats().MaxOpenConnections = %d, want 1", got)
	}

	connection, err := database.Conn(context.Background())
	if err != nil {
		t.Fatalf("database.Conn() error = %v", err)
	}
	defer connection.Close()

	var busyTimeout int
	if err := connection.QueryRowContext(
		context.Background(),
		"PRAGMA busy_timeout;",
	).Scan(&busyTimeout); err != nil {
		t.Fatalf("PRAGMA busy_timeout scan error = %v", err)
	}
	if busyTimeout != 5000 {
		t.Fatalf("PRAGMA busy_timeout = %d, want 5000", busyTimeout)
	}

	var foreignKeys int
	if err := connection.QueryRowContext(
		context.Background(),
		"PRAGMA foreign_keys;",
	).Scan(&foreignKeys); err != nil {
		t.Fatalf("PRAGMA foreign_keys scan error = %v", err)
	}
	if foreignKeys != 1 {
		t.Fatalf("PRAGMA foreign_keys = %d, want 1", foreignKeys)
	}

	var journalMode string
	if err := connection.QueryRowContext(
		context.Background(),
		"PRAGMA journal_mode;",
	).Scan(&journalMode); err != nil {
		t.Fatalf("PRAGMA journal_mode scan error = %v", err)
	}
	if journalMode != "wal" {
		t.Fatalf("PRAGMA journal_mode = %q, want %q", journalMode, "wal")
	}
}
