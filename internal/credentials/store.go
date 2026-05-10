// Package credentials contains durable credential storage used by repository
// and publish target authentication flows.
package credentials

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"strings"
)

const (
	// KindGitHTTPBasic identifies HTTP basic credentials usable by Git remote
	// operations.
	KindGitHTTPBasic = "git-http-basic"
	// KindGitHTTPBearer identifies bearer-token credentials usable by Git remote
	// operations.
	KindGitHTTPBearer = "git-http-bearer"
)

var (
	// ErrInvalid reports validation failures on credentials input.
	ErrInvalid = errors.New("invalid credentials input")
	// ErrNotFound reports missing credentials records.
	ErrNotFound = errors.New("credentials not found")
	// ErrConflict reports uniqueness collisions for credential records.
	ErrConflict = errors.New("credentials conflict")
)

// Record is one durable credentials row stored in SQLite.
type Record struct {
	ID         int64  `json:"id"`
	Name       string `json:"name"`
	Kind       string `json:"kind"`
	ConfigJSON string `json:"config_json"`
	CreatedAt  string `json:"created_at"`
	UpdatedAt  string `json:"updated_at"`
}

// CreateInput defines the fields accepted when credentials are first
// registered.
type CreateInput struct {
	Name       string
	Kind       string
	ConfigJSON string
}

// UpdateInput defines the fields accepted when credentials are replaced.
type UpdateInput struct {
	Name       string
	Kind       string
	ConfigJSON string
}

// Store exposes the credentials operations currently needed by CLI, HTTP, and
// Git authentication consumers.
type Store interface {
	Create(ctx context.Context, input CreateInput) (Record, error)
	Get(ctx context.Context, id int64) (Record, error)
	List(ctx context.Context) ([]Record, error)
	Update(ctx context.Context, id int64, input UpdateInput) (Record, error)
	Delete(ctx context.Context, id int64) error
}

// NewStore creates the SQLite-backed credentials store.
func NewStore(database *sql.DB) Store {
	return &sqliteStore{database: database}
}

// sqliteStore persists credentials records in SQLite.
type sqliteStore struct {
	database *sql.DB
}

// normalizedInput is the canonical validated form of credentials input.
type normalizedInput struct {
	Name       string
	Kind       string
	ConfigJSON string
}

// Create inserts a credentials record and returns the stored row.
func (s *sqliteStore) Create(ctx context.Context, input CreateInput) (Record, error) {
	normalized, err := normalizeCreateInput(input)
	if err != nil {
		return Record{}, err
	}

	result, err := s.database.ExecContext(
		ctx,
		`INSERT INTO credentials (
			name,
			kind,
			config_json
		) VALUES (?, ?, ?)`,
		normalized.Name,
		normalized.Kind,
		normalized.ConfigJSON,
	)
	if err != nil {
		return Record{}, mapSQLError(err)
	}

	id, err := result.LastInsertId()
	if err != nil {
		return Record{}, fmt.Errorf("read credentials id: %w", err)
	}

	return s.Get(ctx, id)
}

// Get loads one credentials record by identifier.
func (s *sqliteStore) Get(ctx context.Context, id int64) (Record, error) {
	row := s.database.QueryRowContext(
		ctx,
		`SELECT
			id,
			name,
			kind,
			config_json,
			created_at,
			updated_at
		FROM credentials
		WHERE id = ?`,
		id,
	)

	record, err := scanRecord(row)
	if err != nil {
		if errors.Is(err, sql.ErrNoRows) {
			return Record{}, ErrNotFound
		}

		return Record{}, fmt.Errorf("query credentials %d: %w", id, err)
	}

	return record, nil
}

// List returns all stored credentials ordered by id.
func (s *sqliteStore) List(ctx context.Context) ([]Record, error) {
	rows, err := s.database.QueryContext(
		ctx,
		`SELECT
			id,
			name,
			kind,
			config_json,
			created_at,
			updated_at
		FROM credentials
		ORDER BY id ASC`,
	)
	if err != nil {
		return nil, fmt.Errorf("list credentials: %w", err)
	}
	defer rows.Close()

	records := make([]Record, 0)
	for rows.Next() {
		record, err := scanRecord(rows)
		if err != nil {
			return nil, fmt.Errorf("scan credentials row: %w", err)
		}

		records = append(records, record)
	}

	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("iterate credentials: %w", err)
	}

	return records, nil
}

// Update replaces the writable fields of one credentials record.
func (s *sqliteStore) Update(ctx context.Context, id int64, input UpdateInput) (Record, error) {
	normalized, err := normalizeUpdateInput(input)
	if err != nil {
		return Record{}, err
	}

	result, err := s.database.ExecContext(
		ctx,
		`UPDATE credentials
		SET
			name = ?,
			kind = ?,
			config_json = ?,
			updated_at = CURRENT_TIMESTAMP
		WHERE id = ?`,
		normalized.Name,
		normalized.Kind,
		normalized.ConfigJSON,
		id,
	)
	if err != nil {
		return Record{}, mapSQLError(err)
	}

	affected, err := result.RowsAffected()
	if err != nil {
		return Record{}, fmt.Errorf("read updated credentials rows: %w", err)
	}
	if affected == 0 {
		return Record{}, ErrNotFound
	}

	return s.Get(ctx, id)
}

// Delete removes one credentials record.
func (s *sqliteStore) Delete(ctx context.Context, id int64) error {
	result, err := s.database.ExecContext(
		ctx,
		`DELETE FROM credentials WHERE id = ?`,
		id,
	)
	if err != nil {
		return fmt.Errorf("delete credentials %d: %w", id, err)
	}

	affected, err := result.RowsAffected()
	if err != nil {
		return fmt.Errorf("read deleted credentials rows: %w", err)
	}
	if affected == 0 {
		return ErrNotFound
	}

	return nil
}

// scanner abstracts row scanners shared by single-row and multi-row queries.
type scanner interface {
	Scan(dest ...any) error
}

// scanRecord decodes one credentials row into the public credentials model.
func scanRecord(row scanner) (Record, error) {
	var record Record
	err := row.Scan(
		&record.ID,
		&record.Name,
		&record.Kind,
		&record.ConfigJSON,
		&record.CreatedAt,
		&record.UpdatedAt,
	)
	if err != nil {
		return Record{}, err
	}

	return record, nil
}

// normalizeCreateInput converts create input into the validated credentials
// payload stored by the credentials store.
func normalizeCreateInput(input CreateInput) (normalizedInput, error) {
	return normalizeInput(normalizedInput{
		Name:       input.Name,
		Kind:       input.Kind,
		ConfigJSON: input.ConfigJSON,
	})
}

// normalizeUpdateInput converts update input into the validated credentials
// payload stored by the credentials store.
func normalizeUpdateInput(input UpdateInput) (normalizedInput, error) {
	return normalizeInput(normalizedInput{
		Name:       input.Name,
		Kind:       input.Kind,
		ConfigJSON: input.ConfigJSON,
	})
}

// normalizeInput trims and validates one credentials payload.
func normalizeInput(input normalizedInput) (normalizedInput, error) {
	input.Name = strings.TrimSpace(input.Name)
	input.Kind = strings.ToLower(strings.TrimSpace(input.Kind))

	if input.Name == "" {
		return normalizedInput{}, fmt.Errorf("%w: name must not be empty", ErrInvalid)
	}
	if input.Kind == "" {
		return normalizedInput{}, fmt.Errorf("%w: kind must not be empty", ErrInvalid)
	}

	configJSON, err := normalizeJSONObject(input.ConfigJSON)
	if err != nil {
		return normalizedInput{}, err
	}
	input.ConfigJSON = configJSON

	return input, nil
}

// normalizeJSONObject validates credentials config payloads as canonical JSON
// objects.
func normalizeJSONObject(raw string) (string, error) {
	trimmed := strings.TrimSpace(raw)
	if trimmed == "" {
		return `{}`, nil
	}

	var decoded any
	if err := json.Unmarshal([]byte(trimmed), &decoded); err != nil {
		return "", fmt.Errorf("%w: config_json must be valid JSON: %v", ErrInvalid, err)
	}
	if _, ok := decoded.(map[string]any); !ok {
		return "", fmt.Errorf("%w: config_json must be a JSON object", ErrInvalid)
	}

	canonical, err := json.Marshal(decoded)
	if err != nil {
		return "", fmt.Errorf("normalize config_json: %w", err)
	}

	return string(canonical), nil
}

// mapSQLError translates common SQLite constraint failures into
// credentials-domain errors.
func mapSQLError(err error) error {
	if err == nil {
		return nil
	}

	lowerError := strings.ToLower(err.Error())
	if strings.Contains(lowerError, "unique constraint failed") {
		return fmt.Errorf("%w: %v", ErrConflict, err)
	}

	return fmt.Errorf("credentials store: %w", err)
}