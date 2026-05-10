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