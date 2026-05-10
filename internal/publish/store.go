// Package publish contains publish target and build-to-publish binding
// persistence.
package publish

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"strings"
)

const (
	// KindFilesystem identifies the deterministic local publish target used for
	// first-pass validation flows.
	KindFilesystem = "filesystem"
)

var (
	// ErrInvalid reports validation failures on publish input.
	ErrInvalid = errors.New("invalid publish input")
	// ErrNotFound reports missing publish target records.
	ErrNotFound = errors.New("publish target not found")
	// ErrConflict reports uniqueness collisions for publish target records.
	ErrConflict = errors.New("publish target conflict")
	// ErrRepositoryNotFound reports publish target operations targeting an
	// unknown repository.
	ErrRepositoryNotFound = errors.New("publish target repository not found")
	// ErrCredentialsNotFound reports publish target operations referencing an
	// unknown credentials record.
	ErrCredentialsNotFound = errors.New("publish target credentials not found")
	// ErrBindingNotFound reports missing build-to-publish binding records.
	ErrBindingNotFound = errors.New("build publish binding not found")
	// ErrBindingConflict reports uniqueness collisions for binding records.
	ErrBindingConflict = errors.New("build publish binding conflict")
	// ErrBuildTargetNotFound reports bindings that reference an unknown build
	// target.
	ErrBuildTargetNotFound = errors.New("publish build target not found")
	// ErrBuildRunNotFound reports publish planning requests for an unknown build
	// run.
	ErrBuildRunNotFound = errors.New("publish build run not found")
	// ErrArtifactsNotFound reports publish planning requests for build runs that
	// do not yet have durable artifact records.
	ErrArtifactsNotFound = errors.New("publish artifacts not found")
	// ErrRepositoryMismatch reports attempts to bind records that belong to
	// different repositories.
	ErrRepositoryMismatch = errors.New("build publish binding repository mismatch")
)

const (
	// StatusQueued is the initial durable state for publish runs planned from a
	// successful build result.
	StatusQueued = "queued"
	// StatusRunning marks a publish run that is actively being executed.
	StatusRunning = "running"
	// StatusSucceeded marks a publish run that finished successfully.
	StatusSucceeded = "succeeded"
	// StatusFailed marks a publish run that finished with a terminal failure.
	StatusFailed = "failed"
	// StatusCanceled marks a publish run that was intentionally canceled.
	StatusCanceled = "canceled"
)

// Target is one durable publish target definition stored in SQLite.
type Target struct {
	ID            int64   `json:"id"`
	RepositoryID  int64   `json:"repository_id"`
	Name          string  `json:"name"`
	Kind          string  `json:"kind"`
	CredentialsID *int64  `json:"credentials_id,omitempty"`
	Enabled       bool    `json:"enabled"`
	ConfigJSON    string  `json:"config_json"`
	CreatedAt     string  `json:"created_at"`
	UpdatedAt     string  `json:"updated_at"`
}

// CreateTargetInput defines the fields accepted when a publish target is first
// registered.
type CreateTargetInput struct {
	RepositoryID  int64
	Name          string
	Kind          string
	CredentialsID *int64
	Enabled       *bool
	ConfigJSON    string
}

// UpdateTargetInput defines the fields accepted when a publish target is
// replaced.
type UpdateTargetInput struct {
	Name          string
	Kind          string
	CredentialsID *int64
	Enabled       bool
	ConfigJSON    string
}

// Binding is one durable build-to-publish mapping stored in SQLite.
type Binding struct {
	ID              int64  `json:"id"`
	BuildTargetID   int64  `json:"build_target_id"`
	PublishTargetID int64  `json:"publish_target_id"`
	Enabled         bool   `json:"enabled"`
	OptionsJSON     string `json:"options_json"`
	CreatedAt       string `json:"created_at"`
	UpdatedAt       string `json:"updated_at"`
}

// Run is one durable publish execution row stored in SQLite.
type Run struct {
	ID              int64   `json:"id"`
	ReleaseRunID    int64   `json:"release_run_id"`
	BuildRunID      int64   `json:"build_run_id"`
	PublishTargetID int64   `json:"publish_target_id"`
	ArtifactID      *int64  `json:"artifact_id,omitempty"`
	Status          string  `json:"status"`
	DestinationRef  *string `json:"destination_ref,omitempty"`
	StartedAt       *string `json:"started_at,omitempty"`
	FinishedAt      *string `json:"finished_at,omitempty"`
	ErrorMessage    *string `json:"error_message,omitempty"`
	CreatedAt       string  `json:"created_at"`
	UpdatedAt       string  `json:"updated_at"`
}

// CreateBindingInput defines the fields accepted when a binding is first
// declared.
type CreateBindingInput struct {
	BuildTargetID   int64
	PublishTargetID int64
	Enabled         *bool
	OptionsJSON     string
}

// UpdateBindingInput defines the fields accepted when a binding is replaced.
type UpdateBindingInput struct {
	Enabled     bool
	OptionsJSON string
}

// Store exposes publish target and binding operations needed by operator
// surfaces and later publish orchestration.
type Store interface {
	CreateTarget(ctx context.Context, input CreateTargetInput) (Target, error)
	GetTarget(ctx context.Context, id int64) (Target, error)
	ListTargetsByRepository(ctx context.Context, repositoryID int64) ([]Target, error)
	UpdateTarget(ctx context.Context, id int64, input UpdateTargetInput) (Target, error)
	DeleteTarget(ctx context.Context, id int64) error
	CreateBinding(ctx context.Context, input CreateBindingInput) (Binding, error)
	GetBinding(ctx context.Context, id int64) (Binding, error)
	ListBindingsByBuildTarget(ctx context.Context, buildTargetID int64) ([]Binding, error)
	UpdateBinding(ctx context.Context, id int64, input UpdateBindingInput) (Binding, error)
	DeleteBinding(ctx context.Context, id int64) error
	PlanBuildRun(ctx context.Context, buildRunID int64) error
	ListRunsByBuildRun(ctx context.Context, buildRunID int64) ([]Run, error)
}

// NewStore creates the SQLite-backed publish store.
func NewStore(database *sql.DB) Store {
	return &sqliteStore{database: database}
}

// sqliteStore persists publish targets, bindings, and publish runs in SQLite.
type sqliteStore struct {
	database *sql.DB
}

// normalizedTarget is the canonical validated form of publish target input.
type normalizedTarget struct {
	RepositoryID  int64
	Name          string
	Kind          string
	CredentialsID *int64
	Enabled       bool
	ConfigJSON    string
}

// normalizedBinding is the canonical validated form of build-to-publish
// binding input.
type normalizedBinding struct {
	BuildTargetID   int64
	PublishTargetID int64
	Enabled         bool
	OptionsJSON     string
}

// buildRunSummary holds the durable identifiers needed to plan publish runs
// from a completed build.
type buildRunSummary struct {
	ID           int64
	ReleaseRunID int64
	BuildTargetID int64
}

// CreateTarget inserts one publish target and returns the stored record.
func (s *sqliteStore) CreateTarget(ctx context.Context, input CreateTargetInput) (Target, error) {
	normalized, err := normalizeCreateTargetInput(input)
	if err != nil {
		return Target{}, err
	}

	repositoryExists, err := s.repositoryExists(ctx, normalized.RepositoryID)
	if err != nil {
		return Target{}, err
	}
	if !repositoryExists {
		return Target{}, ErrRepositoryNotFound
	}

	if normalized.CredentialsID != nil {
		credentialsExist, err := s.credentialsExists(ctx, *normalized.CredentialsID)
		if err != nil {
			return Target{}, err
		}
		if !credentialsExist {
			return Target{}, ErrCredentialsNotFound
		}
	}

	result, err := s.database.ExecContext(
		ctx,
		`INSERT INTO publish_targets (
			repository_id,
			name,
			kind,
			credentials_id,
			enabled,
			config_json
		) VALUES (?, ?, ?, ?, ?, ?)`,
		normalized.RepositoryID,
		normalized.Name,
		normalized.Kind,
		nullableInt64(normalized.CredentialsID),
		boolToSQLite(normalized.Enabled),
		normalized.ConfigJSON,
	)
	if err != nil {
		return Target{}, mapTargetSQLError(err)
	}

	id, err := result.LastInsertId()
	if err != nil {
		return Target{}, fmt.Errorf("read publish target id: %w", err)
	}

	return s.GetTarget(ctx, id)
}

// GetTarget loads one publish target by identifier.
func (s *sqliteStore) GetTarget(ctx context.Context, id int64) (Target, error) {
	row := s.database.QueryRowContext(
		ctx,
		`SELECT
			id,
			repository_id,
			name,
			kind,
			credentials_id,
			enabled,
			config_json,
			created_at,
			updated_at
		FROM publish_targets
		WHERE id = ?`,
		id,
	)

	target, err := scanTarget(row)
	if err != nil {
		if errors.Is(err, sql.ErrNoRows) {
			return Target{}, ErrNotFound
		}

		return Target{}, fmt.Errorf("query publish target %d: %w", id, err)
	}

	return target, nil
}

// ListTargetsByRepository returns publish targets for one repository ordered by
// id.
func (s *sqliteStore) ListTargetsByRepository(
	ctx context.Context,
	repositoryID int64,
) ([]Target, error) {
	if repositoryID <= 0 {
		return nil, fmt.Errorf("%w: repository_id must be greater than zero", ErrInvalid)
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
			kind,
			credentials_id,
			enabled,
			config_json,
			created_at,
			updated_at
		FROM publish_targets
		WHERE repository_id = ?
		ORDER BY id ASC`,
		repositoryID,
	)
	if err != nil {
		return nil, fmt.Errorf("list publish targets: %w", err)
	}
	defer rows.Close()

	targets := make([]Target, 0)
	for rows.Next() {
		target, err := scanTarget(rows)
		if err != nil {
			return nil, fmt.Errorf("scan publish target row: %w", err)
		}

		targets = append(targets, target)
	}

	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("iterate publish targets: %w", err)
	}

	return targets, nil
}

// UpdateTarget replaces the writable fields of an existing publish target.
func (s *sqliteStore) UpdateTarget(
	ctx context.Context,
	id int64,
	input UpdateTargetInput,
) (Target, error) {
	normalized, err := normalizeUpdateTargetInput(input)
	if err != nil {
		return Target{}, err
	}

	if normalized.CredentialsID != nil {
		credentialsExist, err := s.credentialsExists(ctx, *normalized.CredentialsID)
		if err != nil {
			return Target{}, err
		}
		if !credentialsExist {
			return Target{}, ErrCredentialsNotFound
		}
	}

	result, err := s.database.ExecContext(
		ctx,
		`UPDATE publish_targets
		SET
			name = ?,
			kind = ?,
			credentials_id = ?,
			enabled = ?,
			config_json = ?,
			updated_at = CURRENT_TIMESTAMP
		WHERE id = ?`,
		normalized.Name,
		normalized.Kind,
		nullableInt64(normalized.CredentialsID),
		boolToSQLite(normalized.Enabled),
		normalized.ConfigJSON,
		id,
	)
	if err != nil {
		return Target{}, mapTargetSQLError(err)
	}

	rowsAffected, err := result.RowsAffected()
	if err != nil {
		return Target{}, fmt.Errorf("read updated publish target rows: %w", err)
	}
	if rowsAffected == 0 {
		return Target{}, ErrNotFound
	}

	return s.GetTarget(ctx, id)
}

// DeleteTarget removes one publish target.
func (s *sqliteStore) DeleteTarget(ctx context.Context, id int64) error {
	result, err := s.database.ExecContext(
		ctx,
		`DELETE FROM publish_targets WHERE id = ?`,
		id,
	)
	if err != nil {
		return fmt.Errorf("delete publish target %d: %w", id, err)
	}

	rowsAffected, err := result.RowsAffected()
	if err != nil {
		return fmt.Errorf("read deleted publish target rows: %w", err)
	}
	if rowsAffected == 0 {
		return ErrNotFound
	}

	return nil
}

// CreateBinding inserts one build-to-publish binding and validates that both
// sides belong to the same repository.
func (s *sqliteStore) CreateBinding(
	ctx context.Context,
	input CreateBindingInput,
) (Binding, error) {
	normalized, err := normalizeCreateBindingInput(input)
	if err != nil {
		return Binding{}, err
	}

	buildRepositoryID, err := s.buildTargetRepositoryID(ctx, normalized.BuildTargetID)
	if err != nil {
		return Binding{}, err
	}

	publishRepositoryID, err := s.publishTargetRepositoryID(ctx, normalized.PublishTargetID)
	if err != nil {
		return Binding{}, err
	}

	if buildRepositoryID != publishRepositoryID {
		return Binding{}, ErrRepositoryMismatch
	}

	result, err := s.database.ExecContext(
		ctx,
		`INSERT INTO build_publish_bindings (
			build_target_id,
			publish_target_id,
			enabled,
			options_json
		) VALUES (?, ?, ?, ?)`,
		normalized.BuildTargetID,
		normalized.PublishTargetID,
		boolToSQLite(normalized.Enabled),
		normalized.OptionsJSON,
	)
	if err != nil {
		return Binding{}, mapBindingSQLError(err)
	}

	id, err := result.LastInsertId()
	if err != nil {
		return Binding{}, fmt.Errorf("read binding id: %w", err)
	}

	return s.GetBinding(ctx, id)
}

// GetBinding loads one build-to-publish binding by identifier.
func (s *sqliteStore) GetBinding(ctx context.Context, id int64) (Binding, error) {
	row := s.database.QueryRowContext(
		ctx,
		`SELECT
			id,
			build_target_id,
			publish_target_id,
			enabled,
			options_json,
			created_at,
			updated_at
		FROM build_publish_bindings
		WHERE id = ?`,
		id,
	)

	binding, err := scanBinding(row)
	if err != nil {
		if errors.Is(err, sql.ErrNoRows) {
			return Binding{}, ErrBindingNotFound
		}

		return Binding{}, fmt.Errorf("query build publish binding %d: %w", id, err)
	}

	return binding, nil
}

// ListBindingsByBuildTarget returns bindings for one build target ordered by
// id.
func (s *sqliteStore) ListBindingsByBuildTarget(
	ctx context.Context,
	buildTargetID int64,
) ([]Binding, error) {
	if buildTargetID <= 0 {
		return nil, fmt.Errorf("%w: build_target_id must be greater than zero", ErrInvalid)
	}

	if _, err := s.buildTargetRepositoryID(ctx, buildTargetID); err != nil {
		return nil, err
	}

	rows, err := s.database.QueryContext(
		ctx,
		`SELECT
			id,
			build_target_id,
			publish_target_id,
			enabled,
			options_json,
			created_at,
			updated_at
		FROM build_publish_bindings
		WHERE build_target_id = ?
		ORDER BY id ASC`,
		buildTargetID,
	)
	if err != nil {
		return nil, fmt.Errorf("list build publish bindings: %w", err)
	}
	defer rows.Close()

	bindings := make([]Binding, 0)
	for rows.Next() {
		binding, err := scanBinding(rows)
		if err != nil {
			return nil, fmt.Errorf("scan build publish binding row: %w", err)
		}

		bindings = append(bindings, binding)
	}

	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("iterate build publish bindings: %w", err)
	}

	return bindings, nil
}

// UpdateBinding replaces the writable fields of an existing binding.
func (s *sqliteStore) UpdateBinding(
	ctx context.Context,
	id int64,
	input UpdateBindingInput,
) (Binding, error) {
	normalized, err := normalizeUpdateBindingInput(input)
	if err != nil {
		return Binding{}, err
	}

	result, err := s.database.ExecContext(
		ctx,
		`UPDATE build_publish_bindings
		SET
			enabled = ?,
			options_json = ?,
			updated_at = CURRENT_TIMESTAMP
		WHERE id = ?`,
		boolToSQLite(normalized.Enabled),
		normalized.OptionsJSON,
		id,
	)
	if err != nil {
		return Binding{}, mapBindingSQLError(err)
	}

	rowsAffected, err := result.RowsAffected()
	if err != nil {
		return Binding{}, fmt.Errorf("read updated binding rows: %w", err)
	}
	if rowsAffected == 0 {
		return Binding{}, ErrBindingNotFound
	}

	return s.GetBinding(ctx, id)
}

// DeleteBinding removes one build-to-publish binding.
func (s *sqliteStore) DeleteBinding(ctx context.Context, id int64) error {
	result, err := s.database.ExecContext(
		ctx,
		`DELETE FROM build_publish_bindings WHERE id = ?`,
		id,
	)
	if err != nil {
		return fmt.Errorf("delete build publish binding %d: %w", id, err)
	}

	rowsAffected, err := result.RowsAffected()
	if err != nil {
		return fmt.Errorf("read deleted binding rows: %w", err)
	}
	if rowsAffected == 0 {
		return ErrBindingNotFound
	}

	return nil
}

// PlanBuildRun expands one build result into queued publish runs for each
// enabled binding and durable artifact recorded on the build run.
func (s *sqliteStore) PlanBuildRun(ctx context.Context, buildRunID int64) error {
	if buildRunID <= 0 {
		return fmt.Errorf("%w: build_run_id must be greater than zero", ErrInvalid)
	}

	tx, err := s.database.BeginTx(ctx, nil)
	if err != nil {
		return fmt.Errorf("begin publish planning transaction: %w", err)
	}
	defer tx.Rollback()

	summary, err := s.getBuildRunSummary(ctx, tx, buildRunID)
	if err != nil {
		return err
	}

	artifactIDs, err := s.artifactIDsByBuildRun(ctx, tx, buildRunID)
	if err != nil {
		return err
	}
	if len(artifactIDs) == 0 {
		return fmt.Errorf("%w: build run %d has no registered artifacts", ErrArtifactsNotFound, buildRunID)
	}

	publishTargetIDs, err := s.enabledPublishTargetIDsByBuildTarget(ctx, tx, summary.BuildTargetID)
	if err != nil {
		return err
	}
	if len(publishTargetIDs) == 0 {
		return tx.Commit()
	}

	existing, err := s.existingPublishRunKeys(ctx, tx, buildRunID)
	if err != nil {
		return err
	}

	for _, publishTargetID := range publishTargetIDs {
		for _, artifactID := range artifactIDs {
			key := publishRunKey(publishTargetID, artifactID)
			if _, ok := existing[key]; ok {
				continue
			}

			if _, err := tx.ExecContext(
				ctx,
				`INSERT INTO publish_runs (
					release_run_id,
					build_run_id,
					publish_target_id,
					artifact_id,
					status
				) VALUES (?, ?, ?, ?, ?)`,
				summary.ReleaseRunID,
				summary.ID,
				publishTargetID,
				artifactID,
				StatusQueued,
			); err != nil {
				return fmt.Errorf(
					"insert publish run for build run %d target %d artifact %d: %w",
					buildRunID,
					publishTargetID,
					artifactID,
					err,
				)
			}
		}
	}

	if err := tx.Commit(); err != nil {
		return fmt.Errorf("commit publish planning transaction: %w", err)
	}

	return nil
}

// ListRunsByBuildRun returns durable publish runs for one build run ordered by
// publish target and artifact id.
func (s *sqliteStore) ListRunsByBuildRun(ctx context.Context, buildRunID int64) ([]Run, error) {
	if buildRunID <= 0 {
		return nil, fmt.Errorf("%w: build_run_id must be greater than zero", ErrInvalid)
	}

	if _, err := s.getBuildRunSummary(ctx, s.database, buildRunID); err != nil {
		return nil, err
	}

	rows, err := s.database.QueryContext(
		ctx,
		`SELECT
			id,
			release_run_id,
			build_run_id,
			publish_target_id,
			artifact_id,
			status,
			destination_ref,
			started_at,
			finished_at,
			error_message,
			created_at,
			updated_at
		FROM publish_runs
		WHERE build_run_id = ?
		ORDER BY publish_target_id ASC, artifact_id ASC, id ASC`,
		buildRunID,
	)
	if err != nil {
		return nil, fmt.Errorf("list publish runs for build run %d: %w", buildRunID, err)
	}
	defer rows.Close()

	runs := make([]Run, 0)
	for rows.Next() {
		run, err := scanRun(rows)
		if err != nil {
			return nil, fmt.Errorf("scan publish run row: %w", err)
		}

		runs = append(runs, run)
	}

	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("iterate publish runs: %w", err)
	}

	return runs, nil
}

// scanner abstracts row scanners shared by single-row and multi-row queries.
type scanner interface {
	Scan(dest ...any) error
}

// scanTarget decodes one publish target row into the public target model.
func scanTarget(row scanner) (Target, error) {
	var target Target
	var credentialsID sql.NullInt64
	var enabled int64

	err := row.Scan(
		&target.ID,
		&target.RepositoryID,
		&target.Name,
		&target.Kind,
		&credentialsID,
		&enabled,
		&target.ConfigJSON,
		&target.CreatedAt,
		&target.UpdatedAt,
	)
	if err != nil {
		return Target{}, err
	}

	target.CredentialsID = int64Pointer(credentialsID)
	target.Enabled = enabled == 1

	return target, nil
}

// scanBinding decodes one build-to-publish binding row into the public binding
// model.
func scanBinding(row scanner) (Binding, error) {
	var binding Binding
	var enabled int64

	err := row.Scan(
		&binding.ID,
		&binding.BuildTargetID,
		&binding.PublishTargetID,
		&enabled,
		&binding.OptionsJSON,
		&binding.CreatedAt,
		&binding.UpdatedAt,
	)
	if err != nil {
		return Binding{}, err
	}

	binding.Enabled = enabled == 1

	return binding, nil
}

// scanRun decodes one publish run row into the public run model.
func scanRun(row scanner) (Run, error) {
	var run Run
	var artifactID sql.NullInt64
	var destinationRef sql.NullString
	var startedAt sql.NullString
	var finishedAt sql.NullString
	var errorMessage sql.NullString

	err := row.Scan(
		&run.ID,
		&run.ReleaseRunID,
		&run.BuildRunID,
		&run.PublishTargetID,
		&artifactID,
		&run.Status,
		&destinationRef,
		&startedAt,
		&finishedAt,
		&errorMessage,
		&run.CreatedAt,
		&run.UpdatedAt,
	)
	if err != nil {
		return Run{}, err
	}

	run.ArtifactID = int64Pointer(artifactID)
	run.DestinationRef = stringPointer(destinationRef)
	run.StartedAt = stringPointer(startedAt)
	run.FinishedAt = stringPointer(finishedAt)
	run.ErrorMessage = stringPointer(errorMessage)

	return run, nil
}

// normalizeCreateTargetInput converts create input into the validated publish
// target form stored by the publish store.
func normalizeCreateTargetInput(input CreateTargetInput) (normalizedTarget, error) {
	enabled := true
	if input.Enabled != nil {
		enabled = *input.Enabled
	}

	return normalizeTargetInput(normalizedTarget{
		RepositoryID:  input.RepositoryID,
		Name:          input.Name,
		Kind:          input.Kind,
		CredentialsID: input.CredentialsID,
		Enabled:       enabled,
		ConfigJSON:    input.ConfigJSON,
	})
}

// normalizeUpdateTargetInput converts update input into the validated publish
// target form stored by the publish store.
func normalizeUpdateTargetInput(input UpdateTargetInput) (normalizedTarget, error) {
	return normalizeTargetInput(normalizedTarget{
		Name:          input.Name,
		Kind:          input.Kind,
		CredentialsID: input.CredentialsID,
		Enabled:       input.Enabled,
		ConfigJSON:    input.ConfigJSON,
	})
}

// normalizeTargetInput trims and validates one publish target payload.
func normalizeTargetInput(input normalizedTarget) (normalizedTarget, error) {
	input.Name = strings.TrimSpace(input.Name)
	input.Kind = strings.ToLower(strings.TrimSpace(input.Kind))

	if input.RepositoryID < 0 {
		return normalizedTarget{}, fmt.Errorf("%w: repository_id must not be negative", ErrInvalid)
	}
	if input.Name == "" {
		return normalizedTarget{}, fmt.Errorf("%w: name must not be empty", ErrInvalid)
	}
	if input.Kind == "" {
		return normalizedTarget{}, fmt.Errorf("%w: kind must not be empty", ErrInvalid)
	}

	configJSON, err := normalizeJSONObject(input.ConfigJSON, "config_json")
	if err != nil {
		return normalizedTarget{}, err
	}
	input.ConfigJSON = configJSON

	return input, nil
}

// normalizeCreateBindingInput converts create input into the validated binding
// form stored by the publish store.
func normalizeCreateBindingInput(input CreateBindingInput) (normalizedBinding, error) {
	enabled := true
	if input.Enabled != nil {
		enabled = *input.Enabled
	}

	normalized, err := normalizeBindingUpdateInput(normalizedBinding{
		BuildTargetID:   input.BuildTargetID,
		PublishTargetID: input.PublishTargetID,
		Enabled:         enabled,
		OptionsJSON:     input.OptionsJSON,
	})
	if err != nil {
		return normalizedBinding{}, err
	}

	if normalized.BuildTargetID <= 0 {
		return normalizedBinding{}, fmt.Errorf("%w: build_target_id must be greater than zero", ErrInvalid)
	}
	if normalized.PublishTargetID <= 0 {
		return normalizedBinding{}, fmt.Errorf("%w: publish_target_id must be greater than zero", ErrInvalid)
	}

	return normalized, nil
}

// normalizeUpdateBindingInput converts update input into the validated binding
// form stored by the publish store.
func normalizeUpdateBindingInput(input UpdateBindingInput) (normalizedBinding, error) {
	return normalizeBindingUpdateInput(normalizedBinding{
		Enabled:     input.Enabled,
		OptionsJSON: input.OptionsJSON,
	})
}

// normalizeBindingUpdateInput trims and validates one publish binding payload.
func normalizeBindingUpdateInput(input normalizedBinding) (normalizedBinding, error) {
	if input.BuildTargetID < 0 {
		return normalizedBinding{}, fmt.Errorf("%w: build_target_id must not be negative", ErrInvalid)
	}
	if input.PublishTargetID < 0 {
		return normalizedBinding{}, fmt.Errorf("%w: publish_target_id must not be negative", ErrInvalid)
	}

	optionsJSON, err := normalizeJSONObject(input.OptionsJSON, "options_json")
	if err != nil {
		return normalizedBinding{}, err
	}
	input.OptionsJSON = optionsJSON

	return input, nil
}

// normalizeJSONObject validates publish config payloads as canonical JSON
// objects.
func normalizeJSONObject(raw string, fieldName string) (string, error) {
	trimmed := strings.TrimSpace(raw)
	if trimmed == "" {
		return `{}`, nil
	}

	var decoded any
	if err := json.Unmarshal([]byte(trimmed), &decoded); err != nil {
		return "", fmt.Errorf("%w: %s must be valid JSON: %v", ErrInvalid, fieldName, err)
	}

	if _, ok := decoded.(map[string]any); !ok {
		return "", fmt.Errorf("%w: %s must be a JSON object", ErrInvalid, fieldName)
	}

	canonical, err := json.Marshal(decoded)
	if err != nil {
		return "", fmt.Errorf("normalize %s: %w", fieldName, err)
	}

	return string(canonical), nil
}

// repositoryExists checks whether the referenced repository row exists.
func (s *sqliteStore) repositoryExists(ctx context.Context, repositoryID int64) (bool, error) {
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

// credentialsExists checks whether the referenced credentials row exists.
func (s *sqliteStore) credentialsExists(ctx context.Context, credentialsID int64) (bool, error) {
	var count int
	if err := s.database.QueryRowContext(
		ctx,
		`SELECT COUNT(1) FROM credentials WHERE id = ?`,
		credentialsID,
	).Scan(&count); err != nil {
		return false, fmt.Errorf("query credentials %d: %w", credentialsID, err)
	}

	return count > 0, nil
}

// buildTargetRepositoryID resolves the owning repository for one build target.
func (s *sqliteStore) buildTargetRepositoryID(ctx context.Context, buildTargetID int64) (int64, error) {
	var repositoryID int64
	if err := s.database.QueryRowContext(
		ctx,
		`SELECT repository_id FROM build_targets WHERE id = ?`,
		buildTargetID,
	).Scan(&repositoryID); err != nil {
		if errors.Is(err, sql.ErrNoRows) {
			return 0, ErrBuildTargetNotFound
		}

		return 0, fmt.Errorf("query build target %d for binding: %w", buildTargetID, err)
	}

	return repositoryID, nil
}

// publishTargetRepositoryID resolves the owning repository for one publish
// target.
func (s *sqliteStore) publishTargetRepositoryID(ctx context.Context, publishTargetID int64) (int64, error) {
	var repositoryID int64
	if err := s.database.QueryRowContext(
		ctx,
		`SELECT repository_id FROM publish_targets WHERE id = ?`,
		publishTargetID,
	).Scan(&repositoryID); err != nil {
		if errors.Is(err, sql.ErrNoRows) {
			return 0, ErrNotFound
		}

		return 0, fmt.Errorf("query publish target %d for binding: %w", publishTargetID, err)
	}

	return repositoryID, nil
}

// queryer abstracts the subset of SQL query methods shared by database
// handles and transactions.
type queryer interface {
	ExecContext(context.Context, string, ...any) (sql.Result, error)
	QueryContext(context.Context, string, ...any) (*sql.Rows, error)
	QueryRowContext(context.Context, string, ...any) *sql.Row
}

// getBuildRunSummary loads the durable identifiers required to plan publish
// runs for one build result.
func (s *sqliteStore) getBuildRunSummary(
	ctx context.Context,
	query queryer,
	buildRunID int64,
) (buildRunSummary, error) {
	row := query.QueryRowContext(
		ctx,
		`SELECT id, release_run_id, build_target_id FROM build_runs WHERE id = ?`,
		buildRunID,
	)

	var summary buildRunSummary
	if err := row.Scan(&summary.ID, &summary.ReleaseRunID, &summary.BuildTargetID); err != nil {
		if errors.Is(err, sql.ErrNoRows) {
			return buildRunSummary{}, ErrBuildRunNotFound
		}

		return buildRunSummary{}, fmt.Errorf("query build run %d for publish planning: %w", buildRunID, err)
	}

	return summary, nil
}

// artifactIDsByBuildRun lists the persisted artifact identifiers for one build
// run.
func (s *sqliteStore) artifactIDsByBuildRun(
	ctx context.Context,
	query queryer,
	buildRunID int64,
) ([]int64, error) {
	rows, err := query.QueryContext(
		ctx,
		`SELECT id FROM artifacts WHERE build_run_id = ? ORDER BY id ASC`,
		buildRunID,
	)
	if err != nil {
		return nil, fmt.Errorf("list artifacts for build run %d: %w", buildRunID, err)
	}
	defer rows.Close()

	artifactIDs := make([]int64, 0)
	for rows.Next() {
		var artifactID int64
		if err := rows.Scan(&artifactID); err != nil {
			return nil, fmt.Errorf("scan artifact id for build run %d: %w", buildRunID, err)
		}

		artifactIDs = append(artifactIDs, artifactID)
	}

	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("iterate artifact ids for build run %d: %w", buildRunID, err)
	}

	return artifactIDs, nil
}

// enabledPublishTargetIDsByBuildTarget lists enabled publish targets reachable
// from one build target through enabled bindings.
func (s *sqliteStore) enabledPublishTargetIDsByBuildTarget(
	ctx context.Context,
	query queryer,
	buildTargetID int64,
) ([]int64, error) {
	rows, err := query.QueryContext(
		ctx,
		`SELECT b.publish_target_id
		FROM build_publish_bindings b
		JOIN publish_targets pt ON pt.id = b.publish_target_id
		WHERE b.build_target_id = ? AND b.enabled = 1 AND pt.enabled = 1
		ORDER BY b.publish_target_id ASC`,
		buildTargetID,
	)
	if err != nil {
		return nil, fmt.Errorf("list publish targets for build target %d: %w", buildTargetID, err)
	}
	defer rows.Close()

	publishTargetIDs := make([]int64, 0)
	for rows.Next() {
		var publishTargetID int64
		if err := rows.Scan(&publishTargetID); err != nil {
			return nil, fmt.Errorf("scan publish target id for build target %d: %w", buildTargetID, err)
		}

		publishTargetIDs = append(publishTargetIDs, publishTargetID)
	}

	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("iterate publish target ids for build target %d: %w", buildTargetID, err)
	}

	return publishTargetIDs, nil
}

// existingPublishRunKeys returns the publish target and artifact pairs already
// materialized for one build run.
func (s *sqliteStore) existingPublishRunKeys(
	ctx context.Context,
	query queryer,
	buildRunID int64,
) (map[string]struct{}, error) {
	rows, err := query.QueryContext(
		ctx,
		`SELECT publish_target_id, artifact_id FROM publish_runs WHERE build_run_id = ?`,
		buildRunID,
	)
	if err != nil {
		return nil, fmt.Errorf("list existing publish runs for build run %d: %w", buildRunID, err)
	}
	defer rows.Close()

	existing := make(map[string]struct{})
	for rows.Next() {
		var publishTargetID int64
		var artifactID sql.NullInt64
		if err := rows.Scan(&publishTargetID, &artifactID); err != nil {
			return nil, fmt.Errorf("scan existing publish run key for build run %d: %w", buildRunID, err)
		}

		existing[publishRunKey(publishTargetID, artifactID.Int64)] = struct{}{}
	}

	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("iterate existing publish runs for build run %d: %w", buildRunID, err)
	}

	return existing, nil
}

// publishRunKey builds the composite key used to detect duplicate publish run
// materialization.
func publishRunKey(publishTargetID int64, artifactID int64) string {
	return fmt.Sprintf("%d:%d", publishTargetID, artifactID)
}

// mapTargetSQLError translates common SQLite constraint failures into
// publish-target domain errors.
func mapTargetSQLError(err error) error {
	if err == nil {
		return nil
	}

	lowerError := strings.ToLower(err.Error())
	if strings.Contains(lowerError, "unique constraint failed") {
		return fmt.Errorf("%w: %v", ErrConflict, err)
	}

	return fmt.Errorf("publish target store: %w", err)
}

// mapBindingSQLError translates common SQLite constraint failures into
// binding-domain errors.
func mapBindingSQLError(err error) error {
	if err == nil {
		return nil
	}

	lowerError := strings.ToLower(err.Error())
	if strings.Contains(lowerError, "unique constraint failed") {
		return fmt.Errorf("%w: %v", ErrBindingConflict, err)
	}

	return fmt.Errorf("publish binding store: %w", err)
}

// stringPointer converts a nullable SQL string into an optional Go pointer.
func stringPointer(value sql.NullString) *string {
	if !value.Valid {
		return nil
	}

	copy := value.String
	return &copy
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

// int64Pointer converts a nullable SQL integer into an optional Go pointer.
func int64Pointer(value sql.NullInt64) *int64 {
	if !value.Valid {
		return nil
	}

	copy := value.Int64
	return &copy
}