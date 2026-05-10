// Package db contains SQLite bootstrap and migration helpers.
package db

import (
	"context"
	"database/sql"
	"fmt"
	"os"

	"github.com/indiegabo/handy-unity-bulder/internal/config"
	_ "modernc.org/sqlite"
)

// driverName identifies the pure-Go SQLite driver used by the service.
const driverName = "sqlite"

// Open prepares the filesystem layout, opens SQLite, and applies migrations.
func Open(ctx context.Context, cfg config.Config) (*sql.DB, error) {
	if err := ensureDataLayout(cfg); err != nil {
		return nil, fmt.Errorf("ensure data layout: %w", err)
	}

	database, err := sql.Open(driverName, cfg.DBPath())
	if err != nil {
		return nil, fmt.Errorf("open sqlite database: %w", err)
	}

	if err := database.PingContext(ctx); err != nil {
		_ = database.Close()
		return nil, fmt.Errorf("ping sqlite database: %w", err)
	}

	if err := applyPragmas(ctx, database); err != nil {
		_ = database.Close()
		return nil, fmt.Errorf("apply sqlite pragmas: %w", err)
	}

	if err := migrate(ctx, database); err != nil {
		_ = database.Close()
		return nil, fmt.Errorf("apply migrations: %w", err)
	}

	return database, nil
}

// ensureDataLayout creates the mounted directories expected by the runtime so
// later build and publish stages can write files without implicit setup.
func ensureDataLayout(cfg config.Config) error {
	for _, dir := range cfg.RequiredDirs() {
		if err := os.MkdirAll(dir, 0o755); err != nil {
			return fmt.Errorf("mkdir %s: %w", dir, err)
		}
	}

	return nil
}

// applyPragmas enables SQLite behaviors required by the local-first runtime,
// including foreign keys, WAL mode, and bounded lock waits.
func applyPragmas(ctx context.Context, database *sql.DB) error {
	statements := []string{
		"PRAGMA foreign_keys = ON;",
		"PRAGMA journal_mode = WAL;",
		"PRAGMA busy_timeout = 5000;",
	}

	for _, statement := range statements {
		if _, err := database.ExecContext(ctx, statement); err != nil {
			return fmt.Errorf("exec %q: %w", statement, err)
		}
	}

	return nil
}