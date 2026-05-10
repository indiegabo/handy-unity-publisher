// Package automation contains background runtime coordination that keeps
// repositories, releases, and build workers moving without manual CLI steps.
package automation

import (
	"context"
	"fmt"
	"time"

	"github.com/indiegabo/handy-unity-bulder/internal/build"
	"github.com/indiegabo/handy-unity-bulder/internal/release"
)

const (
	// PollStateInactive reports that one repository is not eligible for polling.
	PollStateInactive = "inactive"
	// PollStateReady reports that one repository can be polled immediately.
	PollStateReady = "ready"
	// PollStateScheduled reports that one repository is idle and waiting for its
	// next poll deadline.
	PollStateScheduled = "scheduled"
	// PollStatePaused reports that polling is intentionally suspended for one
	// repository until its current release backlog drains.
	PollStatePaused = "paused"

	// PollStateReasonRepositoryDisabled reports that runtime polling is inactive
	// because the repository itself is disabled.
	PollStateReasonRepositoryDisabled = "repository_disabled"
	// PollStateReasonNoEnabledBuildTargets reports that runtime polling is
	// inactive because the repository has no enabled build targets.
	PollStateReasonNoEnabledBuildTargets = "no_enabled_build_targets"
	// PollStateReasonActiveReleaseBacklog reports that runtime polling is paused
	// because the repository still has queued or running release build work.
	PollStateReasonActiveReleaseBacklog = "active_release_backlog"
)

// Reporter exposes runtime automation snapshots to HTTP handlers and operator
// tooling.
type Reporter interface {
	Snapshot(ctx context.Context) (RuntimeReport, error)
}

// RuntimeReport summarizes the current per-repository polling and release
// backlog state maintained by the automation coordinator.
type RuntimeReport struct {
	GeneratedAt  string                    `json:"generated_at"`
	Repositories []RepositoryRuntimeStatus `json:"repositories"`
}

// RepositoryRuntimeStatus reports the current polling status and queued release
// backlog for one repository.
type RepositoryRuntimeStatus struct {
	RepositoryID            int64                 `json:"repository_id"`
	RepositoryName          string                `json:"repository_name"`
	Enabled                 bool                  `json:"enabled"`
	EnabledBuildTargetCount int                   `json:"enabled_build_target_count"`
	PollingIntervalSeconds  int                   `json:"polling_interval_seconds"`
	LastSeenTag             *string               `json:"last_seen_tag,omitempty"`
	PollState               string                `json:"poll_state"`
	Reason                  *string               `json:"reason,omitempty"`
	NextPollAt              *string               `json:"next_poll_at,omitempty"`
	PendingReleaseCount     int                   `json:"pending_release_count"`
	ReleaseQueue            []QueuedReleaseStatus `json:"release_queue,omitempty"`
}

// QueuedReleaseStatus reports one pending release in the repository-local
// execution order seen by the automation coordinator.
type QueuedReleaseStatus struct {
	ReleaseRunID       int64  `json:"release_run_id"`
	GitTag             string `json:"git_tag"`
	Planned            bool   `json:"planned"`
	BuildProcessActive bool   `json:"build_process_active"`
	QueuedBuildRuns    int    `json:"queued_build_runs"`
	RunningBuildRuns   int    `json:"running_build_runs"`
	TerminalBuildRuns  int    `json:"terminal_build_runs"`
	TotalBuildRuns     int    `json:"total_build_runs"`
}

// Snapshot returns one current runtime automation report suitable for HTTP and
// CLI inspection.
func (c *Coordinator) Snapshot(ctx context.Context) (RuntimeReport, error) {
	repositories, err := c.repositories.List(ctx)
	if err != nil {
		return RuntimeReport{}, fmt.Errorf(
			"list repositories for automation runtime report: %w",
			err,
		)
	}

	queuedReleases, err := c.releases.ListByStatus(ctx, release.StatusQueued)
	if err != nil {
		return RuntimeReport{}, fmt.Errorf(
			"list queued releases for automation runtime report: %w",
			err,
		)
	}

	queuedByRepository := make(map[int64][]release.Record, len(queuedReleases))
	for _, record := range queuedReleases {
		queuedByRepository[record.RepositoryID] = append(
			queuedByRepository[record.RepositoryID],
			record,
		)
	}

	now := time.Now().UTC()
	repoStates := c.snapshotRepoStates()
	report := RuntimeReport{
		GeneratedAt:  now.Format(time.RFC3339),
		Repositories: make([]RepositoryRuntimeStatus, 0, len(repositories)),
	}

	for _, repo := range repositories {
		targets, err := c.builds.ListEnabledTargetsByRepository(ctx, repo.ID)
		if err != nil {
			return RuntimeReport{}, fmt.Errorf(
				"list enabled build targets for repository %d in automation runtime report: %w",
				repo.ID,
				err,
			)
		}

		pendingQueue := make([]QueuedReleaseStatus, 0)
		for _, record := range queuedByRepository[repo.ID] {
			runs, err := c.builds.ListBuildRunsByRelease(ctx, record.ID)
			if err != nil {
				return RuntimeReport{}, fmt.Errorf(
					"list build runs for release %d in automation runtime report: %w",
					record.ID,
					err,
				)
			}
			if !releaseNeedsAttention(runs) {
				continue
			}

			pendingQueue = append(
				pendingQueue,
				summarizeQueuedReleaseStatus(record, runs),
			)
		}

		status := RepositoryRuntimeStatus{
			RepositoryID:            repo.ID,
			RepositoryName:          repo.Name,
			Enabled:                 repo.Enabled,
			EnabledBuildTargetCount: len(targets),
			PollingIntervalSeconds:  repo.PollingIntervalSeconds,
			LastSeenTag:             cloneStringPointer(repo.LastSeenTag),
			PendingReleaseCount:     len(pendingQueue),
			ReleaseQueue:            pendingQueue,
		}

		switch {
		case !repo.Enabled:
			status.PollState = PollStateInactive
			status.Reason = stringPointer(PollStateReasonRepositoryDisabled)
		case len(targets) == 0:
			status.PollState = PollStateInactive
			status.Reason = stringPointer(PollStateReasonNoEnabledBuildTargets)
		case len(pendingQueue) > 0:
			status.PollState = PollStatePaused
			status.Reason = stringPointer(PollStateReasonActiveReleaseBacklog)
		default:
			repoState, ok := repoStates[repo.ID]
			if !ok || repoState.nextPollAt.IsZero() || !now.Before(repoState.nextPollAt) {
				status.PollState = PollStateReady
				break
			}

			status.PollState = PollStateScheduled
			nextPollAt := repoState.nextPollAt.UTC().Format(time.RFC3339)
			status.NextPollAt = &nextPollAt
		}

		report.Repositories = append(report.Repositories, status)
	}

	return report, nil
}

// releaseNeedsAttention reports whether one queued release is still blocking
// or waiting for repository-local execution.
func releaseNeedsAttention(runs []build.Run) bool {
	return len(runs) == 0 || repositoryBuildProcessActive(runs)
}

// summarizeQueuedReleaseStatus converts one queued release and its build runs
// into the operator-facing runtime queue report shape.
func summarizeQueuedReleaseStatus(
	record release.Record,
	runs []build.Run,
) QueuedReleaseStatus {
	status := QueuedReleaseStatus{
		ReleaseRunID:       record.ID,
		GitTag:             record.GitTag,
		Planned:            len(runs) > 0,
		BuildProcessActive: repositoryBuildProcessActive(runs),
		TotalBuildRuns:     len(runs),
	}

	for _, run := range runs {
		switch run.Status {
		case build.StatusQueued:
			status.QueuedBuildRuns++
		case build.StatusRunning:
			status.RunningBuildRuns++
		default:
			status.TerminalBuildRuns++
		}
	}

	return status
}

// cloneStringPointer returns a detached copy of one optional string pointer.
func cloneStringPointer(value *string) *string {
	if value == nil {
		return nil
	}

	cloned := *value
	return &cloned
}

// stringPointer returns one allocated string pointer for operator-facing JSON
// responses.
func stringPointer(value string) *string {
	cloned := value
	return &cloned
}
