// Package build contains build target persistence and release-to-build
// planning logic.
package build

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"strings"

	"github.com/indiegabo/handy-unity-bulder/internal/credentials"
	internalgit "github.com/indiegabo/handy-unity-bulder/internal/git"
	"github.com/indiegabo/handy-unity-bulder/internal/release"
)

const (
	// DefaultRunnerType identifies the default isolated build executor family.
	DefaultRunnerType = "gameci"
	// DefaultTimeoutSeconds is the default build timeout applied when operators
	// do not specify an explicit value.
	DefaultTimeoutSeconds = 3600
	// StatusQueued is the first persisted state for build runs created by the
	// release planner.
	StatusQueued = "queued"
	// StatusRunning marks a build run that is actively being executed.
	StatusRunning = "running"
	// StatusSucceeded marks a build run that finished successfully.
	StatusSucceeded = "succeeded"
	// StatusFailed marks a build run that finished with a terminal failure.
	StatusFailed = "failed"
	// StatusCanceled marks a build run that was intentionally canceled.
	StatusCanceled = "canceled"
)

var (
	// ErrInvalid reports validation failures on build input.
	ErrInvalid = errors.New("invalid build input")
	// ErrNotFound reports missing build target records.
	ErrNotFound = errors.New("build target not found")
	// ErrConflict reports uniqueness collisions for build target records.
	ErrConflict = errors.New("build target conflict")
	// ErrRepositoryNotFound reports build target operations that reference an
	// unknown repository.
	ErrRepositoryNotFound = errors.New("build target repository not found")
	// ErrReleaseNotFound reports planning requests for unknown release runs.
	ErrReleaseNotFound = errors.New("build release run not found")
	// ErrReleaseNotQueued reports planning requests for releases that were not
	// yet handed off into the queued state.
	ErrReleaseNotQueued = errors.New("build release run not queued")
	// ErrNoEnabledTargets reports planning requests for repositories without any
	// enabled build targets.
	ErrNoEnabledTargets = errors.New("no enabled build targets")
	// ErrUnityVersionUnavailable reports that release planning could not resolve
	// a Unity version from repository contents for the requested tag.
	ErrUnityVersionUnavailable = errors.New("unity version unavailable")
	// ErrImageResolutionUnavailable reports that build planning could not
	// determine a concrete GameCI image reference for a target.
	ErrImageResolutionUnavailable = errors.New("build image resolution unavailable")
	// ErrRunNotFound reports missing build run records.
	ErrRunNotFound = errors.New("build run not found")
	// ErrRunNotQueued reports attempts to claim build runs that are no longer
	// waiting in the queued state.
	ErrRunNotQueued = errors.New("build run not queued")
	// ErrRunNotRunning reports terminal state updates for build runs that were
	// not first claimed into the running state.
	ErrRunNotRunning = errors.New("build run not running")
)

// Target is one durable build target definition stored in SQLite.
type Target struct {
	ID                   int64   `json:"id"`
	RepositoryID         int64   `json:"repository_id"`
	Name                 string  `json:"name"`
	Platform             string  `json:"platform"`
	RunnerType           string  `json:"runner_type"`
	BuildMethod          *string `json:"build_method,omitempty"`
	OutputKind           *string `json:"output_kind,omitempty"`
	OutputPathTemplate   *string `json:"output_path_template,omitempty"`
	UnityVersionOverride *string `json:"unity_version_override,omitempty"`
	ImageOverride        *string `json:"image_override,omitempty"`
	TimeoutSeconds       int     `json:"timeout_seconds"`
	Enabled              bool    `json:"enabled"`
	ConfigJSON           string  `json:"config_json"`
	CreatedAt            string  `json:"created_at"`
	UpdatedAt            string  `json:"updated_at"`
}

// CreateTargetInput defines the fields accepted when a build target is first
// registered.
type CreateTargetInput struct {
	RepositoryID         int64
	Name                 string
	Platform             string
	RunnerType           string
	BuildMethod          string
	OutputKind           string
	OutputPathTemplate   string
	UnityVersionOverride string
	ImageOverride        string
	TimeoutSeconds       int
	Enabled              *bool
	ConfigJSON           string
}

// UpdateTargetInput defines the fields accepted when a build target is
// replaced.
type UpdateTargetInput struct {
	Name                 string
	Platform             string
	RunnerType           string
	BuildMethod          string
	OutputKind           string
	OutputPathTemplate   string
	UnityVersionOverride string
	ImageOverride        string
	TimeoutSeconds       int
	Enabled              bool
	ConfigJSON           string
}

// Run is the durable build-run record created during release planning.
type Run struct {
	ID               int64   `json:"id"`
	ReleaseRunID     int64   `json:"release_run_id"`
	BuildTargetID    int64   `json:"build_target_id"`
	UnityVersion     *string `json:"unity_version,omitempty"`
	ImageRef         *string `json:"image_ref,omitempty"`
	Status           string  `json:"status"`
	WorkspacePath    *string `json:"workspace_path,omitempty"`
	LogPath          *string `json:"log_path,omitempty"`
	ArtifactRootPath *string `json:"artifact_root_path,omitempty"`
	StartedAt        *string `json:"started_at,omitempty"`
	FinishedAt       *string `json:"finished_at,omitempty"`
	ErrorMessage     *string `json:"error_message,omitempty"`
	CreatedAt        string  `json:"created_at"`
	UpdatedAt        string  `json:"updated_at"`
}

// Artifact is one durable artifact metadata row associated with a build run.
type Artifact struct {
	ID             int64   `json:"id"`
	BuildRunID     int64   `json:"build_run_id"`
	Name           string  `json:"name"`
	Kind           string  `json:"kind"`
	Path           string  `json:"path"`
	SizeBytes      *int64  `json:"size_bytes,omitempty"`
	ChecksumSHA256 *string `json:"checksum_sha256,omitempty"`
	CreatedAt      string  `json:"created_at"`
}

// CreateArtifactInput defines one filesystem artifact discovered for a build
// run that must be recorded durably in SQLite.
type CreateArtifactInput struct {
	Name           string
	Kind           string
	Path           string
	SizeBytes      *int64
	ChecksumSHA256 string
}

// ExecutionPlan joins the durable data required to execute one build run.
type ExecutionPlan struct {
	BuildRunID              int64   `json:"build_run_id"`
	ReleaseRunID            int64   `json:"release_run_id"`
	RepositoryID            int64   `json:"repository_id"`
	RepositoryName          string  `json:"repository_name"`
	RepositoryCredentialsID *int64  `json:"repository_credentials_id,omitempty"`
	BuildTargetID           int64   `json:"build_target_id"`
	RepositoryURL           string  `json:"repository_url"`
	GitTag                  string  `json:"git_tag"`
	GitCommit               *string `json:"git_commit,omitempty"`
	TargetName              string  `json:"target_name"`
	Platform                string  `json:"platform"`
	RunnerType              string  `json:"runner_type"`
	BuildMethod             *string `json:"build_method,omitempty"`
	OutputKind              *string `json:"output_kind,omitempty"`
	OutputPathTemplate      *string `json:"output_path_template,omitempty"`
	ConfigJSON              string  `json:"config_json"`
	UnityVersion            string  `json:"unity_version"`
	ImageRef                string  `json:"image_ref"`
	TimeoutSeconds          int     `json:"timeout_seconds"`
	Status                  string  `json:"status"`
}

// StartRunInput defines the writable execution fields recorded when a worker
// claims one queued build run.
type StartRunInput struct {
	WorkspacePath    string
	LogPath          string
	ArtifactRootPath string
}

// CompleteRunInput defines the writable execution fields recorded when a
// running build run finishes successfully.
type CompleteRunInput struct {
	WorkspacePath    string
	LogPath          string
	ArtifactRootPath string
}

// FailRunInput defines the writable execution fields recorded when a running
// build run finishes with a terminal error.
type FailRunInput struct {
	WorkspacePath    string
	LogPath          string
	ArtifactRootPath string
	ErrorMessage     string
}

// Store exposes the build target and build planning operations currently
// needed by operator surfaces and release planning.
type Store interface {
	CreateTarget(ctx context.Context, input CreateTargetInput) (Target, error)
	GetTarget(ctx context.Context, id int64) (Target, error)
	ListTargetsByRepository(ctx context.Context, repositoryID int64) ([]Target, error)
	ListEnabledTargetsByRepository(ctx context.Context, repositoryID int64) ([]Target, error)
	UpdateTarget(ctx context.Context, id int64, input UpdateTargetInput) (Target, error)
	DeleteTarget(ctx context.Context, id int64) error
	PlanRelease(ctx context.Context, releaseRunID int64) ([]Run, error)
	GetRun(ctx context.Context, id int64) (Run, error)
	GetExecutionPlan(ctx context.Context, buildRunID int64) (ExecutionPlan, error)
	StartRun(ctx context.Context, id int64, input StartRunInput) (Run, error)
	CompleteRun(ctx context.Context, id int64, input CompleteRunInput) (Run, error)
	FailRun(ctx context.Context, id int64, input FailRunInput) (Run, error)
	ReplaceArtifacts(ctx context.Context, buildRunID int64, inputs []CreateArtifactInput) ([]Artifact, error)
	ListArtifactsByBuildRun(ctx context.Context, buildRunID int64) ([]Artifact, error)
	ListBuildRunsByRelease(ctx context.Context, releaseRunID int64) ([]Run, error)
}

// NewStore creates the SQLite-backed build store.
func NewStore(database *sql.DB) Store {
	return &sqliteStore{
		database:             database,
		credentials:          credentials.NewStore(database),
		unityVersionDetector: newGitProjectVersionDetector(),
		imageResolver:        newGameCIImageResolver(),
	}
}

// sqliteStore persists build targets, build runs, and artifact metadata in
// SQLite.
type sqliteStore struct {
	database             *sql.DB
	credentials          credentials.Store
	unityVersionDetector unityVersionDetector
	imageResolver        imageReferenceResolver
}

// normalizedTarget is the canonical validated form of build target input.
type normalizedTarget struct {
	RepositoryID         int64
	Name                 string
	Platform             string
	RunnerType           string
	BuildMethod          string
	OutputKind           string
	OutputPathTemplate   string
	UnityVersionOverride string
	ImageOverride        string
	TimeoutSeconds       int
	Enabled              bool
	ConfigJSON           string
}

// releaseSummary holds the joined release metadata needed during build
// planning.
type releaseSummary struct {
	ID                      int64
	RepositoryID            int64
	RepositoryCredentialsID *int64
	RepositoryURL           string
	GitTag                  string
	UnityVersion            *string
	Status                  string
}

// CreateTarget inserts one build target and returns the stored record.
func (s *sqliteStore) CreateTarget(
	ctx context.Context,
	input CreateTargetInput,
) (Target, error) {
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

	result, err := s.database.ExecContext(
		ctx,
		`INSERT INTO build_targets (
			repository_id,
			name,
			platform,
			runner_type,
			build_method,
			output_kind,
			output_path_template,
			unity_version_override,
			image_override,
			timeout_seconds,
			enabled,
			config_json
		) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
		normalized.RepositoryID,
		normalized.Name,
		normalized.Platform,
		normalized.RunnerType,
		nullableString(normalized.BuildMethod),
		nullableString(normalized.OutputKind),
		nullableString(normalized.OutputPathTemplate),
		nullableString(normalized.UnityVersionOverride),
		nullableString(normalized.ImageOverride),
		normalized.TimeoutSeconds,
		boolToSQLite(normalized.Enabled),
		normalized.ConfigJSON,
	)
	if err != nil {
		return Target{}, mapSQLError(err)
	}

	id, err := result.LastInsertId()
	if err != nil {
		return Target{}, fmt.Errorf("read build target id: %w", err)
	}

	return s.GetTarget(ctx, id)
}

// GetTarget loads one build target by identifier.
func (s *sqliteStore) GetTarget(ctx context.Context, id int64) (Target, error) {
	row := s.database.QueryRowContext(
		ctx,
		`SELECT
			id,
			repository_id,
			name,
			platform,
			runner_type,
			build_method,
			output_kind,
			output_path_template,
			unity_version_override,
			image_override,
			timeout_seconds,
			enabled,
			config_json,
			created_at,
			updated_at
		FROM build_targets
		WHERE id = ?`,
		id,
	)

	target, err := scanTarget(row)
	if err != nil {
		if errors.Is(err, sql.ErrNoRows) {
			return Target{}, ErrNotFound
		}

		return Target{}, fmt.Errorf("query build target %d: %w", id, err)
	}

	return target, nil
}

// ListTargetsByRepository returns build targets for one repository ordered by
// id.
func (s *sqliteStore) ListTargetsByRepository(
	ctx context.Context,
	repositoryID int64,
) ([]Target, error) {
	return s.listTargetsByRepository(ctx, repositoryID, false)
}

// ListEnabledTargetsByRepository returns enabled build targets for one
// repository ordered by id.
func (s *sqliteStore) ListEnabledTargetsByRepository(
	ctx context.Context,
	repositoryID int64,
) ([]Target, error) {
	return s.listTargetsByRepository(ctx, repositoryID, true)
}

// UpdateTarget replaces the writable fields of an existing build target.
func (s *sqliteStore) UpdateTarget(
	ctx context.Context,
	id int64,
	input UpdateTargetInput,
) (Target, error) {
	normalized, err := normalizeUpdateTargetInput(input)
	if err != nil {
		return Target{}, err
	}

	result, err := s.database.ExecContext(
		ctx,
		`UPDATE build_targets
		SET
			name = ?,
			platform = ?,
			runner_type = ?,
			build_method = ?,
			output_kind = ?,
			output_path_template = ?,
			unity_version_override = ?,
			image_override = ?,
			timeout_seconds = ?,
			enabled = ?,
			config_json = ?,
			updated_at = CURRENT_TIMESTAMP
		WHERE id = ?`,
		normalized.Name,
		normalized.Platform,
		normalized.RunnerType,
		nullableString(normalized.BuildMethod),
		nullableString(normalized.OutputKind),
		nullableString(normalized.OutputPathTemplate),
		nullableString(normalized.UnityVersionOverride),
		nullableString(normalized.ImageOverride),
		normalized.TimeoutSeconds,
		boolToSQLite(normalized.Enabled),
		normalized.ConfigJSON,
		id,
	)
	if err != nil {
		return Target{}, mapSQLError(err)
	}

	rowsAffected, err := result.RowsAffected()
	if err != nil {
		return Target{}, fmt.Errorf("read updated build target rows: %w", err)
	}
	if rowsAffected == 0 {
		return Target{}, ErrNotFound
	}

	return s.GetTarget(ctx, id)
}

// DeleteTarget removes one build target.
func (s *sqliteStore) DeleteTarget(ctx context.Context, id int64) error {
	result, err := s.database.ExecContext(
		ctx,
		`DELETE FROM build_targets WHERE id = ?`,
		id,
	)
	if err != nil {
		return fmt.Errorf("delete build target %d: %w", id, err)
	}

	rowsAffected, err := result.RowsAffected()
	if err != nil {
		return fmt.Errorf("read deleted build target rows: %w", err)
	}
	if rowsAffected == 0 {
		return ErrNotFound
	}

	return nil
}

// PlanRelease expands one queued release run into queued build runs for every
// enabled build target on the release repository.
func (s *sqliteStore) PlanRelease(
	ctx context.Context,
	releaseRunID int64,
) ([]Run, error) {
	if releaseRunID <= 0 {
		return nil, fmt.Errorf(
			"%w: release_run_id must be greater than zero",
			ErrInvalid,
		)
	}

	releaseRun, err := s.getReleaseSummary(ctx, s.database, releaseRunID)
	if err != nil {
		return nil, err
	}

	if releaseRun.Status != release.StatusQueued {
		return nil, fmt.Errorf(
			"%w: release run %d has status %q",
			ErrReleaseNotQueued,
			releaseRun.ID,
			releaseRun.Status,
		)
	}

	targets, err := s.listTargetsByRepositoryWithQuery(
		ctx,
		s.database,
		releaseRun.RepositoryID,
		true,
	)
	if err != nil {
		return nil, err
	}
	if len(targets) == 0 {
		return nil, ErrNoEnabledTargets
	}

	releaseUnityVersion, err := s.resolveReleaseUnityVersion(
		ctx,
		s.database,
		releaseRun,
	)
	if err != nil {
		return nil, err
	}

	tx, err := s.database.BeginTx(ctx, nil)
	if err != nil {
		return nil, fmt.Errorf("begin build planning transaction: %w", err)
	}
	defer tx.Rollback()

	releaseRun, err = s.getReleaseSummary(ctx, tx, releaseRunID)
	if err != nil {
		return nil, err
	}

	if releaseRun.Status != release.StatusQueued {
		return nil, fmt.Errorf(
			"%w: release run %d has status %q",
			ErrReleaseNotQueued,
			releaseRun.ID,
			releaseRun.Status,
		)
	}

	targets, err = s.listTargetsByRepositoryWithQuery(
		ctx,
		tx,
		releaseRun.RepositoryID,
		true,
	)
	if err != nil {
		return nil, err
	}
	if len(targets) == 0 {
		return nil, ErrNoEnabledTargets
	}

	for _, target := range targets {
		plannedUnityVersion := resolveTargetUnityVersion(target, releaseUnityVersion)
		imageRef, err := s.imageResolver.Resolve(target, plannedUnityVersion)
		if err != nil {
			return nil, err
		}

		if _, err := tx.ExecContext(
			ctx,
			`INSERT INTO build_runs (
				release_run_id,
				build_target_id,
				unity_version,
				image_ref,
				status
			) VALUES (?, ?, ?, ?, ?)
			ON CONFLICT(release_run_id, build_target_id) DO UPDATE SET
				unity_version = excluded.unity_version,
				image_ref = excluded.image_ref,
				updated_at = CURRENT_TIMESTAMP
			WHERE build_runs.status = ?`,
			releaseRunID,
			target.ID,
			plannedUnityVersion,
			imageRef,
			StatusQueued,
			StatusQueued,
		); err != nil {
			return nil, fmt.Errorf(
				"plan build run for release %d target %d: %w",
				releaseRunID,
				target.ID,
				err,
			)
		}
	}

	runs, err := s.listBuildRunsByReleaseWithQuery(ctx, tx, releaseRunID)
	if err != nil {
		return nil, err
	}

	if err := tx.Commit(); err != nil {
		return nil, fmt.Errorf("commit build planning transaction: %w", err)
	}

	return runs, nil
}

// ListBuildRunsByRelease returns build runs for one release ordered by target
// and identifier.
func (s *sqliteStore) ListBuildRunsByRelease(
	ctx context.Context,
	releaseRunID int64,
) ([]Run, error) {
	if releaseRunID <= 0 {
		return nil, fmt.Errorf(
			"%w: release_run_id must be greater than zero",
			ErrInvalid,
		)
	}

	return s.listBuildRunsByReleaseWithQuery(ctx, s.database, releaseRunID)
}

// GetRun loads one build run by identifier.
func (s *sqliteStore) GetRun(ctx context.Context, id int64) (Run, error) {
	row := s.database.QueryRowContext(
		ctx,
		`SELECT
			id,
			release_run_id,
			build_target_id,
			unity_version,
			image_ref,
			status,
			workspace_path,
			log_path,
			artifact_root_path,
			started_at,
			finished_at,
			error_message,
			created_at,
			updated_at
		FROM build_runs
		WHERE id = ?`,
		id,
	)

	run, err := scanRun(row)
	if err != nil {
		if errors.Is(err, sql.ErrNoRows) {
			return Run{}, ErrRunNotFound
		}

		return Run{}, fmt.Errorf("query build run %d: %w", id, err)
	}

	return run, nil
}

// GetExecutionPlan loads the joined repository, release, and target metadata
// required to execute one build run.
func (s *sqliteStore) GetExecutionPlan(
	ctx context.Context,
	buildRunID int64,
) (ExecutionPlan, error) {
	row := s.database.QueryRowContext(
		ctx,
		`SELECT
			br.id,
			br.release_run_id,
			rr.repository_id,
			r.name,
			r.credentials_id,
			br.build_target_id,
			r.repo_url,
			rr.git_tag,
			rr.git_commit,
			bt.name,
			bt.platform,
			bt.runner_type,
			bt.build_method,
			bt.output_kind,
			bt.output_path_template,
			bt.config_json,
			br.unity_version,
			br.image_ref,
			bt.timeout_seconds,
			br.status
		FROM build_runs br
		JOIN release_runs rr ON rr.id = br.release_run_id
		JOIN repositories r ON r.id = rr.repository_id
		JOIN build_targets bt ON bt.id = br.build_target_id
		WHERE br.id = ?`,
		buildRunID,
	)

	var plan ExecutionPlan
	var gitCommit sql.NullString
	var buildMethod sql.NullString
	var outputKind sql.NullString
	var outputPathTemplate sql.NullString
	var unityVersion sql.NullString
	var imageRef sql.NullString
	if err := row.Scan(
		&plan.BuildRunID,
		&plan.ReleaseRunID,
		&plan.RepositoryID,
		&plan.RepositoryName,
		&plan.RepositoryCredentialsID,
		&plan.BuildTargetID,
		&plan.RepositoryURL,
		&plan.GitTag,
		&gitCommit,
		&plan.TargetName,
		&plan.Platform,
		&plan.RunnerType,
		&buildMethod,
		&outputKind,
		&outputPathTemplate,
		&plan.ConfigJSON,
		&unityVersion,
		&imageRef,
		&plan.TimeoutSeconds,
		&plan.Status,
	); err != nil {
		if errors.Is(err, sql.ErrNoRows) {
			return ExecutionPlan{}, ErrRunNotFound
		}

		return ExecutionPlan{}, fmt.Errorf(
			"query build execution plan for run %d: %w",
			buildRunID,
			err,
		)
	}

	plan.GitCommit = stringPointer(gitCommit)
	plan.BuildMethod = stringPointer(buildMethod)
	plan.OutputKind = stringPointer(outputKind)
	plan.OutputPathTemplate = stringPointer(outputPathTemplate)
	plan.UnityVersion = strings.TrimSpace(nullStringValue(unityVersion))
	plan.ImageRef = strings.TrimSpace(nullStringValue(imageRef))

	if plan.UnityVersion == "" || plan.ImageRef == "" {
		return ExecutionPlan{}, fmt.Errorf(
			"%w: build run %d is missing planned image metadata",
			ErrInvalid,
			buildRunID,
		)
	}

	return plan, nil
}

// StartRun claims one queued build run into the running state.
func (s *sqliteStore) StartRun(
	ctx context.Context,
	id int64,
	input StartRunInput,
) (Run, error) {
	result, err := s.database.ExecContext(
		ctx,
		`UPDATE build_runs
		SET
			status = ?,
			workspace_path = COALESCE(?, workspace_path),
			log_path = COALESCE(?, log_path),
			artifact_root_path = COALESCE(?, artifact_root_path),
			started_at = COALESCE(started_at, CURRENT_TIMESTAMP),
			finished_at = NULL,
			error_message = NULL,
			updated_at = CURRENT_TIMESTAMP
		WHERE id = ? AND status = ?`,
		StatusRunning,
		nullableString(strings.TrimSpace(input.WorkspacePath)),
		nullableString(strings.TrimSpace(input.LogPath)),
		nullableString(strings.TrimSpace(input.ArtifactRootPath)),
		id,
		StatusQueued,
	)
	if err != nil {
		return Run{}, fmt.Errorf("start build run %d: %w", id, err)
	}

	affected, err := result.RowsAffected()
	if err != nil {
		return Run{}, fmt.Errorf("count started build run rows for %d: %w", id, err)
	}
	if affected == 0 {
		run, err := s.GetRun(ctx, id)
		if err != nil {
			return Run{}, err
		}

		return Run{}, fmt.Errorf(
			"%w: build run %d has status %q",
			ErrRunNotQueued,
			id,
			run.Status,
		)
	}

	return s.GetRun(ctx, id)
}

// CompleteRun marks one running build run as succeeded.
func (s *sqliteStore) CompleteRun(
	ctx context.Context,
	id int64,
	input CompleteRunInput,
) (Run, error) {
	result, err := s.database.ExecContext(
		ctx,
		`UPDATE build_runs
		SET
			status = ?,
			workspace_path = COALESCE(?, workspace_path),
			log_path = COALESCE(?, log_path),
			artifact_root_path = COALESCE(?, artifact_root_path),
			finished_at = CURRENT_TIMESTAMP,
			error_message = NULL,
			updated_at = CURRENT_TIMESTAMP
		WHERE id = ? AND status = ?`,
		StatusSucceeded,
		nullableString(strings.TrimSpace(input.WorkspacePath)),
		nullableString(strings.TrimSpace(input.LogPath)),
		nullableString(strings.TrimSpace(input.ArtifactRootPath)),
		id,
		StatusRunning,
	)
	if err != nil {
		return Run{}, fmt.Errorf("complete build run %d: %w", id, err)
	}

	affected, err := result.RowsAffected()
	if err != nil {
		return Run{}, fmt.Errorf("count completed build run rows for %d: %w", id, err)
	}
	if affected == 0 {
		run, err := s.GetRun(ctx, id)
		if err != nil {
			return Run{}, err
		}

		return Run{}, fmt.Errorf(
			"%w: build run %d has status %q",
			ErrRunNotRunning,
			id,
			run.Status,
		)
	}

	return s.GetRun(ctx, id)
}

// ReplaceArtifacts replaces the durable artifact metadata for one build run
// with the newly discovered filesystem outputs.
func (s *sqliteStore) ReplaceArtifacts(
	ctx context.Context,
	buildRunID int64,
	inputs []CreateArtifactInput,
) ([]Artifact, error) {
	if buildRunID <= 0 {
		return nil, fmt.Errorf("%w: build_run_id must be greater than zero", ErrInvalid)
	}

	if _, err := s.GetRun(ctx, buildRunID); err != nil {
		return nil, err
	}

	tx, err := s.database.BeginTx(ctx, nil)
	if err != nil {
		return nil, fmt.Errorf("begin artifact registration transaction: %w", err)
	}
	defer tx.Rollback()

	if _, err := tx.ExecContext(
		ctx,
		`DELETE FROM artifacts WHERE build_run_id = ?`,
		buildRunID,
	); err != nil {
		return nil, fmt.Errorf("clear build artifacts for run %d: %w", buildRunID, err)
	}

	for _, input := range inputs {
		normalized, err := normalizeArtifactInput(input)
		if err != nil {
			return nil, err
		}

		if _, err := tx.ExecContext(
			ctx,
			`INSERT INTO artifacts (
				build_run_id,
				name,
				kind,
				path,
				size_bytes,
				checksum_sha256
			) VALUES (?, ?, ?, ?, ?, ?)`,
			buildRunID,
			normalized.Name,
			normalized.Kind,
			normalized.Path,
			nullableInt64(normalized.SizeBytes),
			nullableString(normalized.ChecksumSHA256),
		); err != nil {
			return nil, fmt.Errorf(
				"insert artifact %q for build run %d: %w",
				normalized.Path,
				buildRunID,
				err,
			)
		}
	}

	artifacts, err := s.listArtifactsByBuildRunWithQuery(ctx, tx, buildRunID)
	if err != nil {
		return nil, err
	}

	if err := tx.Commit(); err != nil {
		return nil, fmt.Errorf("commit artifact registration transaction: %w", err)
	}

	return artifacts, nil
}

// ListArtifactsByBuildRun returns recorded artifact metadata for one build run.
func (s *sqliteStore) ListArtifactsByBuildRun(
	ctx context.Context,
	buildRunID int64,
) ([]Artifact, error) {
	if buildRunID <= 0 {
		return nil, fmt.Errorf("%w: build_run_id must be greater than zero", ErrInvalid)
	}

	if _, err := s.GetRun(ctx, buildRunID); err != nil {
		return nil, err
	}

	return s.listArtifactsByBuildRunWithQuery(ctx, s.database, buildRunID)
}

// FailRun marks one running build run as failed and records the terminal
// error message.
func (s *sqliteStore) FailRun(
	ctx context.Context,
	id int64,
	input FailRunInput,
) (Run, error) {
	errorMessage := strings.TrimSpace(input.ErrorMessage)
	if errorMessage == "" {
		return Run{}, fmt.Errorf(
			"%w: error_message must not be empty",
			ErrInvalid,
		)
	}

	result, err := s.database.ExecContext(
		ctx,
		`UPDATE build_runs
		SET
			status = ?,
			workspace_path = COALESCE(?, workspace_path),
			log_path = COALESCE(?, log_path),
			artifact_root_path = COALESCE(?, artifact_root_path),
			finished_at = CURRENT_TIMESTAMP,
			error_message = ?,
			updated_at = CURRENT_TIMESTAMP
		WHERE id = ? AND status = ?`,
		StatusFailed,
		nullableString(strings.TrimSpace(input.WorkspacePath)),
		nullableString(strings.TrimSpace(input.LogPath)),
		nullableString(strings.TrimSpace(input.ArtifactRootPath)),
		errorMessage,
		id,
		StatusRunning,
	)
	if err != nil {
		return Run{}, fmt.Errorf("fail build run %d: %w", id, err)
	}

	affected, err := result.RowsAffected()
	if err != nil {
		return Run{}, fmt.Errorf("count failed build run rows for %d: %w", id, err)
	}
	if affected == 0 {
		run, err := s.GetRun(ctx, id)
		if err != nil {
			return Run{}, err
		}

		return Run{}, fmt.Errorf(
			"%w: build run %d has status %q",
			ErrRunNotRunning,
			id,
			run.Status,
		)
	}

	return s.GetRun(ctx, id)
}

// queryer abstracts the subset of SQL query methods shared by database
// handles and transactions.
type queryer interface {
	ExecContext(context.Context, string, ...any) (sql.Result, error)
	QueryContext(context.Context, string, ...any) (*sql.Rows, error)
	QueryRowContext(context.Context, string, ...any) *sql.Row
}

// scanner abstracts row scanners shared by single-row and multi-row queries.
type scanner interface {
	Scan(dest ...any) error
}

// listTargetsByRepository routes target listing through the shared query-based
// implementation.
func (s *sqliteStore) listTargetsByRepository(
	ctx context.Context,
	repositoryID int64,
	onlyEnabled bool,
) ([]Target, error) {
	return s.listTargetsByRepositoryWithQuery(
		ctx,
		s.database,
		repositoryID,
		onlyEnabled,
	)
}

// listTargetsByRepositoryWithQuery lists repository build targets using the
// provided query executor and optional enabled-only filtering.
func (s *sqliteStore) listTargetsByRepositoryWithQuery(
	ctx context.Context,
	query queryer,
	repositoryID int64,
	onlyEnabled bool,
) ([]Target, error) {
	if repositoryID <= 0 {
		return nil, fmt.Errorf(
			"%w: repository_id must be greater than zero",
			ErrInvalid,
		)
	}

	repositoryExists, err := s.repositoryExistsWithQuery(ctx, query, repositoryID)
	if err != nil {
		return nil, err
	}
	if !repositoryExists {
		return nil, ErrRepositoryNotFound
	}

	sqlText := `SELECT
		id,
		repository_id,
		name,
		platform,
		runner_type,
		build_method,
		output_kind,
		output_path_template,
		unity_version_override,
		image_override,
		timeout_seconds,
		enabled,
		config_json,
		created_at,
		updated_at
	FROM build_targets
	WHERE repository_id = ?`
	args := []any{repositoryID}
	if onlyEnabled {
		sqlText += ` AND enabled = 1`
	}
	sqlText += ` ORDER BY id ASC`

	rows, err := query.QueryContext(ctx, sqlText, args...)
	if err != nil {
		return nil, fmt.Errorf("list build targets: %w", err)
	}
	defer rows.Close()

	targets := make([]Target, 0)
	for rows.Next() {
		target, err := scanTarget(rows)
		if err != nil {
			return nil, fmt.Errorf("scan build target row: %w", err)
		}

		targets = append(targets, target)
	}

	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("iterate build targets: %w", err)
	}

	return targets, nil
}

// listBuildRunsByReleaseWithQuery lists build runs for one release using the
// provided query executor.
func (s *sqliteStore) listBuildRunsByReleaseWithQuery(
	ctx context.Context,
	query queryer,
	releaseRunID int64,
) ([]Run, error) {
	rows, err := query.QueryContext(
		ctx,
		`SELECT
			id,
			release_run_id,
			build_target_id,
			unity_version,
			image_ref,
			status,
			workspace_path,
			log_path,
			artifact_root_path,
			started_at,
			finished_at,
			error_message,
			created_at,
			updated_at
		FROM build_runs
		WHERE release_run_id = ?
		ORDER BY build_target_id ASC, id ASC`,
		releaseRunID,
	)
	if err != nil {
		return nil, fmt.Errorf("list build runs: %w", err)
	}
	defer rows.Close()

	runs := make([]Run, 0)
	for rows.Next() {
		run, err := scanRun(rows)
		if err != nil {
			return nil, fmt.Errorf("scan build run row: %w", err)
		}

		runs = append(runs, run)
	}

	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("iterate build runs: %w", err)
	}

	return runs, nil
}

// listArtifactsByBuildRunWithQuery lists artifact rows for one build run using
// the provided query executor.
func (s *sqliteStore) listArtifactsByBuildRunWithQuery(
	ctx context.Context,
	query queryer,
	buildRunID int64,
) ([]Artifact, error) {
	rows, err := query.QueryContext(
		ctx,
		`SELECT
			id,
			build_run_id,
			name,
			kind,
			path,
			size_bytes,
			checksum_sha256,
			created_at
		FROM artifacts
		WHERE build_run_id = ?
		ORDER BY path ASC, id ASC`,
		buildRunID,
	)
	if err != nil {
		return nil, fmt.Errorf("list artifacts for build run %d: %w", buildRunID, err)
	}
	defer rows.Close()

	artifacts := make([]Artifact, 0)
	for rows.Next() {
		artifact, err := scanArtifact(rows)
		if err != nil {
			return nil, fmt.Errorf("scan artifact row: %w", err)
		}

		artifacts = append(artifacts, artifact)
	}

	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("iterate artifacts: %w", err)
	}

	return artifacts, nil
}

// getReleaseSummary loads the release metadata required to plan build runs.
func (s *sqliteStore) getReleaseSummary(
	ctx context.Context,
	query queryer,
	releaseRunID int64,
) (releaseSummary, error) {
	row := query.QueryRowContext(
		ctx,
		`SELECT
			rr.id,
			rr.repository_id,
			r.credentials_id,
			r.repo_url,
			rr.git_tag,
			rr.unity_version,
			rr.status
		FROM release_runs rr
		JOIN repositories r ON r.id = rr.repository_id
		WHERE rr.id = ?`,
		releaseRunID,
	)

	var summary releaseSummary
	var unityVersion sql.NullString
	if err := row.Scan(
		&summary.ID,
		&summary.RepositoryID,
		&summary.RepositoryCredentialsID,
		&summary.RepositoryURL,
		&summary.GitTag,
		&unityVersion,
		&summary.Status,
	); err != nil {
		if errors.Is(err, sql.ErrNoRows) {
			return releaseSummary{}, ErrReleaseNotFound
		}

		return releaseSummary{}, fmt.Errorf(
			"query release run %d for planning: %w",
			releaseRunID,
			err,
		)
	}

	summary.UnityVersion = stringPointer(unityVersion)

	return summary, nil
}

// resolveReleaseUnityVersion returns the persisted release Unity version or
// detects and persists it when still unset.
func (s *sqliteStore) resolveReleaseUnityVersion(
	ctx context.Context,
	query queryer,
	releaseRun releaseSummary,
) (string, error) {
	if releaseRun.UnityVersion != nil {
		persistedVersion := strings.TrimSpace(*releaseRun.UnityVersion)
		if persistedVersion != "" {
			return persistedVersion, nil
		}
	}

	auth, err := s.resolveRepositoryGitAuth(ctx, releaseRun.RepositoryCredentialsID)
	if err != nil {
		return "", err
	}

	detectedVersion, err := s.unityVersionDetector.Detect(
		ctx,
		releaseRun.RepositoryURL,
		releaseRun.GitTag,
		auth,
	)
	if err != nil {
		return "", err
	}

	if _, err := query.ExecContext(
		ctx,
		`UPDATE release_runs
		SET
			unity_version = ?,
			updated_at = CURRENT_TIMESTAMP
		WHERE id = ?`,
		detectedVersion,
		releaseRun.ID,
	); err != nil {
		return "", fmt.Errorf(
			"persist release run %d unity version: %w",
			releaseRun.ID,
			err,
		)
	}

	return detectedVersion, nil
}

// resolveRepositoryGitAuth loads optional repository credentials and converts
// them into Git authentication flags for release planning.
func (s *sqliteStore) resolveRepositoryGitAuth(
	ctx context.Context,
	credentialsID *int64,
) (internalgit.AuthOptions, error) {
	if credentialsID == nil {
		return internalgit.AuthOptions{}, nil
	}

	record, err := s.credentials.Get(ctx, *credentialsID)
	if err != nil {
		return internalgit.AuthOptions{}, fmt.Errorf(
			"load repository credentials %d: %w",
			*credentialsID,
			err,
		)
	}

	auth, err := internalgit.AuthOptionsFromCredentials(record)
	if err != nil {
		return internalgit.AuthOptions{}, fmt.Errorf(
			"resolve repository credentials %d for Git auth: %w",
			*credentialsID,
			err,
		)
	}

	return auth, nil
}

// scanTarget decodes one build target row into the public target model.
func scanTarget(row scanner) (Target, error) {
	var target Target
	var buildMethod sql.NullString
	var outputKind sql.NullString
	var outputPathTemplate sql.NullString
	var unityVersionOverride sql.NullString
	var imageOverride sql.NullString
	var enabled int64

	err := row.Scan(
		&target.ID,
		&target.RepositoryID,
		&target.Name,
		&target.Platform,
		&target.RunnerType,
		&buildMethod,
		&outputKind,
		&outputPathTemplate,
		&unityVersionOverride,
		&imageOverride,
		&target.TimeoutSeconds,
		&enabled,
		&target.ConfigJSON,
		&target.CreatedAt,
		&target.UpdatedAt,
	)
	if err != nil {
		return Target{}, err
	}

	target.BuildMethod = stringPointer(buildMethod)
	target.OutputKind = stringPointer(outputKind)
	target.OutputPathTemplate = stringPointer(outputPathTemplate)
	target.UnityVersionOverride = stringPointer(unityVersionOverride)
	target.ImageOverride = stringPointer(imageOverride)
	target.Enabled = enabled == 1

	return target, nil
}

// scanRun decodes one build run row into the public run model.
func scanRun(row scanner) (Run, error) {
	var run Run
	var unityVersion sql.NullString
	var imageRef sql.NullString
	var workspacePath sql.NullString
	var logPath sql.NullString
	var artifactRootPath sql.NullString
	var startedAt sql.NullString
	var finishedAt sql.NullString
	var errorMessage sql.NullString

	err := row.Scan(
		&run.ID,
		&run.ReleaseRunID,
		&run.BuildTargetID,
		&unityVersion,
		&imageRef,
		&run.Status,
		&workspacePath,
		&logPath,
		&artifactRootPath,
		&startedAt,
		&finishedAt,
		&errorMessage,
		&run.CreatedAt,
		&run.UpdatedAt,
	)
	if err != nil {
		return Run{}, err
	}

	run.UnityVersion = stringPointer(unityVersion)
	run.ImageRef = stringPointer(imageRef)
	run.WorkspacePath = stringPointer(workspacePath)
	run.LogPath = stringPointer(logPath)
	run.ArtifactRootPath = stringPointer(artifactRootPath)
	run.StartedAt = stringPointer(startedAt)
	run.FinishedAt = stringPointer(finishedAt)
	run.ErrorMessage = stringPointer(errorMessage)

	return run, nil
}

// scanArtifact decodes one artifact row into the public artifact model.
func scanArtifact(row scanner) (Artifact, error) {
	var artifact Artifact
	var sizeBytes sql.NullInt64
	var checksumSHA256 sql.NullString

	err := row.Scan(
		&artifact.ID,
		&artifact.BuildRunID,
		&artifact.Name,
		&artifact.Kind,
		&artifact.Path,
		&sizeBytes,
		&checksumSHA256,
		&artifact.CreatedAt,
	)
	if err != nil {
		return Artifact{}, err
	}

	artifact.SizeBytes = int64Pointer(sizeBytes)
	artifact.ChecksumSHA256 = stringPointer(checksumSHA256)

	return artifact, nil
}

// nullStringValue unwraps a nullable SQL string into the empty string when the
// value is absent.
func nullStringValue(value sql.NullString) string {
	if !value.Valid {
		return ""
	}

	return value.String
}

// normalizeCreateTargetInput converts create input into the validated target
// form stored by the build store.
func normalizeCreateTargetInput(
	input CreateTargetInput,
) (normalizedTarget, error) {
	enabled := true
	if input.Enabled != nil {
		enabled = *input.Enabled
	}

	timeoutSeconds := input.TimeoutSeconds
	if timeoutSeconds == 0 {
		timeoutSeconds = DefaultTimeoutSeconds
	}

	return normalizeTarget(normalizedTarget{
		RepositoryID:         input.RepositoryID,
		Name:                 input.Name,
		Platform:             input.Platform,
		RunnerType:           input.RunnerType,
		BuildMethod:          input.BuildMethod,
		OutputKind:           input.OutputKind,
		OutputPathTemplate:   input.OutputPathTemplate,
		UnityVersionOverride: input.UnityVersionOverride,
		ImageOverride:        input.ImageOverride,
		TimeoutSeconds:       timeoutSeconds,
		Enabled:              enabled,
		ConfigJSON:           input.ConfigJSON,
	})
}

// normalizeUpdateTargetInput converts update input into the validated target
// form stored by the build store.
func normalizeUpdateTargetInput(
	input UpdateTargetInput,
) (normalizedTarget, error) {
	return normalizeTarget(normalizedTarget{
		Name:                 input.Name,
		Platform:             input.Platform,
		RunnerType:           input.RunnerType,
		BuildMethod:          input.BuildMethod,
		OutputKind:           input.OutputKind,
		OutputPathTemplate:   input.OutputPathTemplate,
		UnityVersionOverride: input.UnityVersionOverride,
		ImageOverride:        input.ImageOverride,
		TimeoutSeconds:       input.TimeoutSeconds,
		Enabled:              input.Enabled,
		ConfigJSON:           input.ConfigJSON,
	})
}

// normalizeTarget trims and validates one normalized build target payload.
func normalizeTarget(input normalizedTarget) (normalizedTarget, error) {
	input.Name = strings.TrimSpace(input.Name)
	input.Platform = strings.TrimSpace(input.Platform)
	input.RunnerType = strings.TrimSpace(input.RunnerType)
	input.BuildMethod = strings.TrimSpace(input.BuildMethod)
	input.OutputKind = strings.TrimSpace(input.OutputKind)
	input.OutputPathTemplate = strings.TrimSpace(input.OutputPathTemplate)
	input.UnityVersionOverride = strings.TrimSpace(input.UnityVersionOverride)
	input.ImageOverride = strings.TrimSpace(input.ImageOverride)

	if input.RepositoryID < 0 {
		return normalizedTarget{}, fmt.Errorf(
			"%w: repository_id must not be negative",
			ErrInvalid,
		)
	}

	if input.Name == "" {
		return normalizedTarget{}, fmt.Errorf(
			"%w: name must not be empty",
			ErrInvalid,
		)
	}

	if input.Platform == "" {
		return normalizedTarget{}, fmt.Errorf(
			"%w: platform must not be empty",
			ErrInvalid,
		)
	}

	if input.RunnerType == "" {
		input.RunnerType = DefaultRunnerType
	}

	if input.TimeoutSeconds <= 0 {
		return normalizedTarget{}, fmt.Errorf(
			"%w: timeout_seconds must be greater than zero",
			ErrInvalid,
		)
	}

	if err := ValidateRequestedOutputPath(input.OutputKind, input.OutputPathTemplate); err != nil {
		return normalizedTarget{}, err
	}

	configJSON, err := normalizeConfigJSON(input.ConfigJSON)
	if err != nil {
		return normalizedTarget{}, err
	}
	input.ConfigJSON = configJSON

	return input, nil
}

// normalizeConfigJSON validates build target config payloads as canonical JSON
// objects.
func normalizeConfigJSON(raw string) (string, error) {
	trimmed := strings.TrimSpace(raw)
	if trimmed == "" {
		return `{}`, nil
	}

	var decoded any
	if err := json.Unmarshal([]byte(trimmed), &decoded); err != nil {
		return "", fmt.Errorf(
			"%w: config_json must be valid JSON: %v",
			ErrInvalid,
			err,
		)
	}

	if _, ok := decoded.(map[string]any); !ok {
		return "", fmt.Errorf(
			"%w: config_json must be a JSON object",
			ErrInvalid,
		)
	}

	encoded, err := json.Marshal(decoded)
	if err != nil {
		return "", fmt.Errorf("normalize build target config: %w", err)
	}

	return string(encoded), nil
}

// normalizeArtifactInput trims and validates one artifact registration input.
func normalizeArtifactInput(input CreateArtifactInput) (CreateArtifactInput, error) {
	input.Name = strings.TrimSpace(input.Name)
	input.Kind = strings.TrimSpace(input.Kind)
	input.Path = strings.TrimSpace(input.Path)
	input.ChecksumSHA256 = strings.TrimSpace(input.ChecksumSHA256)

	if input.Name == "" {
		return CreateArtifactInput{}, fmt.Errorf("%w: artifact name must not be empty", ErrInvalid)
	}
	if input.Kind == "" {
		return CreateArtifactInput{}, fmt.Errorf("%w: artifact kind must not be empty", ErrInvalid)
	}
	if input.Path == "" {
		return CreateArtifactInput{}, fmt.Errorf("%w: artifact path must not be empty", ErrInvalid)
	}
	if input.SizeBytes != nil && *input.SizeBytes < 0 {
		return CreateArtifactInput{}, fmt.Errorf("%w: artifact size must not be negative", ErrInvalid)
	}

	return input, nil
}

// repositoryExists checks whether the referenced repository row exists.
func (s *sqliteStore) repositoryExists(
	ctx context.Context,
	repositoryID int64,
) (bool, error) {
	return s.repositoryExistsWithQuery(ctx, s.database, repositoryID)
}

// repositoryExistsWithQuery checks repository existence using the provided
// query executor.
func (s *sqliteStore) repositoryExistsWithQuery(
	ctx context.Context,
	query queryer,
	repositoryID int64,
) (bool, error) {
	var count int
	if err := query.QueryRowContext(
		ctx,
		`SELECT COUNT(1) FROM repositories WHERE id = ?`,
		repositoryID,
	).Scan(&count); err != nil {
		return false, fmt.Errorf("query repository %d: %w", repositoryID, err)
	}

	return count > 0, nil
}

// mapSQLError translates common SQLite constraint failures into build-domain
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

	return fmt.Errorf("build store: %w", err)
}

	// boolToSQLite converts one boolean into the integer representation used by
	// SQLite tables.
func boolToSQLite(value bool) int64 {
	if value {
		return 1
	}

	return 0
}

// nullableString returns nil for blank strings so optional SQLite columns stay
// NULL when unset.
func nullableString(value string) any {
	value = strings.TrimSpace(value)
	if value == "" {
		return nil
	}

	return value
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

// stringPointer converts a nullable SQL string into an optional Go pointer.
func stringPointer(value sql.NullString) *string {
	if !value.Valid {
		return nil
	}

	copy := value.String
	return &copy
}
