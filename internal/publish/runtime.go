package publish

import (
	"context"
	"database/sql"
	"errors"
	"fmt"
	"path/filepath"
	"strings"
)

var (
	// ErrRunNotFound reports missing publish run records.
	ErrRunNotFound = errors.New("publish run not found")
	// ErrRunNotQueued reports attempts to claim publish runs that are no longer
	// queued.
	ErrRunNotQueued = errors.New("publish run not queued")
	// ErrRunNotRunning reports terminal updates for publish runs that were not
	// first claimed into the running state.
	ErrRunNotRunning = errors.New("publish run not running")
)

// ExecutionPlan joins the durable metadata required to execute one publish run.
type ExecutionPlan struct {
	PublishRunID           int64  `json:"publish_run_id"`
	ReleaseRunID           int64  `json:"release_run_id"`
	RepositoryID           int64  `json:"repository_id"`
	RepositoryName         string `json:"repository_name"`
	GitTag                 string `json:"git_tag"`
	BuildRunID             int64  `json:"build_run_id"`
	PublishTargetID        int64  `json:"publish_target_id"`
	PublishTargetName      string `json:"publish_target_name"`
	PublishTargetKind      string `json:"publish_target_kind"`
	PublishTargetConfigJSON string `json:"publish_target_config_json"`
	ArtifactID             int64  `json:"artifact_id"`
	ArtifactName           string `json:"artifact_name"`
	ArtifactKind           string `json:"artifact_kind"`
	ArtifactPath           string `json:"artifact_path"`
	ArtifactRootPath       string `json:"artifact_root_path"`
	SourcePath             string `json:"source_path"`
	Status                 string `json:"status"`
}

// StartRunInput defines the optional fields accepted when a publish run moves
// from queued to running.
type StartRunInput struct{}

// CompleteRunInput defines the fields persisted when a publish run succeeds.
type CompleteRunInput struct {
	DestinationRef string
}

// FailRunInput defines the fields persisted when a publish run fails.
type FailRunInput struct {
	DestinationRef string
	ErrorMessage   string
}

// ExecutionStore exposes the publish-run execution operations needed by the
// publish worker runtime.
type ExecutionStore interface {
	GetRun(ctx context.Context, id int64) (Run, error)
	GetExecutionPlan(ctx context.Context, publishRunID int64) (ExecutionPlan, error)
	StartRun(ctx context.Context, id int64, input StartRunInput) (Run, error)
	CompleteRun(ctx context.Context, id int64, input CompleteRunInput) (Run, error)
	FailRun(ctx context.Context, id int64, input FailRunInput) (Run, error)
}

// NewExecutionStore creates the SQLite-backed publish execution store.
func NewExecutionStore(database *sql.DB) ExecutionStore {
	return &sqliteStore{database: database}
}

// GetRun loads one publish run by identifier.
func (s *sqliteStore) GetRun(ctx context.Context, id int64) (Run, error) {
	row := s.database.QueryRowContext(
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
		WHERE id = ?`,
		id,
	)

	run, err := scanRun(row)
	if err != nil {
		if errors.Is(err, sql.ErrNoRows) {
			return Run{}, ErrRunNotFound
		}

		return Run{}, fmt.Errorf("query publish run %d: %w", id, err)
	}

	return run, nil
}

// GetExecutionPlan loads the joined metadata required to execute one publish
// run against a concrete publisher.
func (s *sqliteStore) GetExecutionPlan(
	ctx context.Context,
	publishRunID int64,
) (ExecutionPlan, error) {
	row := s.database.QueryRowContext(
		ctx,
		`SELECT
			pr.id,
			pr.release_run_id,
			rr.repository_id,
			r.name,
			rr.git_tag,
			pr.build_run_id,
			pr.publish_target_id,
			pt.name,
			pt.kind,
			pt.config_json,
			pr.status,
			pr.artifact_id,
			a.name,
			a.kind,
			a.path,
			br.artifact_root_path
		FROM publish_runs pr
		JOIN release_runs rr ON rr.id = pr.release_run_id
		JOIN repositories r ON r.id = rr.repository_id
		JOIN publish_targets pt ON pt.id = pr.publish_target_id
		JOIN build_runs br ON br.id = pr.build_run_id
		LEFT JOIN artifacts a ON a.id = pr.artifact_id
		WHERE pr.id = ?`,
		publishRunID,
	)

	var plan ExecutionPlan
	var artifactID sql.NullInt64
	var artifactName sql.NullString
	var artifactKind sql.NullString
	var artifactPath sql.NullString
	var artifactRootPath sql.NullString
	if err := row.Scan(
		&plan.PublishRunID,
		&plan.ReleaseRunID,
		&plan.RepositoryID,
		&plan.RepositoryName,
		&plan.GitTag,
		&plan.BuildRunID,
		&plan.PublishTargetID,
		&plan.PublishTargetName,
		&plan.PublishTargetKind,
		&plan.PublishTargetConfigJSON,
		&plan.Status,
		&artifactID,
		&artifactName,
		&artifactKind,
		&artifactPath,
		&artifactRootPath,
	); err != nil {
		if errors.Is(err, sql.ErrNoRows) {
			return ExecutionPlan{}, ErrRunNotFound
		}

		return ExecutionPlan{}, fmt.Errorf(
			"query publish execution plan for run %d: %w",
			publishRunID,
			err,
		)
	}

	if !artifactID.Valid {
		return ExecutionPlan{}, fmt.Errorf(
			"%w: publish run %d is missing artifact metadata",
			ErrInvalid,
			publishRunID,
		)
	}

	plan.ArtifactID = artifactID.Int64
	plan.ArtifactName = strings.TrimSpace(artifactName.String)
	plan.ArtifactKind = strings.TrimSpace(artifactKind.String)
	plan.ArtifactPath = strings.TrimSpace(artifactPath.String)
	plan.ArtifactRootPath = strings.TrimSpace(artifactRootPath.String)

	if plan.ArtifactName == "" || plan.ArtifactPath == "" {
		return ExecutionPlan{}, fmt.Errorf(
			"%w: publish run %d is missing artifact path data",
			ErrInvalid,
			publishRunID,
		)
	}
	if plan.ArtifactRootPath == "" {
		return ExecutionPlan{}, fmt.Errorf(
			"%w: publish run %d is missing build artifact root path",
			ErrInvalid,
			publishRunID,
		)
	}

	relativeArtifactPath, err := normalizeRelativeArtifactPath(plan.ArtifactPath)
	if err != nil {
		return ExecutionPlan{}, err
	}
	plan.ArtifactPath = relativeArtifactPath
	plan.SourcePath = filepath.Join(
		filepath.Clean(plan.ArtifactRootPath),
		filepath.FromSlash(relativeArtifactPath),
	)

	return plan, nil
}

// StartRun claims one queued publish run into the running state.
func (s *sqliteStore) StartRun(
	ctx context.Context,
	id int64,
	_ StartRunInput,
) (Run, error) {
	result, err := s.database.ExecContext(
		ctx,
		`UPDATE publish_runs
		SET
			status = ?,
			started_at = COALESCE(started_at, CURRENT_TIMESTAMP),
			finished_at = NULL,
			destination_ref = NULL,
			error_message = NULL,
			updated_at = CURRENT_TIMESTAMP
		WHERE id = ? AND status = ?`,
		StatusRunning,
		id,
		StatusQueued,
	)
	if err != nil {
		return Run{}, fmt.Errorf("start publish run %d: %w", id, err)
	}

	affected, err := result.RowsAffected()
	if err != nil {
		return Run{}, fmt.Errorf("count started publish run rows for %d: %w", id, err)
	}
	if affected == 0 {
		run, err := s.GetRun(ctx, id)
		if err != nil {
			return Run{}, err
		}

		return Run{}, fmt.Errorf(
			"%w: publish run %d has status %q",
			ErrRunNotQueued,
			id,
			run.Status,
		)
	}

	return s.GetRun(ctx, id)
}

// CompleteRun marks one running publish run as succeeded.
func (s *sqliteStore) CompleteRun(
	ctx context.Context,
	id int64,
	input CompleteRunInput,
) (Run, error) {
	result, err := s.database.ExecContext(
		ctx,
		`UPDATE publish_runs
		SET
			status = ?,
			destination_ref = COALESCE(?, destination_ref),
			finished_at = CURRENT_TIMESTAMP,
			error_message = NULL,
			updated_at = CURRENT_TIMESTAMP
		WHERE id = ? AND status = ?`,
		StatusSucceeded,
		nullableString(strings.TrimSpace(input.DestinationRef)),
		id,
		StatusRunning,
	)
	if err != nil {
		return Run{}, fmt.Errorf("complete publish run %d: %w", id, err)
	}

	affected, err := result.RowsAffected()
	if err != nil {
		return Run{}, fmt.Errorf("count completed publish run rows for %d: %w", id, err)
	}
	if affected == 0 {
		run, err := s.GetRun(ctx, id)
		if err != nil {
			return Run{}, err
		}

		return Run{}, fmt.Errorf(
			"%w: publish run %d has status %q",
			ErrRunNotRunning,
			id,
			run.Status,
		)
	}

	return s.GetRun(ctx, id)
}

// FailRun marks one running publish run as failed and records the terminal
// error message.
func (s *sqliteStore) FailRun(
	ctx context.Context,
	id int64,
	input FailRunInput,
) (Run, error) {
	errorMessage := strings.TrimSpace(input.ErrorMessage)
	if errorMessage == "" {
		return Run{}, fmt.Errorf("%w: error_message must not be empty", ErrInvalid)
	}

	result, err := s.database.ExecContext(
		ctx,
		`UPDATE publish_runs
		SET
			status = ?,
			destination_ref = COALESCE(?, destination_ref),
			finished_at = CURRENT_TIMESTAMP,
			error_message = ?,
			updated_at = CURRENT_TIMESTAMP
		WHERE id = ? AND status = ?`,
		StatusFailed,
		nullableString(strings.TrimSpace(input.DestinationRef)),
		errorMessage,
		id,
		StatusRunning,
	)
	if err != nil {
		return Run{}, fmt.Errorf("fail publish run %d: %w", id, err)
	}

	affected, err := result.RowsAffected()
	if err != nil {
		return Run{}, fmt.Errorf("count failed publish run rows for %d: %w", id, err)
	}
	if affected == 0 {
		run, err := s.GetRun(ctx, id)
		if err != nil {
			return Run{}, err
		}

		return Run{}, fmt.Errorf(
			"%w: publish run %d has status %q",
			ErrRunNotRunning,
			id,
			run.Status,
		)
	}

	return s.GetRun(ctx, id)
}

// nullableString returns nil for blank strings so optional SQLite columns stay
// NULL when unset.
func nullableString(value string) any {
	if strings.TrimSpace(value) == "" {
		return nil
	}

	return value
}