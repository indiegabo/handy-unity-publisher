// Package automation contains background runtime coordination that keeps
// repositories, releases, and build workers moving without manual CLI steps.
package automation

import (
	"context"
	"errors"
	"fmt"
	"log/slog"
	"sync"
	"time"

	"github.com/indiegabo/handy-unity-bulder/internal/build"
	"github.com/indiegabo/handy-unity-bulder/internal/credentials"
	internalgit "github.com/indiegabo/handy-unity-bulder/internal/git"
	"github.com/indiegabo/handy-unity-bulder/internal/release"
	"github.com/indiegabo/handy-unity-bulder/internal/repository"
	"github.com/indiegabo/handy-unity-bulder/internal/worker"
)

// defaultAutomationSweepInterval, defaultReleaseDequeueWait, and
// defaultRepoPollingSeconds define the runtime automation cadence.
const (
	defaultAutomationSweepInterval = 5 * time.Second
	defaultReleaseDequeueWait      = 5 * time.Second
	defaultReleasePlanningLockTTL  = 30 * time.Minute
	defaultRepoPollingSeconds      = 300
)

// repoPollState tracks the next poll deadline for one repository inside the
// in-process automation coordinator.
type repoPollState struct {
	nextPollAt time.Time
}

// Coordinator keeps automatic repository polling and release planning active
// inside the main application runtime.
type Coordinator struct {
	logger             *slog.Logger
	repositories       repository.Store
	builds             build.Store
	credentials        credentials.Store
	releases           release.Store
	releaseDispatcher  *release.Dispatcher
	buildDispatcher    *build.Dispatcher
	queue              worker.Queue
	planningLocks      worker.LockManager
	tags               internalgit.TagSource
	sweepInterval      time.Duration
	releaseDequeueWait time.Duration
	repoStatesMu       sync.RWMutex
	repoStates         map[int64]repoPollState
}

// WithCoordination attaches distributed coordination primitives used to avoid
// duplicate release planning across overlapping automation loops.
func (c *Coordinator) WithCoordination(lockManager worker.LockManager) *Coordinator {
	c.planningLocks = lockManager
	return c
}

// NewCoordinator creates the default background automation coordinator.
func NewCoordinator(
	logger *slog.Logger,
	repositories repository.Store,
	builds build.Store,
	credentials credentials.Store,
	releases release.Store,
	releaseDispatcher *release.Dispatcher,
	buildDispatcher *build.Dispatcher,
	queue worker.Queue,
	tags internalgit.TagSource,
) *Coordinator {
	return &Coordinator{
		logger:             logger,
		repositories:       repositories,
		builds:             builds,
		credentials:        credentials,
		releases:           releases,
		releaseDispatcher:  releaseDispatcher,
		buildDispatcher:    buildDispatcher,
		queue:              queue,
		tags:               tags,
		sweepInterval:      defaultAutomationSweepInterval,
		releaseDequeueWait: defaultReleaseDequeueWait,
		repoStates:         make(map[int64]repoPollState),
	}
}

// Run starts the automatic recovery, release planning, and repository polling loops.
func (c *Coordinator) Run(ctx context.Context) error {
	if c.logger == nil {
		c.logger = slog.Default()
	}

	plannerDone := make(chan struct{})
	go func() {
		defer close(plannerDone)
		c.runReleasePlannerLoop(ctx)
	}()

	if err := c.recoverQueuedReleases(ctx); err != nil {
		c.logger.Error("recover queued releases", "error", err)
	}
	if err := c.sweepRepositories(ctx, time.Now()); err != nil {
		c.logger.Error("initial repository automation sweep", "error", err)
	}

	ticker := time.NewTicker(c.sweepInterval)
	defer ticker.Stop()

	for {
		select {
		case <-ctx.Done():
			<-plannerDone
			return nil
		case now := <-ticker.C:
			if err := c.sweepRepositories(ctx, now); err != nil {
				c.logger.Error("repository automation sweep failed", "error", err)
			}
		}
	}
}

// runReleasePlannerLoop continuously consumes queued release jobs and expands
// them into queued build runs until shutdown.
func (c *Coordinator) runReleasePlannerLoop(ctx context.Context) {
	for {
		select {
		case <-ctx.Done():
			return
		default:
		}

		payload, err := c.queue.Dequeue(ctx, release.QueueName, c.releaseDequeueWait)
		if err != nil {
			if ctx.Err() != nil {
				return
			}
			c.logger.Error("dequeue release job", "error", err)
			continue
		}
		if payload == nil {
			continue
		}

		job, err := release.UnmarshalJob(payload)
		if err != nil {
			c.logger.Error("decode release job", "error", err)
			continue
		}

		if err := c.waitForQueuedRelease(ctx, job.ReleaseRunID); err != nil {
			c.logger.Error(
				"wait for dequeued release to become queued",
				"release_run_id", job.ReleaseRunID,
				"repository_id", job.RepositoryID,
				"git_tag", job.GitTag,
				"error", err,
			)
			continue
		}

		if _, err := c.advanceRepositoryReleaseQueue(ctx, job.RepositoryID); err != nil {
			c.logger.Error(
				"advance release queue for dequeued release job",
				"release_run_id", job.ReleaseRunID,
				"repository_id", job.RepositoryID,
				"git_tag", job.GitTag,
				"error", err,
			)
		}
	}
}

// waitForQueuedRelease blocks briefly until a freshly dequeued release has its
// durable status advanced from detected to queued by the dispatch path.
func (c *Coordinator) waitForQueuedRelease(
	ctx context.Context,
	releaseRunID int64,
) error {
	for attempt := 0; attempt < 20; attempt++ {
		record, err := c.releases.Get(ctx, releaseRunID)
		if err != nil {
			return err
		}

		switch record.Status {
		case release.StatusQueued:
			return nil
		case release.StatusDetected:
			select {
			case <-ctx.Done():
				return ctx.Err()
			case <-time.After(50 * time.Millisecond):
			}
		default:
			return nil
		}
	}

	record, err := c.releases.Get(ctx, releaseRunID)
	if err != nil {
		return err
	}
	if record.Status == release.StatusDetected {
		return fmt.Errorf(
			"release run %d remained in %q after dequeue",
			releaseRunID,
			record.Status,
		)
	}

	return nil
}

// recoverQueuedReleases resumes at most one eligible queued release per
// repository when the runtime starts.
func (c *Coordinator) recoverQueuedReleases(ctx context.Context) error {
	records, err := c.releases.ListByStatus(ctx, release.StatusQueued)
	if err != nil {
		return err
	}

	seenRepositories := make(map[int64]struct{}, len(records))
	for _, record := range records {
		if _, ok := seenRepositories[record.RepositoryID]; ok {
			continue
		}
		seenRepositories[record.RepositoryID] = struct{}{}

		if _, err := c.advanceRepositoryReleaseQueue(ctx, record.RepositoryID); err != nil {
			c.logger.Error(
				"recover queued release",
				"repository_id", record.RepositoryID,
				"error", err,
			)
		}
	}

	return nil
}

// sweepRepositories evaluates enabled repositories, schedules polling based on
// per-repository cadence, and cleans state for repositories that disappeared
// or became inactive.
func (c *Coordinator) sweepRepositories(ctx context.Context, now time.Time) error {
	repos, err := c.repositories.List(ctx)
	if err != nil {
		return err
	}

	seen := make(map[int64]struct{}, len(repos))
	for _, repo := range repos {
		seen[repo.ID] = struct{}{}

		if !repo.Enabled {
			c.deleteRepoState(repo.ID)
			continue
		}

		targets, err := c.builds.ListEnabledTargetsByRepository(ctx, repo.ID)
		if err != nil {
			c.logger.Error(
				"list enabled build targets for repository",
				"repository_id", repo.ID,
				"error", err,
			)
			continue
		}
		if len(targets) == 0 {
			c.deleteRepoState(repo.ID)
			continue
		}

		busy, err := c.advanceRepositoryReleaseQueue(ctx, repo.ID)
		if err != nil {
			c.logger.Error(
				"advance repository release queue",
				"repository_id", repo.ID,
				"repo_url", repo.RepoURL,
				"error", err,
			)
			continue
		}
		if busy {
			continue
		}

		state, ok := c.getRepoState(repo.ID)
		if ok && now.Before(state.nextPollAt) {
			continue
		}

		queuedCount, err := c.pollRepository(ctx, repo)
		c.setRepoState(repo.ID, repoPollState{
			nextPollAt: now.Add(repoPollingInterval(repo)),
		})
		if err != nil {
			c.logger.Error(
				"poll repository for new tags",
				"repository_id", repo.ID,
				"repo_url", repo.RepoURL,
				"error", err,
			)
			continue
		}

		if queuedCount > 0 {
			if _, err := c.advanceRepositoryReleaseQueue(ctx, repo.ID); err != nil {
				c.logger.Error(
					"advance newly queued repository releases",
					"repository_id", repo.ID,
					"repo_url", repo.RepoURL,
					"error", err,
				)
			}
		}
	}

	c.pruneRepoStates(seen)

	return nil
}

// pollRepository checks one repository for all unseen tags in ascending order,
// dispatches releases when appropriate, and repairs the durable last-seen tag
// baseline after each accepted or already-seen tag.
func (c *Coordinator) pollRepository(
	ctx context.Context,
	repo repository.Record,
) (int, error) {
	auth, err := c.resolveRepositoryGitAuth(ctx, repo.CredentialsID)
	if err != nil {
		return 0, err
	}

	tags, err := c.tags.ListTags(ctx, repo.RepoURL, auth)
	if err != nil {
		return 0, err
	}

	selectedTags, _, ok := release.SelectQueuedRepositoryTags(tags, repo.LastSeenTag)
	if !ok {
		return 0, nil
	}

	queuedCount := 0
	for _, tag := range selectedTags {
		_, err := c.releaseDispatcher.DispatchRepositoryPoll(
			ctx,
			release.RepositoryPollDispatchInput{
				RepositoryID: repo.ID,
				GitTag:       tag.Name,
				GitCommit:    tag.Commit,
				ObservedVia:  "runtime-automation",
			},
		)
		if err != nil {
			if errors.Is(err, release.ErrBuildInProgress) {
				return queuedCount, nil
			}

			if errors.Is(err, release.ErrConflict) {
				if _, updateErr := c.repositories.UpdateLastSeenTag(ctx, repo.ID, tag.Name); updateErr != nil {
					return queuedCount, updateErr
				}
				continue
			}

			return queuedCount, err
		}

		if _, err := c.repositories.UpdateLastSeenTag(ctx, repo.ID, tag.Name); err != nil {
			return queuedCount, err
		}

		queuedCount++
	}

	return queuedCount, nil
}

// resolveRepositoryGitAuth loads optional repository credentials and converts
// them into Git CLI authentication flags.
func (c *Coordinator) resolveRepositoryGitAuth(
	ctx context.Context,
	credentialsID *int64,
) (internalgit.AuthOptions, error) {
	if credentialsID == nil {
		return internalgit.AuthOptions{}, nil
	}

	record, err := c.credentials.Get(ctx, *credentialsID)
	if err != nil {
		return internalgit.AuthOptions{}, err
	}

	return internalgit.AuthOptionsFromCredentials(record)
}

// planAndDispatchRelease expands one queued release into build runs and
// dispatches the queued subset to the build worker queue.
func (c *Coordinator) planAndDispatchRelease(ctx context.Context, releaseRunID int64) error {
	runs, err := c.builds.PlanRelease(ctx, releaseRunID)
	if err != nil {
		if errors.Is(err, build.ErrReleaseNotQueued) || errors.Is(err, build.ErrNoEnabledTargets) {
			return nil
		}

		return err
	}

	queuedRuns := make([]build.Run, 0, len(runs))
	for _, run := range runs {
		if run.Status != build.StatusQueued {
			continue
		}

		queuedRuns = append(queuedRuns, run)
	}

	if len(queuedRuns) == 0 {
		return nil
	}

	return c.buildDispatcher.DispatchMany(ctx, queuedRuns)
}

// advanceRepositoryReleaseQueue keeps at most one release build process active
// per repository while allowing already-completed queued releases to be
// skipped so the next tag in order can start.
func (c *Coordinator) advanceRepositoryReleaseQueue(
	ctx context.Context,
	repositoryID int64,
) (bool, error) {
	queuedReleases, err := c.releases.ListByStatus(ctx, release.StatusQueued)
	if err != nil {
		return false, err
	}

	for _, record := range queuedReleases {
		if record.RepositoryID != repositoryID {
			continue
		}

		runs, err := c.builds.ListBuildRunsByRelease(ctx, record.ID)
		if err != nil {
			return false, err
		}
		if repositoryBuildProcessActive(runs) {
			return true, nil
		}
		if len(runs) != 0 {
			continue
		}

		planningLock, planningBusy, err := c.acquireReleasePlanningLock(
			ctx,
			record.ID,
		)
		if err != nil {
			return false, err
		}
		if planningBusy {
			return true, nil
		}
		if planningLock != nil {
			defer func() {
				if err := planningLock.Release(context.Background()); err != nil {
					c.logger.Error(
						"release planning lock release failed",
						"release_run_id", record.ID,
						"repository_id", repositoryID,
						"error", err,
					)
				}
			}()
		}

		if err := c.planAndDispatchRelease(ctx, record.ID); err != nil {
			return false, err
		}

		plannedRuns, err := c.builds.ListBuildRunsByRelease(ctx, record.ID)
		if err != nil {
			return false, err
		}
		if repositoryBuildProcessActive(plannedRuns) {
			return true, nil
		}
	}

	return false, nil
}

// acquireReleasePlanningLock claims exclusive access to one release planning
// attempt so recovery, sweep, and dequeue paths cannot materialize the same
// repository tag concurrently.
func (c *Coordinator) acquireReleasePlanningLock(
	ctx context.Context,
	releaseRunID int64,
) (worker.Lock, bool, error) {
	if c.planningLocks == nil {
		return nil, false, nil
	}

	lock, ok, err := c.planningLocks.Acquire(
		ctx,
		fmt.Sprintf("release-plan:%d", releaseRunID),
		defaultReleasePlanningLockTTL,
	)
	if err != nil {
		return nil, false, err
	}
	if !ok {
		return nil, true, nil
	}

	return lock, false, nil
}

// repositoryBuildProcessActive reports whether one release already has queued
// or running build runs that must finish before the next tag can start.
func repositoryBuildProcessActive(runs []build.Run) bool {
	for _, run := range runs {
		switch run.Status {
		case build.StatusQueued, build.StatusRunning:
			return true
		}
	}

	return false
}

// getRepoState returns one repository poll schedule snapshot.
func (c *Coordinator) getRepoState(repositoryID int64) (repoPollState, bool) {
	c.repoStatesMu.RLock()
	defer c.repoStatesMu.RUnlock()

	state, ok := c.repoStates[repositoryID]
	return state, ok
}

// setRepoState stores one repository poll schedule snapshot.
func (c *Coordinator) setRepoState(repositoryID int64, state repoPollState) {
	c.repoStatesMu.Lock()
	defer c.repoStatesMu.Unlock()

	c.repoStates[repositoryID] = state
}

// deleteRepoState removes one repository poll schedule snapshot.
func (c *Coordinator) deleteRepoState(repositoryID int64) {
	c.repoStatesMu.Lock()
	defer c.repoStatesMu.Unlock()

	delete(c.repoStates, repositoryID)
}

// pruneRepoStates removes repository schedule entries that are no longer
// present in the active repository set.
func (c *Coordinator) pruneRepoStates(active map[int64]struct{}) {
	c.repoStatesMu.Lock()
	defer c.repoStatesMu.Unlock()

	for repositoryID := range c.repoStates {
		if _, ok := active[repositoryID]; ok {
			continue
		}

		delete(c.repoStates, repositoryID)
	}
}

// snapshotRepoStates returns a copy of the in-memory repository poll schedule
// map for concurrent runtime inspection.
func (c *Coordinator) snapshotRepoStates() map[int64]repoPollState {
	c.repoStatesMu.RLock()
	defer c.repoStatesMu.RUnlock()

	states := make(map[int64]repoPollState, len(c.repoStates))
	for repositoryID, state := range c.repoStates {
		states[repositoryID] = state
	}

	return states
}

// repoPollingInterval resolves the effective poll cadence for one repository,
// falling back to the runtime default when the stored interval is invalid.
func repoPollingInterval(repo repository.Record) time.Duration {
	seconds := repo.PollingIntervalSeconds
	if seconds <= 0 {
		seconds = defaultRepoPollingSeconds
	}

	return time.Duration(seconds) * time.Second
}
