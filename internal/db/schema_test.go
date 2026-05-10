package db

import (
	"context"
	"database/sql"
	"path/filepath"
	"testing"

	"github.com/indiegabo/handy-unity-bulder/internal/config"
)

func TestOpenAppliesTriggerRuleSchema(t *testing.T) {
	t.Parallel()

	dataDir := t.TempDir()
	cfg := config.Config{
		DataDir:      dataDir,
		DatabasePath: filepath.Join(dataDir, "schema.db"),
	}

	database, err := Open(context.Background(), cfg)
	if err != nil {
		t.Fatalf("open database: %v", err)
	}
	t.Cleanup(func() {
		_ = database.Close()
	})

	if !tableExists(t, database, "trigger_rules") {
		t.Fatalf("expected trigger_rules table to exist")
	}

	columns := tableColumns(t, database, "release_runs")
	for _, column := range []string{
		"trigger_source",
		"trigger_rule_id",
		"source_metadata_json",
	} {
		if _, ok := columns[column]; !ok {
			t.Fatalf("expected release_runs column %q to exist", column)
		}
	}
}

func tableExists(t *testing.T, database *sql.DB, tableName string) bool {
	t.Helper()

	var count int
	if err := database.QueryRow(
		`SELECT COUNT(1) FROM sqlite_master WHERE type = 'table' AND name = ?`,
		tableName,
	).Scan(&count); err != nil {
		t.Fatalf("query sqlite_master: %v", err)
	}

	return count == 1
}

func tableColumns(
	t *testing.T,
	database *sql.DB,
	tableName string,
) map[string]struct{} {
	t.Helper()

	rows, err := database.Query(`PRAGMA table_info(` + tableName + `)`)
	if err != nil {
		t.Fatalf("query table_info(%s): %v", tableName, err)
	}
	defer rows.Close()

	columns := make(map[string]struct{})
	for rows.Next() {
		var cid int
		var name string
		var columnType string
		var notNull int
		var defaultValue sql.NullString
		var primaryKey int

		if err := rows.Scan(
			&cid,
			&name,
			&columnType,
			&notNull,
			&defaultValue,
			&primaryKey,
		); err != nil {
			t.Fatalf("scan table_info row: %v", err)
		}

		columns[name] = struct{}{}
	}

	if err := rows.Err(); err != nil {
		t.Fatalf("iterate table_info rows: %v", err)
	}

	return columns
}