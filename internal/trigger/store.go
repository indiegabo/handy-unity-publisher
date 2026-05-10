// Package trigger contains trigger-rule persistence for release creation
// sources.
package trigger

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"strings"
)

const (
	// SourceManual marks trigger rules initiated by explicit operator action.
	SourceManual = "manual"
	// SourcePoll marks trigger rules driven by repository polling.
	SourcePoll = "poll"
	// SourceWebhook marks trigger rules driven by external callbacks.
	SourceWebhook = "webhook"
)

var (
	// ErrInvalid reports validation failures on trigger-rule input.
	ErrInvalid = errors.New("invalid trigger rule input")
	// ErrNotFound reports missing trigger-rule records.
	ErrNotFound = errors.New("trigger rule not found")
	// ErrConflict reports uniqueness collisions for trigger-rule records.
	ErrConflict = errors.New("trigger rule conflict")
	// ErrRepositoryNotFound reports trigger-rule operations targeting an unknown
	// repository.
	ErrRepositoryNotFound = errors.New("trigger rule repository not found")
)

// Rule is the durable trigger-rule record stored in SQLite.
type Rule struct {
	ID           int64  `json:"id"`
	RepositoryID int64  `json:"repository_id"`
	Name         string `json:"name"`
	Source       string `json:"source"`
	Enabled      bool   `json:"enabled"`
	ConfigJSON   string `json:"config_json"`
	CreatedAt    string `json:"created_at"`
	UpdatedAt    string `json:"updated_at"`
}

// CreateInput defines the fields accepted when a trigger rule is first
// declared.
type CreateInput struct {
	RepositoryID int64
	Name         string
	Source       string
	Enabled      *bool
	ConfigJSON   string
}

// UpdateInput defines the fields accepted when a trigger rule is replaced.
type UpdateInput struct {
	Name       string
	Source     string
	Enabled    bool
	ConfigJSON string
}

// Store exposes the trigger-rule operations currently needed by operator
// surfaces.
type Store interface {
	Create(ctx context.Context, input CreateInput) (Rule, error)
	Get(ctx context.Context, id int64) (Rule, error)
	ListEnabledBySource(ctx context.Context, source string) ([]Rule, error)
	ListByRepository(ctx context.Context, repositoryID int64) ([]Rule, error)
	Update(ctx context.Context, id int64, input UpdateInput) (Rule, error)
	Delete(ctx context.Context, id int64) error
}

// NewStore creates the SQLite-backed trigger-rule store.
func NewStore(database *sql.DB) Store {
	return &sqliteStore{database: database}
}

// sqliteStore persists trigger rules in SQLite.
type sqliteStore struct {
	database *sql.DB
}

// normalizedInput is the canonical validated form of trigger-rule input.
type normalizedInput struct {
	RepositoryID int64
	Name         string
	Source       string
	Enabled      bool
	ConfigJSON   string
}

// Create inserts a trigger rule and returns the stored record.
func (s *sqliteStore) Create(ctx context.Context, input CreateInput) (Rule, error) {
	normalized, err := normalizeCreateInput(input)
	if err != nil {
		return Rule{}, err
	}

	repositoryExists, err := s.repositoryExists(ctx, normalized.RepositoryID)
	if err != nil {
		return Rule{}, err
	}

	if !repositoryExists {
		return Rule{}, ErrRepositoryNotFound
	}

	result, err := s.database.ExecContext(
		ctx,
		`INSERT INTO trigger_rules (
			repository_id,
			name,
			source,
			enabled,
			config_json
		) VALUES (?, ?, ?, ?, ?)`,
		normalized.RepositoryID,
		normalized.Name,
		normalized.Source,
		boolToSQLite(normalized.Enabled),
		normalized.ConfigJSON,
	)
	if err != nil {
		return Rule{}, mapSQLError(err)
	}

	id, err := result.LastInsertId()
	if err != nil {
		return Rule{}, fmt.Errorf("read trigger rule id: %w", err)
	}

	return s.Get(ctx, id)
}

// Get loads a trigger rule by identifier.
func (s *sqliteStore) Get(ctx context.Context, id int64) (Rule, error) {
	row := s.database.QueryRowContext(
		ctx,
		`SELECT
			id,
			repository_id,
			name,
			source,
			enabled,
			config_json,
			created_at,
			updated_at
		FROM trigger_rules
		WHERE id = ?`,
		id,
	)

	rule, err := scanRule(row)
	if err != nil {
		if errors.Is(err, sql.ErrNoRows) {
			return Rule{}, ErrNotFound
		}

		return Rule{}, fmt.Errorf("query trigger rule %d: %w", id, err)
	}

	return rule, nil
}

// ListEnabledBySource returns enabled trigger rules for one source ordered by
// identifier so one-shot schedulers can evaluate them deterministically.
func (s *sqliteStore) ListEnabledBySource(
	ctx context.Context,
	source string,
) ([]Rule, error) {
	source = strings.TrimSpace(strings.ToLower(source))
	if source == "" {
		return nil, fmt.Errorf("%w: source must not be empty", ErrInvalid)
	}

	switch source {
	case SourceManual, SourcePoll, SourceWebhook:
	default:
		return nil, fmt.Errorf(
			"%w: source must be one of %q, %q, or %q",
			ErrInvalid,
			SourceManual,
			SourcePoll,
			SourceWebhook,
		)
	}

	rows, err := s.database.QueryContext(
		ctx,
		`SELECT
			id,
			repository_id,
			name,
			source,
			enabled,
			config_json,
			created_at,
			updated_at
		FROM trigger_rules
		WHERE source = ? AND enabled = 1
		ORDER BY id ASC`,
		source,
	)
	if err != nil {
		return nil, fmt.Errorf("list trigger rules by source: %w", err)
	}
	defer rows.Close()

	rules := make([]Rule, 0)
	for rows.Next() {
		rule, err := scanRule(rows)
		if err != nil {
			return nil, fmt.Errorf("scan trigger rule row: %w", err)
		}

		rules = append(rules, rule)
	}

	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("iterate trigger rules: %w", err)
	}

	return rules, nil
}

// ListByRepository returns the trigger rules for one repository ordered by id.
func (s *sqliteStore) ListByRepository(
	ctx context.Context,
	repositoryID int64,
) ([]Rule, error) {
	if repositoryID <= 0 {
		return nil, fmt.Errorf(
			"%w: repository_id must be greater than zero",
			ErrInvalid,
		)
	}

	repositoryExists, err := s.repositoryExists(ctx, repositoryID)
	if err != nil {
		return nil, err
	}

	if !repositoryExists {
		return nil, ErrRepositoryNotFound
	}

	rows, err := s.database.QueryContext(
		ctx,
		`SELECT
			id,
			repository_id,
			name,
			source,
			enabled,
			config_json,
			created_at,
			updated_at
		FROM trigger_rules
		WHERE repository_id = ?
		ORDER BY id ASC`,
		repositoryID,
	)
	if err != nil {
		return nil, fmt.Errorf("list trigger rules: %w", err)
	}
	defer rows.Close()

	rules := make([]Rule, 0)
	for rows.Next() {
		rule, err := scanRule(rows)
		if err != nil {
			return nil, fmt.Errorf("scan trigger rule row: %w", err)
		}

		rules = append(rules, rule)
	}

	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("iterate trigger rules: %w", err)
	}

	return rules, nil
}

// Update replaces the writable fields of an existing trigger rule.
func (s *sqliteStore) Update(ctx context.Context, id int64, input UpdateInput) (Rule, error) {
	normalized, err := normalizeUpdateInput(input)
	if err != nil {
		return Rule{}, err
	}

	result, err := s.database.ExecContext(
		ctx,
		`UPDATE trigger_rules
		SET
			name = ?,
			source = ?,
			enabled = ?,
			config_json = ?,
			updated_at = CURRENT_TIMESTAMP
		WHERE id = ?`,
		normalized.Name,
		normalized.Source,
		boolToSQLite(normalized.Enabled),
		normalized.ConfigJSON,
		id,
	)
	if err != nil {
		return Rule{}, mapSQLError(err)
	}

	rowsAffected, err := result.RowsAffected()
	if err != nil {
		return Rule{}, fmt.Errorf("read updated trigger rows: %w", err)
	}

	if rowsAffected == 0 {
		return Rule{}, ErrNotFound
	}

	return s.Get(ctx, id)
}

// Delete removes a trigger rule.
func (s *sqliteStore) Delete(ctx context.Context, id int64) error {
	result, err := s.database.ExecContext(
		ctx,
		`DELETE FROM trigger_rules WHERE id = ?`,
		id,
	)
	if err != nil {
		return fmt.Errorf("delete trigger rule %d: %w", id, err)
	}

	rowsAffected, err := result.RowsAffected()
	if err != nil {
		return fmt.Errorf("read deleted trigger rows: %w", err)
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

// scanRule decodes one trigger-rule row into the public trigger model.
func scanRule(row scanner) (Rule, error) {
	var rule Rule
	var enabled int64

	err := row.Scan(
		&rule.ID,
		&rule.RepositoryID,
		&rule.Name,
		&rule.Source,
		&enabled,
		&rule.ConfigJSON,
		&rule.CreatedAt,
		&rule.UpdatedAt,
	)
	if err != nil {
		return Rule{}, err
	}

	rule.Enabled = enabled == 1
	return rule, nil
}

// normalizeCreateInput converts create input into the validated trigger-rule
// payload stored by the trigger store.
func normalizeCreateInput(input CreateInput) (normalizedInput, error) {
	enabled := true
	if input.Enabled != nil {
		enabled = *input.Enabled
	}

	return normalizeInput(normalizedInput{
		RepositoryID: input.RepositoryID,
		Name:         input.Name,
		Source:       input.Source,
		Enabled:      enabled,
		ConfigJSON:   input.ConfigJSON,
	})
}

// normalizeUpdateInput converts update input into the validated trigger-rule
// payload stored by the trigger store.
func normalizeUpdateInput(input UpdateInput) (normalizedInput, error) {
	return normalizeInput(normalizedInput{
		Name:       input.Name,
		Source:     input.Source,
		Enabled:    input.Enabled,
		ConfigJSON: input.ConfigJSON,
	})
}

// normalizeInput trims and validates one trigger-rule payload.
func normalizeInput(input normalizedInput) (normalizedInput, error) {
	input.Name = strings.TrimSpace(input.Name)
	input.Source = strings.TrimSpace(strings.ToLower(input.Source))

	if input.RepositoryID < 0 {
		return normalizedInput{}, fmt.Errorf(
			"%w: repository_id must not be negative",
			ErrInvalid,
		)
	}

	if input.Name == "" {
		return normalizedInput{}, fmt.Errorf("%w: name must not be empty", ErrInvalid)
	}

	switch input.Source {
	case SourceManual, SourcePoll, SourceWebhook:
	default:
		return normalizedInput{}, fmt.Errorf(
			"%w: source must be one of %q, %q, or %q",
			ErrInvalid,
			SourceManual,
			SourcePoll,
			SourceWebhook,
		)
	}

	configJSON, err := normalizeConfigJSON(input.ConfigJSON)
	if err != nil {
		return normalizedInput{}, err
	}
	input.ConfigJSON = configJSON

	return input, nil
}

// normalizeConfigJSON validates trigger config payloads as canonical JSON
// objects.
func normalizeConfigJSON(raw string) (string, error) {
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

// repositoryExists checks whether the referenced repository row exists.
func (s *sqliteStore) repositoryExists(
	ctx context.Context,
	repositoryID int64,
) (bool, error) {
	var count int
	if err := s.database.QueryRowContext(
		ctx,
		`SELECT COUNT(1) FROM repositories WHERE id = ?`,
		repositoryID,
	).Scan(&count); err != nil {
		return false, fmt.Errorf("query repository %d: %w", repositoryID, err)
	}

	return count > 0, nil
}

// mapSQLError translates common SQLite constraint failures into trigger-domain
// errors.
func mapSQLError(err error) error {
	if err == nil {
		return nil
	}

	lowerError := strings.ToLower(err.Error())
	if strings.Contains(lowerError, "unique constraint failed") {
		return fmt.Errorf("%w: %v", ErrConflict, err)
	}

	if strings.Contains(lowerError, "foreign key constraint failed") {
		return fmt.Errorf("%w: %v", ErrRepositoryNotFound, err)
	}

	return fmt.Errorf("trigger rule store: %w", err)
}

// boolToSQLite converts one boolean into the integer representation used by
// SQLite tables.
func boolToSQLite(value bool) int {
	if value {
		return 1
	}

	return 0
}