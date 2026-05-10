// Package repository contains repository pipeline configuration models and
// consumer-facing storage contracts.
package repository

import (
	"context"
	"database/sql"
	"errors"
	"fmt"
	"strings"
)

// defaultPollingIntervalSeconds is the fallback repository polling interval
// applied when callers omit an explicit value.
const defaultPollingIntervalSeconds = 300

var (
	// ErrInvalid reports validation failures on repository input.
	ErrInvalid = errors.New("invalid repository input")
	// ErrNotFound reports missing repository records.
	ErrNotFound = errors.New("repository not found")
	// ErrConflict reports uniqueness collisions for repository records.
	ErrConflict = errors.New("repository conflict")
)

// Record is the durable repository pipeline definition stored in SQLite.
type Record struct {
	ID                     int64   `json:"id"`
	Name                   string  `json:"name"`
	RepoURL                string  `json:"repo_url"`
	CredentialsID          *int64  `json:"credentials_id,omitempty"`
	DefaultBranch          *string `json:"default_branch,omitempty"`
	PollingIntervalSeconds int     `json:"polling_interval_seconds"`
	LastSeenTag            *string `json:"last_seen_tag,omitempty"`
	Enabled                bool    `json:"enabled"`
	CreatedAt              string  `json:"created_at"`
	UpdatedAt              string  `json:"updated_at"`
}

// CreateInput defines the user-controlled fields accepted when a repository is
// first registered.
type CreateInput struct {
	Name                   string
	RepoURL                string
	CredentialsID          *int64
	DefaultBranch          string
	PollingIntervalSeconds int
	Enabled                *bool
}

// UpdateInput defines the user-controlled fields accepted when a repository is
// replaced.
type UpdateInput struct {
	Name                   string
	RepoURL                string
	CredentialsID          *int64
	DefaultBranch          string
	PollingIntervalSeconds int
	Enabled                bool
}

// Store exposes the repository operations currently needed by CLI and HTTP
// management surfaces.
type Store interface {
	Create(ctx context.Context, input CreateInput) (Record, error)
	Get(ctx context.Context, id int64) (Record, error)
	List(ctx context.Context) ([]Record, error)
	UpdateLastSeenTag(ctx context.Context, id int64, lastSeenTag string) (Record, error)
	Update(ctx context.Context, id int64, input UpdateInput) (Record, error)
	Delete(ctx context.Context, id int64) error
}

// NewStore creates the SQLite-backed repository store.
func NewStore(database *sql.DB) Store {
	return &sqliteStore{database: database}
}

// sqliteStore persists repository pipeline definitions in SQLite.
type sqliteStore struct {
	database *sql.DB
}

// Create inserts a repository definition and returns the stored record.
func (s *sqliteStore) Create(ctx context.Context, input CreateInput) (Record, error) {
	normalized, err := normalizeCreateInput(input)
	if err != nil {
		return Record{}, err
	}

	result, err := s.database.ExecContext(
		ctx,
		`INSERT INTO repositories (
			name,
			repo_url,
			credentials_id,
			default_branch,
			polling_interval_seconds,
			enabled
		) VALUES (?, ?, ?, ?, ?, ?)`,
		normalized.Name,
		normalized.RepoURL,
		nullableInt64(normalized.CredentialsID),
		nullableString(normalized.DefaultBranch),
		normalized.PollingIntervalSeconds,
		boolToSQLite(normalized.Enabled),
	)
	if err != nil {
		return Record{}, mapSQLError(err)
	}

	id, err := result.LastInsertId()
	if err != nil {
		return Record{}, fmt.Errorf("read repository id: %w", err)
	}

	return s.Get(ctx, id)
}

// Get loads a repository definition by identifier.
func (s *sqliteStore) Get(ctx context.Context, id int64) (Record, error) {
	row := s.database.QueryRowContext(
		ctx,
		`SELECT
			id,
			name,
			repo_url,
			credentials_id,
			default_branch,
			polling_interval_seconds,
			last_seen_tag,
			enabled,
			created_at,
			updated_at
		FROM repositories
		WHERE id = ?`,
		id,
	)

	record, err := scanRecord(row)
	if err != nil {
		if errors.Is(err, sql.ErrNoRows) {
			return Record{}, ErrNotFound
		}

		return Record{}, fmt.Errorf("query repository %d: %w", id, err)
	}

	return record, nil
}

// List returns the registered repositories ordered by identifier.
func (s *sqliteStore) List(ctx context.Context) ([]Record, error) {
	rows, err := s.database.QueryContext(
		ctx,
		`SELECT
			id,
			name,
			repo_url,
			credentials_id,
			default_branch,
			polling_interval_seconds,
			last_seen_tag,
			enabled,
			created_at,
			updated_at
		FROM repositories
		ORDER BY id ASC`,
	)
	if err != nil {
		return nil, fmt.Errorf("list repositories: %w", err)
	}
	defer rows.Close()

	records := make([]Record, 0)
	for rows.Next() {
		record, err := scanRecord(rows)
		if err != nil {
			return nil, fmt.Errorf("scan repository row: %w", err)
		}

		records = append(records, record)
	}

	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("iterate repositories: %w", err)
	}

	return records, nil
}

// UpdateLastSeenTag persists the most recent tag observed by repository
// polling.
func (s *sqliteStore) UpdateLastSeenTag(
	ctx context.Context,
	id int64,
	lastSeenTag string,
) (Record, error) {
	lastSeenTag = strings.TrimSpace(lastSeenTag)
	if id <= 0 {
		return Record{}, fmt.Errorf(
			"%w: repository_id must be greater than zero",
			ErrInvalid,
		)
	}

	if lastSeenTag == "" {
		return Record{}, fmt.Errorf(
			"%w: last_seen_tag must not be empty",
			ErrInvalid,
		)
	}

	result, err := s.database.ExecContext(
		ctx,
		`UPDATE repositories
		SET
			last_seen_tag = ?,
			updated_at = CURRENT_TIMESTAMP
		WHERE id = ?`,
		lastSeenTag,
		id,
	)
	if err != nil {
		return Record{}, fmt.Errorf("update repository %d last seen tag: %w", id, err)
	}

	rowsAffected, err := result.RowsAffected()
	if err != nil {
		return Record{}, fmt.Errorf("read updated rows: %w", err)
	}

	if rowsAffected == 0 {
		return Record{}, ErrNotFound
	}

	return s.Get(ctx, id)
}

// Update replaces the writable fields of an existing repository definition.
func (s *sqliteStore) Update(ctx context.Context, id int64, input UpdateInput) (Record, error) {
	normalized, err := normalizeUpdateInput(input)
	if err != nil {
		return Record{}, err
	}

	result, err := s.database.ExecContext(
		ctx,
		`UPDATE repositories
		SET
			name = ?,
			repo_url = ?,
			credentials_id = ?,
			default_branch = ?,
			polling_interval_seconds = ?,
			enabled = ?,
			updated_at = CURRENT_TIMESTAMP
		WHERE id = ?`,
		normalized.Name,
		normalized.RepoURL,
		nullableInt64(normalized.CredentialsID),
		nullableString(normalized.DefaultBranch),
		normalized.PollingIntervalSeconds,
		boolToSQLite(normalized.Enabled),
		id,
	)
	if err != nil {
		return Record{}, mapSQLError(err)
	}

	rowsAffected, err := result.RowsAffected()
	if err != nil {
		return Record{}, fmt.Errorf("read updated rows: %w", err)
	}

	if rowsAffected == 0 {
		return Record{}, ErrNotFound
	}

	return s.Get(ctx, id)
}

// Delete removes a repository definition.
func (s *sqliteStore) Delete(ctx context.Context, id int64) error {
	result, err := s.database.ExecContext(
		ctx,
		`DELETE FROM repositories WHERE id = ?`,
		id,
	)
	if err != nil {
		return fmt.Errorf("delete repository %d: %w", id, err)
	}

	rowsAffected, err := result.RowsAffected()
	if err != nil {
		return fmt.Errorf("read deleted rows: %w", err)
	}

	if rowsAffected == 0 {
		return ErrNotFound
	}

	return nil
}

// scanner abstracts row scanners shared by single-row and multi-row queries.
type scanner interface {
	Scan(dest ...any) error
}

// scanRecord decodes one repository row into the public repository model.
func scanRecord(row scanner) (Record, error) {
	var record Record
	var credentialsID sql.NullInt64
	var defaultBranch sql.NullString
	var lastSeenTag sql.NullString
	var enabled int64

	err := row.Scan(
		&record.ID,
		&record.Name,
		&record.RepoURL,
		&credentialsID,
		&defaultBranch,
		&record.PollingIntervalSeconds,
		&lastSeenTag,
		&enabled,
		&record.CreatedAt,
		&record.UpdatedAt,
	)
	if err != nil {
		return Record{}, err
	}

	record.CredentialsID = int64Pointer(credentialsID)
	record.DefaultBranch = stringPointer(defaultBranch)
	record.LastSeenTag = stringPointer(lastSeenTag)
	record.Enabled = enabled == 1

	return record, nil
}

// normalizeCreateInput converts create input into the validated update shape
// used internally by the repository store.
func normalizeCreateInput(input CreateInput) (UpdateInput, error) {
	enabled := true
	if input.Enabled != nil {
		enabled = *input.Enabled
	}

	interval := input.PollingIntervalSeconds
	if interval == 0 {
		interval = defaultPollingIntervalSeconds
	}

	return normalizeUpdateInput(UpdateInput{
		Name:                   input.Name,
		RepoURL:                input.RepoURL,
		CredentialsID:          input.CredentialsID,
		DefaultBranch:          input.DefaultBranch,
		PollingIntervalSeconds: interval,
		Enabled:                enabled,
	})
}

// normalizeUpdateInput trims and validates one repository payload.
func normalizeUpdateInput(input UpdateInput) (UpdateInput, error) {
	input.Name = strings.TrimSpace(input.Name)
	input.RepoURL = strings.TrimSpace(input.RepoURL)
	input.DefaultBranch = strings.TrimSpace(input.DefaultBranch)

	if input.Name == "" {
		return UpdateInput{}, fmt.Errorf("%w: name must not be empty", ErrInvalid)
	}

	if input.RepoURL == "" {
		return UpdateInput{}, fmt.Errorf("%w: repo_url must not be empty", ErrInvalid)
	}

	if input.PollingIntervalSeconds <= 0 {
		return UpdateInput{}, fmt.Errorf(
			"%w: polling_interval_seconds must be greater than zero",
			ErrInvalid,
		)
	}

	return input, nil
}

// mapSQLError translates common SQLite constraint failures into
// repository-domain errors.
func mapSQLError(err error) error {
	if err == nil {
		return nil
	}

	if strings.Contains(strings.ToLower(err.Error()), "unique constraint failed") {
		return fmt.Errorf("%w: %v", ErrConflict, err)
	}

	return fmt.Errorf("repository store: %w", err)
}

// boolToSQLite converts one boolean into the integer representation used by
// SQLite tables.
func boolToSQLite(value bool) int {
	if value {
		return 1
	}

	return 0
}

// nullableInt64 returns nil when the optional integer pointer is absent.
func nullableInt64(value *int64) any {
	if value == nil {
		return nil
	}

	return *value
}

// nullableString returns nil for blank strings so optional SQLite columns stay
// NULL when unset.
func nullableString(value string) any {
	if strings.TrimSpace(value) == "" {
		return nil
	}

	return strings.TrimSpace(value)
}

// int64Pointer converts a nullable SQL integer into an optional Go pointer.
func int64Pointer(value sql.NullInt64) *int64 {
	if !value.Valid {
		return nil
	}

	copy := value.Int64
	return &copy
}

// stringPointer converts a nullable SQL string into an optional Go pointer.
func stringPointer(value sql.NullString) *string {
	if !value.Valid {
		return nil
	}

	copy := value.String
	return &copy
}