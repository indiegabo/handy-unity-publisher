// Package release contains release-run persistence and orchestration entry
// points.
package release

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"strings"

	"github.com/indiegabo/handy-unity-bulder/internal/trigger"
)

const (
	// TriggerSourceManual marks release work explicitly requested by an operator.
	TriggerSourceManual = trigger.SourceManual
	// TriggerSourcePoll marks release work discovered by repository polling.
	TriggerSourcePoll = trigger.SourcePoll
	// TriggerSourceWebhook marks release work discovered by external callbacks.
	TriggerSourceWebhook = trigger.SourceWebhook

	// StatusDetected records release work accepted by the system before it is
	// handed off to the queue-backed execution path.
	StatusDetected = "detected"
	// StatusQueued is the first operational state for manually requested release
	// work after it is handed off to the queue-backed execution path.
	StatusQueued = "queued"
)

var (
	// ErrInvalid reports validation failures on release input.
	ErrInvalid = errors.New("invalid release input")
	// ErrNotFound reports missing release run records.
	ErrNotFound = errors.New("release run not found")
	// ErrConflict reports duplicate release dispatch attempts.
	ErrConflict = errors.New("release run conflict")
	// ErrBuildInProgress reports that the repository already has active build
	// work and cannot accept another manual dispatch yet.
	ErrBuildInProgress = errors.New("repository already has queued or running build work")
	// ErrRepositoryNotFound reports dispatch requests for unknown repositories.
	ErrRepositoryNotFound = errors.New("release repository not found")
)

// Record is the durable release-run row stored in SQLite.
type Record struct {
	ID                 int64   `json:"id"`
	RepositoryID       int64   `json:"repository_id"`
	GitTag             string  `json:"git_tag"`
	GitCommit          *string `json:"git_commit,omitempty"`
	TriggerSource      string  `json:"trigger_source"`
	TriggerRuleID      *int64  `json:"trigger_rule_id,omitempty"`
	SourceMetadataJSON string  `json:"source_metadata_json"`
	UnityVersion       *string `json:"unity_version,omitempty"`
	Status             string  `json:"status"`
	StartedAt          *string `json:"started_at,omitempty"`
	FinishedAt         *string `json:"finished_at,omitempty"`
	ErrorMessage       *string `json:"error_message,omitempty"`
	CreatedAt          string  `json:"created_at"`
	UpdatedAt          string  `json:"updated_at"`
}

// ManualDispatchInput defines the operator-provided fields for manual release
// creation.
type ManualDispatchInput struct {
	RepositoryID int64
	GitTag       string
	GitCommit    string
	RequestedVia string
}

// PollDispatchInput defines the fields accepted when repository polling
// discovers a release candidate.
type PollDispatchInput struct {
	RepositoryID  int64
	TriggerRuleID int64
	GitTag        string
	GitCommit     string
	ObservedVia   string
}

// RepositoryPollDispatchInput defines one release candidate discovered through
// repository-level polling automation instead of a stored trigger rule.
type RepositoryPollDispatchInput struct {
	RepositoryID int64
	GitTag       string
	GitCommit    string
	ObservedVia  string
}

// Store exposes the release-run operations currently needed by the CLI.
type Store interface {
	CreateManualDispatch(ctx context.Context, input ManualDispatchInput) (Record, error)
	RebuildManualDispatch(ctx context.Context, input ManualDispatchInput) (Record, error)
	CreatePollDispatch(ctx context.Context, input PollDispatchInput) (Record, error)
	CreateRepositoryPollDispatch(ctx context.Context, input RepositoryPollDispatchInput) (Record, error)
	Get(ctx context.Context, id int64) (Record, error)
	ListByStatus(ctx context.Context, statuses ...string) ([]Record, error)
	MarkQueued(ctx context.Context, id int64) (Record, error)
}

// NewStore creates the SQLite-backed release store.
func NewStore(database *sql.DB) Store {
	return &sqliteStore{database: database}
}

// sqliteStore persists release runs and dispatch metadata in SQLite.
type sqliteStore struct {
	database *sql.DB
}

// CreateManualDispatch inserts a detected release run with explicit manual
// trigger metadata.
func (s *sqliteStore) CreateManualDispatch(
	ctx context.Context,
	input ManualDispatchInput,
) (Record, error) {
	normalized, err := normalizeManualDispatchInput(input)
	if err != nil {
		return Record{}, err
	}

	repositoryExists, err := s.repositoryExists(ctx, normalized.RepositoryID)
	if err != nil {
		return Record{}, err
	}

	if !repositoryExists {
		return Record{}, ErrRepositoryNotFound
	}

	if err := s.rejectIfRepositoryBuildWorkActive(ctx, normalized.RepositoryID); err != nil {
		return Record{}, err
	}

	metadataJSON, err := manualDispatchMetadataJSON(normalized.RequestedVia)
	if err != nil {
		return Record{}, fmt.Errorf("encode release metadata: %w", err)
	}

	return s.insertManualDispatch(ctx, normalized, metadataJSON)
}

// RebuildManualDispatch reuses an existing release row when the repository and
// tag already exist, clears its derived build and publish state, and returns
// the release to the detected state so it can be queued again.
func (s *sqliteStore) RebuildManualDispatch(
	ctx context.Context,
	input ManualDispatchInput,
) (Record, error) {
	normalized, err := normalizeManualDispatchInput(input)
	if err != nil {
		return Record{}, err
	}

	repositoryExists, err := s.repositoryExists(ctx, normalized.RepositoryID)
	if err != nil {
		return Record{}, err
	}
	if !repositoryExists {
		return Record{}, ErrRepositoryNotFound
	}

	if err := s.rejectIfRepositoryBuildWorkActive(ctx, normalized.RepositoryID); err != nil {
		return Record{}, err
	}

	metadataJSON, err := manualDispatchMetadataJSON(normalized.RequestedVia)
	if err != nil {
		return Record{}, fmt.Errorf("encode release metadata: %w", err)
	}

	existingID, err := s.releaseRunIDByRepositoryAndTag(
		ctx,
		normalized.RepositoryID,
		normalized.GitTag,
	)
	if err != nil {
		if errors.Is(err, ErrNotFound) {
			return s.insertManualDispatch(ctx, normalized, metadataJSON)
		}

		return Record{}, err
	}

	tx, err := s.database.BeginTx(ctx, nil)
	if err != nil {
		return Record{}, fmt.Errorf("begin manual release rebuild transaction: %w", err)
	}
	defer tx.Rollback()

	if _, err := tx.ExecContext(
		ctx,
		`DELETE FROM build_runs WHERE release_run_id = ?`,
		existingID,
	); err != nil {
		return Record{}, fmt.Errorf(
			"clear build runs for release %d: %w",
			existingID,
			err,
		)
	}

	if _, err := tx.ExecContext(
		ctx,
		`UPDATE release_runs
		SET
			git_commit = ?,
			trigger_source = ?,
			trigger_rule_id = NULL,
			source_metadata_json = ?,
			unity_version = NULL,
			status = ?,
			started_at = NULL,
			finished_at = NULL,
			error_message = NULL,
			updated_at = CURRENT_TIMESTAMP
		WHERE id = ?`,
		nullableString(normalized.GitCommit),
		TriggerSourceManual,
		metadataJSON,
		StatusDetected,
		existingID,
	); err != nil {
		return Record{}, fmt.Errorf(
			"reset release run %d for rebuild: %w",
			existingID,
			err,
		)
	}

	if err := tx.Commit(); err != nil {
		return Record{}, fmt.Errorf("commit manual release rebuild transaction: %w", err)
	}

	return s.Get(ctx, existingID)
}

// insertManualDispatch inserts one manual release row after the repository and
// active-build checks already succeeded.
func (s *sqliteStore) insertManualDispatch(
	ctx context.Context,
	input ManualDispatchInput,
	metadataJSON string,
) (Record, error) {
	result, err := s.database.ExecContext(
		ctx,
		`INSERT INTO release_runs (
			repository_id,
			git_tag,
			git_commit,
			trigger_source,
			source_metadata_json,
			status
		) VALUES (?, ?, ?, ?, ?, ?)`,
		input.RepositoryID,
		input.GitTag,
		nullableString(input.GitCommit),
		TriggerSourceManual,
		metadataJSON,
		StatusDetected,
	)
	if err != nil {
		return Record{}, mapSQLError(err)
	}

	id, err := result.LastInsertId()
	if err != nil {
		return Record{}, fmt.Errorf("read release run id: %w", err)
	}

	return s.Get(ctx, id)
}

// CreatePollDispatch inserts a detected release run with polling metadata and
// the owning trigger rule.
func (s *sqliteStore) CreatePollDispatch(
	ctx context.Context,
	input PollDispatchInput,
) (Record, error) {
	normalized, err := normalizePollDispatchInput(input)
	if err != nil {
		return Record{}, err
	}

	repositoryExists, err := s.repositoryExists(ctx, normalized.RepositoryID)
	if err != nil {
		return Record{}, err
	}

	if !repositoryExists {
		return Record{}, ErrRepositoryNotFound
	}

	if err := s.rejectIfRepositoryBuildWorkActive(ctx, normalized.RepositoryID); err != nil {
		return Record{}, err
	}

	triggerRuleExists, err := s.pollTriggerRuleExists(
		ctx,
		normalized.TriggerRuleID,
		normalized.RepositoryID,
	)
	if err != nil {
		return Record{}, err
	}

	if !triggerRuleExists {
		return Record{}, fmt.Errorf(
			"%w: poll trigger rule does not belong to repository",
			ErrInvalid,
		)
	}

	metadataJSON, err := pollDispatchMetadataJSON(normalized.ObservedVia)
	if err != nil {
		return Record{}, fmt.Errorf("encode release metadata: %w", err)
	}

	result, err := s.database.ExecContext(
		ctx,
		`INSERT INTO release_runs (
			repository_id,
			git_tag,
			git_commit,
			trigger_source,
			trigger_rule_id,
			source_metadata_json,
			status
		) VALUES (?, ?, ?, ?, ?, ?, ?)`,
		normalized.RepositoryID,
		normalized.GitTag,
		nullableString(normalized.GitCommit),
		TriggerSourcePoll,
		normalized.TriggerRuleID,
		metadataJSON,
		StatusDetected,
	)
	if err != nil {
		return Record{}, mapSQLError(err)
	}

	id, err := result.LastInsertId()
	if err != nil {
		return Record{}, fmt.Errorf("read release run id: %w", err)
	}

	return s.Get(ctx, id)
}

// CreateRepositoryPollDispatch inserts a detected release run discovered by
// repository-level polling without a persisted trigger-rule record.
func (s *sqliteStore) CreateRepositoryPollDispatch(
	ctx context.Context,
	input RepositoryPollDispatchInput,
) (Record, error) {
	normalized, err := normalizeRepositoryPollDispatchInput(input)
	if err != nil {
		return Record{}, err
	}

	repositoryExists, err := s.repositoryExists(ctx, normalized.RepositoryID)
	if err != nil {
		return Record{}, err
	}

	if !repositoryExists {
		return Record{}, ErrRepositoryNotFound
	}

	if err := s.rejectIfRepositoryBuildWorkActive(ctx, normalized.RepositoryID); err != nil {
		return Record{}, err
	}

	metadataJSON, err := pollDispatchMetadataJSON(normalized.ObservedVia)
	if err != nil {
		return Record{}, fmt.Errorf("encode release metadata: %w", err)
	}

	result, err := s.database.ExecContext(
		ctx,
		`INSERT INTO release_runs (
			repository_id,
			git_tag,
			git_commit,
			trigger_source,
			source_metadata_json,
			status
		) VALUES (?, ?, ?, ?, ?, ?)`,
		normalized.RepositoryID,
		normalized.GitTag,
		nullableString(normalized.GitCommit),
		TriggerSourcePoll,
		metadataJSON,
		StatusDetected,
	)
	if err != nil {
		return Record{}, mapSQLError(err)
	}

	id, err := result.LastInsertId()
	if err != nil {
		return Record{}, fmt.Errorf("read release run id: %w", err)
	}

	return s.Get(ctx, id)
}

// Get loads one release run by identifier.
func (s *sqliteStore) Get(ctx context.Context, id int64) (Record, error) {
	row := s.database.QueryRowContext(
		ctx,
		`SELECT
			id,
			repository_id,
			git_tag,
			git_commit,
			trigger_source,
			trigger_rule_id,
			source_metadata_json,
			unity_version,
			status,
			started_at,
			finished_at,
			error_message,
			created_at,
			updated_at
		FROM release_runs
		WHERE id = ?`,
		id,
	)

	record, err := scanRecord(row)
	if err != nil {
		if errors.Is(err, sql.ErrNoRows) {
			return Record{}, ErrNotFound
		}

		return Record{}, fmt.Errorf("query release run %d: %w", id, err)
	}

	return record, nil
}

// ListByStatus returns release runs whose status matches one of the requested values.
func (s *sqliteStore) ListByStatus(ctx context.Context, statuses ...string) ([]Record, error) {
	if len(statuses) == 0 {
		return nil, fmt.Errorf("%w: at least one status is required", ErrInvalid)
	}

	placeholders := make([]string, 0, len(statuses))
	args := make([]any, 0, len(statuses))
	for _, status := range statuses {
		normalized := strings.TrimSpace(status)
		if normalized == "" {
			return nil, fmt.Errorf("%w: status must not be empty", ErrInvalid)
		}

		placeholders = append(placeholders, "?")
		args = append(args, normalized)
	}

	query := fmt.Sprintf(
		`SELECT
			id,
			repository_id,
			git_tag,
			git_commit,
			trigger_source,
			trigger_rule_id,
			source_metadata_json,
			unity_version,
			status,
			started_at,
			finished_at,
			error_message,
			created_at,
			updated_at
		FROM release_runs
		WHERE status IN (%s)
		ORDER BY id ASC`,
		strings.Join(placeholders, ", "),
	)

	rows, err := s.database.QueryContext(ctx, query, args...)
	if err != nil {
		return nil, fmt.Errorf("list release runs by status: %w", err)
	}
	defer rows.Close()

	records := make([]Record, 0)
	for rows.Next() {
		record, err := scanRecord(rows)
		if err != nil {
			return nil, fmt.Errorf("scan release run row: %w", err)
		}

		records = append(records, record)
	}

	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("iterate release runs: %w", err)
	}

	return records, nil
}

// MarkQueued records that a release run has been handed off to the queue.
func (s *sqliteStore) MarkQueued(ctx context.Context, id int64) (Record, error) {
	result, err := s.database.ExecContext(
		ctx,
		`UPDATE release_runs
		SET
			status = ?,
			updated_at = CURRENT_TIMESTAMP
		WHERE id = ?`,
		StatusQueued,
		id,
	)
	if err != nil {
		return Record{}, fmt.Errorf("mark release run %d queued: %w", id, err)
	}

	rowsAffected, err := result.RowsAffected()
	if err != nil {
		return Record{}, fmt.Errorf("read queued rows: %w", err)
	}

	if rowsAffected == 0 {
		return Record{}, ErrNotFound
	}

	return s.Get(ctx, id)
}

// scanner abstracts row scanners shared by single-row and multi-row queries.
type scanner interface {
	Scan(dest ...any) error
}

// scanRecord decodes one release run row into the public release model.
func scanRecord(row scanner) (Record, error) {
	var record Record
	var gitCommit sql.NullString
	var triggerRuleID sql.NullInt64
	var unityVersion sql.NullString
	var startedAt sql.NullString
	var finishedAt sql.NullString
	var errorMessage sql.NullString

	err := row.Scan(
		&record.ID,
		&record.RepositoryID,
		&record.GitTag,
		&gitCommit,
		&record.TriggerSource,
		&triggerRuleID,
		&record.SourceMetadataJSON,
		&unityVersion,
		&record.Status,
		&startedAt,
		&finishedAt,
		&errorMessage,
		&record.CreatedAt,
		&record.UpdatedAt,
	)
	if err != nil {
		return Record{}, err
	}

	record.GitCommit = stringPointer(gitCommit)
	record.TriggerRuleID = int64Pointer(triggerRuleID)
	record.UnityVersion = stringPointer(unityVersion)
	record.StartedAt = stringPointer(startedAt)
	record.FinishedAt = stringPointer(finishedAt)
	record.ErrorMessage = stringPointer(errorMessage)

	return record, nil
}

// normalizeManualDispatchInput trims and validates one manual dispatch
// payload.
func normalizeManualDispatchInput(
	input ManualDispatchInput,
) (ManualDispatchInput, error) {
	input.GitTag = strings.TrimSpace(input.GitTag)
	input.GitCommit = strings.TrimSpace(input.GitCommit)
	input.RequestedVia = strings.TrimSpace(input.RequestedVia)

	if input.RepositoryID <= 0 {
		return ManualDispatchInput{}, fmt.Errorf(
			"%w: repository_id must be greater than zero",
			ErrInvalid,
		)
	}

	if input.GitTag == "" {
		return ManualDispatchInput{}, fmt.Errorf(
			"%w: git_tag must not be empty",
			ErrInvalid,
		)
	}

	return input, nil
}

// normalizePollDispatchInput trims and validates one poll-trigger dispatch
// payload.
func normalizePollDispatchInput(input PollDispatchInput) (PollDispatchInput, error) {
	input.GitTag = strings.TrimSpace(input.GitTag)
	input.GitCommit = strings.TrimSpace(input.GitCommit)
	input.ObservedVia = strings.TrimSpace(input.ObservedVia)

	if input.RepositoryID <= 0 {
		return PollDispatchInput{}, fmt.Errorf(
			"%w: repository_id must be greater than zero",
			ErrInvalid,
		)
	}

	if input.TriggerRuleID <= 0 {
		return PollDispatchInput{}, fmt.Errorf(
			"%w: trigger_rule_id must be greater than zero",
			ErrInvalid,
		)
	}

	if input.GitTag == "" {
		return PollDispatchInput{}, fmt.Errorf(
			"%w: git_tag must not be empty",
			ErrInvalid,
		)
	}

	return input, nil
}

// normalizeRepositoryPollDispatchInput trims and validates one repository-level
// poll dispatch payload.
func normalizeRepositoryPollDispatchInput(
	input RepositoryPollDispatchInput,
) (RepositoryPollDispatchInput, error) {
	input.GitTag = strings.TrimSpace(input.GitTag)
	input.GitCommit = strings.TrimSpace(input.GitCommit)
	input.ObservedVia = strings.TrimSpace(input.ObservedVia)

	if input.RepositoryID <= 0 {
		return RepositoryPollDispatchInput{}, fmt.Errorf(
			"%w: repository_id must be greater than zero",
			ErrInvalid,
		)
	}

	if input.GitTag == "" {
		return RepositoryPollDispatchInput{}, fmt.Errorf(
			"%w: git_tag must not be empty",
			ErrInvalid,
		)
	}

	return input, nil
}

// manualDispatchMetadataJSON encodes optional metadata captured for manual
// release dispatches.
func manualDispatchMetadataJSON(requestedVia string) (string, error) {
	metadata := map[string]string{}
	if requestedVia != "" {
		metadata["requested_via"] = requestedVia
	}

	encoded, err := json.Marshal(metadata)
	if err != nil {
		return "", err
	}

	return string(encoded), nil
}

// pollDispatchMetadataJSON encodes optional metadata captured for polling
// release dispatches.
func pollDispatchMetadataJSON(observedVia string) (string, error) {
	metadata := map[string]string{}
	if observedVia != "" {
		metadata["observed_via"] = observedVia
	}

	encoded, err := json.Marshal(metadata)
	if err != nil {
		return "", err
	}

	return string(encoded), nil
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

// repositoryName loads the durable repository name for operator-facing error
// messages and validation context.
func (s *sqliteStore) repositoryName(
	ctx context.Context,
	repositoryID int64,
) (string, error) {
	var repositoryName string
	if err := s.database.QueryRowContext(
		ctx,
		`SELECT name FROM repositories WHERE id = ?`,
		repositoryID,
	).Scan(&repositoryName); err != nil {
		if errors.Is(err, sql.ErrNoRows) {
			return "", ErrRepositoryNotFound
		}

		return "", fmt.Errorf("query repository name %d: %w", repositoryID, err)
	}

	return repositoryName, nil
}

// releaseRunIDByRepositoryAndTag resolves one unique release row from the
// repository and git tag pair.
func (s *sqliteStore) releaseRunIDByRepositoryAndTag(
	ctx context.Context,
	repositoryID int64,
	gitTag string,
) (int64, error) {
	gitTag = strings.TrimSpace(gitTag)
	if gitTag == "" {
		return 0, fmt.Errorf("%w: git_tag must not be empty", ErrInvalid)
	}

	var releaseRunID int64
	if err := s.database.QueryRowContext(
		ctx,
		`SELECT id
		FROM release_runs
		WHERE repository_id = ? AND git_tag = ?`,
		repositoryID,
		gitTag,
	).Scan(&releaseRunID); err != nil {
		if errors.Is(err, sql.ErrNoRows) {
			return 0, ErrNotFound
		}

		return 0, fmt.Errorf(
			"query release run for repository %d tag %q: %w",
			repositoryID,
			gitTag,
			err,
		)
	}

	return releaseRunID, nil
}

// rejectIfRepositoryBuildWorkActive blocks new release intake while the
// repository still has queued or running build work.
func (s *sqliteStore) rejectIfRepositoryBuildWorkActive(
	ctx context.Context,
	repositoryID int64,
) error {
	hasActiveBuildWork, err := s.repositoryHasActiveBuildWork(ctx, repositoryID)
	if err != nil {
		return err
	}
	if !hasActiveBuildWork {
		return nil
	}

	repositoryName, err := s.repositoryName(ctx, repositoryID)
	if err != nil {
		return err
	}

	return fmt.Errorf(
		"%w for repository %q",
		ErrBuildInProgress,
		repositoryName,
	)
}

// repositoryHasActiveBuildWork reports whether the repository already has a
// queued or running build run that should block another manual dispatch.
func (s *sqliteStore) repositoryHasActiveBuildWork(
	ctx context.Context,
	repositoryID int64,
) (bool, error) {
	var count int
	if err := s.database.QueryRowContext(
		ctx,
		`SELECT COUNT(1)
		FROM build_runs br
		JOIN release_runs rr ON rr.id = br.release_run_id
		WHERE rr.repository_id = ? AND br.status IN (?, ?)`,
		repositoryID,
		"queued",
		"running",
	).Scan(&count); err != nil {
		return false, fmt.Errorf(
			"query active build work for repository %d: %w",
			repositoryID,
			err,
		)
	}

	return count > 0, nil
}

// pollTriggerRuleExists checks whether the referenced poll trigger rule belongs
// to the requested repository.
func (s *sqliteStore) pollTriggerRuleExists(
	ctx context.Context,
	triggerRuleID int64,
	repositoryID int64,
) (bool, error) {
	var count int
	if err := s.database.QueryRowContext(
		ctx,
		`SELECT COUNT(1)
		FROM trigger_rules
		WHERE id = ? AND repository_id = ? AND source = ?`,
		triggerRuleID,
		repositoryID,
		TriggerSourcePoll,
	).Scan(&count); err != nil {
		return false, fmt.Errorf(
			"query poll trigger rule %d for repository %d: %w",
			triggerRuleID,
			repositoryID,
			err,
		)
	}

	return count > 0, nil
}

// mapSQLError translates common SQLite constraint failures into release-domain
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

	return fmt.Errorf("release store: %w", err)
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
