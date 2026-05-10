// Package db owns SQLite bootstrap, migrations, and operator snapshot flows.
package db

import (
	"context"
	"database/sql"
	"embed"
	"fmt"
	"io/fs"
	"sort"
	"strings"
	"time"
)

//go:embed migrations/*.sql
// migrationsFS embeds all schema migration files applied during bootstrap.
var migrationsFS embed.FS

// migration represents one embedded SQL file applied in deterministic order.
type migration struct {
	name string
	sql  string
}

// migrate ensures the migration ledger exists and applies each pending SQL file
// exactly once.
func migrate(ctx context.Context, database *sql.DB) error {
	if _, err := database.ExecContext(ctx, `
		CREATE TABLE IF NOT EXISTS schema_migrations (
			name TEXT PRIMARY KEY,
			applied_at TEXT NOT NULL
		);
	`); err != nil {
		return fmt.Errorf("create schema_migrations: %w", err)
	}

	migrations, err := loadMigrations()
	if err != nil {
		return err
	}

	for _, item := range migrations {
		applied, err := migrationApplied(ctx, database, item.name)
		if err != nil {
			return err
		}

		if applied {
			continue
		}

		if err := applyMigration(ctx, database, item); err != nil {
			return err
		}
	}

	return nil
}

// loadMigrations reads embedded SQL files and sorts them lexicographically so
// numeric prefixes define execution order.
func loadMigrations() ([]migration, error) {
	paths, err := fs.Glob(migrationsFS, "migrations/*.sql")
	if err != nil {
		return nil, fmt.Errorf("glob migrations: %w", err)
	}

	sort.Strings(paths)

	migrations := make([]migration, 0, len(paths))
	for _, path := range paths {
		contents, err := fs.ReadFile(migrationsFS, path)
		if err != nil {
			return nil, fmt.Errorf("read migration %s: %w", path, err)
		}

		migrations = append(migrations, migration{
			name: strings.TrimPrefix(path, "migrations/"),
			sql:  string(contents),
		})
	}

	return migrations, nil
}

// migrationApplied checks the migration ledger before attempting to run a file.
func migrationApplied(
	ctx context.Context,
	database *sql.DB,
	name string,
) (bool, error) {
	var count int
	if err := database.QueryRowContext(
		ctx,
		`SELECT COUNT(1) FROM schema_migrations WHERE name = ?`,
		name,
	).Scan(&count); err != nil {
		return false, fmt.Errorf("query migration %s: %w", name, err)
	}

	return count > 0, nil
}

// applyMigration executes one SQL file inside a transaction and records it in
// the ledger only after the schema change commits successfully.
func applyMigration(
	ctx context.Context,
	database *sql.DB,
	item migration,
) error {
	tx, err := database.BeginTx(ctx, nil)
	if err != nil {
		return fmt.Errorf("begin migration %s: %w", item.name, err)
	}

	defer func() {
		_ = tx.Rollback()
	}()

	if _, err := tx.ExecContext(ctx, item.sql); err != nil {
		return fmt.Errorf("exec migration %s: %w", item.name, err)
	}

	if _, err := tx.ExecContext(
		ctx,
		`INSERT INTO schema_migrations (name, applied_at) VALUES (?, ?)`,
		item.name,
		time.Now().UTC().Format(time.RFC3339),
	); err != nil {
		return fmt.Errorf("record migration %s: %w", item.name, err)
	}

	if err := tx.Commit(); err != nil {
		return fmt.Errorf("commit migration %s: %w", item.name, err)
	}

	return nil
}