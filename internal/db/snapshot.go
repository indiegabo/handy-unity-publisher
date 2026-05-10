package db

import (
	"context"
	"database/sql"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"
)

var (
	// ErrInvalidSnapshot reports that one imported SQLite snapshot is not valid.
	ErrInvalidSnapshot = errors.New("invalid sqlite snapshot")
)

// CreateSnapshot materializes one consistent SQLite snapshot on disk and
// returns the temporary file path. The caller must remove the returned file.
func CreateSnapshot(ctx context.Context, databasePath string) (string, error) {
	databasePath = strings.TrimSpace(databasePath)
	if databasePath == "" {
		return "", fmt.Errorf("database path must not be empty")
	}

	tempDir := filepath.Dir(databasePath)
	if err := os.MkdirAll(tempDir, 0o755); err != nil {
		return "", fmt.Errorf("mkdir snapshot directory %s: %w", tempDir, err)
	}

	tempFile, err := os.CreateTemp(tempDir, ".hub-export-*.db")
	if err != nil {
		return "", fmt.Errorf("create snapshot temp file: %w", err)
	}
	tempPath := tempFile.Name()
	if err := tempFile.Close(); err != nil {
		_ = os.Remove(tempPath)
		return "", fmt.Errorf("close snapshot temp file: %w", err)
	}
	if err := os.Remove(tempPath); err != nil && !errors.Is(err, os.ErrNotExist) {
		return "", fmt.Errorf("prepare snapshot temp path: %w", err)
	}

	database, err := sql.Open(driverName, databasePath)
	if err != nil {
		return "", fmt.Errorf("open sqlite database for snapshot: %w", err)
	}
	defer database.Close()

	if err := database.PingContext(ctx); err != nil {
		return "", fmt.Errorf("ping sqlite database for snapshot: %w", err)
	}

	statement := fmt.Sprintf("VACUUM INTO %s;", sqliteStringLiteral(tempPath))
	if _, err := database.ExecContext(ctx, statement); err != nil {
		_ = os.Remove(tempPath)
		return "", fmt.Errorf("vacuum sqlite snapshot: %w", err)
	}

	if err := ValidateSnapshot(ctx, tempPath); err != nil {
		_ = os.Remove(tempPath)
		return "", err
	}

	return tempPath, nil
}

// ReplaceWithSnapshot validates one incoming SQLite snapshot and atomically
// replaces the configured database file with it.
func ReplaceWithSnapshot(
	ctx context.Context,
	databasePath string,
	snapshot io.Reader,
) error {
	databasePath = strings.TrimSpace(databasePath)
	if databasePath == "" {
		return fmt.Errorf("database path must not be empty")
	}
	if snapshot == nil {
		return fmt.Errorf("snapshot reader must not be nil")
	}

	tempDir := filepath.Dir(databasePath)
	if err := os.MkdirAll(tempDir, 0o755); err != nil {
		return fmt.Errorf("mkdir database directory %s: %w", tempDir, err)
	}

	tempFile, err := os.CreateTemp(tempDir, ".hub-import-*.db")
	if err != nil {
		return fmt.Errorf("create import temp file: %w", err)
	}
	tempPath := tempFile.Name()
	cleanupTemp := true
	defer func() {
		if cleanupTemp {
			_ = os.Remove(tempPath)
			_ = os.Remove(tempPath + "-wal")
			_ = os.Remove(tempPath + "-shm")
		}
	}()

	if _, err := io.Copy(tempFile, snapshot); err != nil {
		_ = tempFile.Close()
		return fmt.Errorf("copy import snapshot: %w", err)
	}
	if err := tempFile.Sync(); err != nil {
		_ = tempFile.Close()
		return fmt.Errorf("sync import snapshot: %w", err)
	}
	if err := tempFile.Close(); err != nil {
		return fmt.Errorf("close import snapshot: %w", err)
	}

	if err := ValidateSnapshot(ctx, tempPath); err != nil {
		return err
	}

	_ = os.Remove(databasePath + "-wal")
	_ = os.Remove(databasePath + "-shm")

	if err := os.Rename(tempPath, databasePath); err != nil {
		return fmt.Errorf("replace sqlite database: %w", err)
	}

	cleanupTemp = false
	return nil
}

// ValidateSnapshot checks whether one SQLite file is structurally valid and
// readable by running a lightweight integrity check.
func ValidateSnapshot(ctx context.Context, snapshotPath string) error {
	snapshotPath = strings.TrimSpace(snapshotPath)
	if snapshotPath == "" {
		return fmt.Errorf("%w: snapshot path must not be empty", ErrInvalidSnapshot)
	}

	database, err := sql.Open(driverName, snapshotPath)
	if err != nil {
		return fmt.Errorf("%w: open sqlite snapshot: %v", ErrInvalidSnapshot, err)
	}
	defer database.Close()

	if err := database.PingContext(ctx); err != nil {
		return fmt.Errorf("%w: ping sqlite snapshot: %v", ErrInvalidSnapshot, err)
	}

	var result string
	if err := database.QueryRowContext(ctx, "PRAGMA integrity_check(1);").Scan(&result); err != nil {
		return fmt.Errorf("%w: integrity check failed: %v", ErrInvalidSnapshot, err)
	}

	if !strings.EqualFold(strings.TrimSpace(result), "ok") {
		return fmt.Errorf("%w: integrity check returned %q", ErrInvalidSnapshot, result)
	}

	return nil
}

// sqliteStringLiteral escapes one filesystem path as a SQLite string literal
// for `VACUUM INTO` statements.
func sqliteStringLiteral(value string) string {
	return "'" + strings.ReplaceAll(value, "'", "''") + "'"
}