package release_test

import (
	"context"
	"testing"

	"github.com/indiegabo/handy-unity-bulder/internal/build"
	internalgit "github.com/indiegabo/handy-unity-bulder/internal/git"
	"github.com/indiegabo/handy-unity-bulder/internal/release"
	"github.com/indiegabo/handy-unity-bulder/internal/repository"
	"github.com/indiegabo/handy-unity-bulder/internal/trigger"
)

func TestPollerQueuesDiscoveredTagsAndUpdatesLastSeenTag(t *testing.T) {
	t.Parallel()

	ctx := context.Background()
	database := newDispatchTestDatabase(t)
	repositoryStore := repository.NewStore(database)
	triggerStore := trigger.NewStore(database)
	releaseStore := release.NewStore(database)
	queue := &queueStub{}
	poller := release.NewPoller(
		repositoryStore,
		triggerStore,
		release.NewDispatcher(releaseStore, queue),
		&tagSourceStub{tags: []internalgit.Tag{{Name: "v1.0.0", Commit: "111"}, {Name: "v1.1.0", Commit: "222"}}},
	)

	repo, err := repositoryStore.Create(ctx, repository.CreateInput{
		Name:    "polled-repo",
		RepoURL: "https://example.com/org/polled.git",
	})
	if err != nil {
		t.Fatalf("create repository: %v", err)
	}

	rule, err := triggerStore.Create(ctx, trigger.CreateInput{
		RepositoryID: repo.ID,
		Name:         "default-poll",
		Source:       trigger.SourcePoll,
	})
	if err != nil {
		t.Fatalf("create trigger rule: %v", err)
	}

	result, err := poller.PollRule(ctx, rule.ID)
	if err != nil {
		t.Fatalf("poll rule: %v", err)
	}

	if result.Status != release.PollStatusQueued {
		t.Fatalf("expected queued poll status, got %q", result.Status)
	}

	if len(result.Tags) != 2 {
		t.Fatalf("expected two queued tags, got %#v", result.Tags)
	}
	if result.Tags[0].Name != "v1.0.0" || result.Tags[1].Name != "v1.1.0" {
		t.Fatalf("expected queued tags v1.0.0 then v1.1.0, got %#v", result.Tags)
	}

	if result.Tag == nil || result.Tag.Name != "v1.1.0" {
		t.Fatalf("expected latest queued tag v1.1.0, got %#v", result.Tag)
	}

	if len(result.Releases) != 2 {
		t.Fatalf("expected two queued releases, got %#v", result.Releases)
	}
	if result.Release == nil || result.Release.GitTag != "v1.1.0" {
		t.Fatalf("expected latest queued poll release for v1.1.0, got %#v", result.Release)
	}
	for _, queuedRelease := range result.Releases {
		if queuedRelease.TriggerSource != release.TriggerSourcePoll {
			t.Fatalf("expected queued poll release, got %#v", queuedRelease)
		}
	}

	loadedRepo, err := repositoryStore.Get(ctx, repo.ID)
	if err != nil {
		t.Fatalf("get repository: %v", err)
	}

	if loadedRepo.LastSeenTag == nil || *loadedRepo.LastSeenTag != "v1.1.0" {
		t.Fatalf("expected repository last seen tag v1.1.0, got %#v", loadedRepo.LastSeenTag)
	}

	if len(queue.payloads) != 2 {
		t.Fatalf("expected two queued payloads, got %d", len(queue.payloads))
	}
}

func TestPollerReturnsUnchangedWhenLastSeenTagMatchesLatest(t *testing.T) {
	t.Parallel()

	ctx := context.Background()
	database := newDispatchTestDatabase(t)
	repositoryStore := repository.NewStore(database)
	triggerStore := trigger.NewStore(database)
	releaseStore := release.NewStore(database)
	queue := &queueStub{}
	poller := release.NewPoller(
		repositoryStore,
		triggerStore,
		release.NewDispatcher(releaseStore, queue),
		&tagSourceStub{tags: []internalgit.Tag{{Name: "v1.0.0", Commit: "111"}, {Name: "v1.1.0", Commit: "222"}}},
	)

	repo, err := repositoryStore.Create(ctx, repository.CreateInput{
		Name:    "steady-repo",
		RepoURL: "https://example.com/org/steady.git",
	})
	if err != nil {
		t.Fatalf("create repository: %v", err)
	}

	if _, err := repositoryStore.UpdateLastSeenTag(ctx, repo.ID, "v1.1.0"); err != nil {
		t.Fatalf("seed repository last seen tag: %v", err)
	}

	rule, err := triggerStore.Create(ctx, trigger.CreateInput{
		RepositoryID: repo.ID,
		Name:         "steady-poll",
		Source:       trigger.SourcePoll,
	})
	if err != nil {
		t.Fatalf("create trigger rule: %v", err)
	}

	result, err := poller.PollRule(ctx, rule.ID)
	if err != nil {
		t.Fatalf("poll rule: %v", err)
	}

	if result.Status != release.PollStatusUnchanged {
		t.Fatalf("expected unchanged poll status, got %q", result.Status)
	}

	if result.Release != nil {
		t.Fatalf("expected no release for unchanged poll, got %#v", result.Release)
	}

	if len(queue.payloads) != 0 {
		t.Fatalf("expected no queued payloads, got %d", len(queue.payloads))
	}
}

func TestPollerQueuesEveryUnseenTagAfterLastSeenBaseline(t *testing.T) {
	t.Parallel()

	ctx := context.Background()
	database := newDispatchTestDatabase(t)
	repositoryStore := repository.NewStore(database)
	triggerStore := trigger.NewStore(database)
	releaseStore := release.NewStore(database)
	queue := &queueStub{}
	poller := release.NewPoller(
		repositoryStore,
		triggerStore,
		release.NewDispatcher(releaseStore, queue),
		&tagSourceStub{tags: []internalgit.Tag{{Name: "v1.0.0", Commit: "111"}, {Name: "v1.1.0", Commit: "222"}, {Name: "v1.2.0", Commit: "333"}}},
	)

	repo, err := repositoryStore.Create(ctx, repository.CreateInput{
		Name:    "progressive-repo",
		RepoURL: "https://example.com/org/progressive.git",
	})
	if err != nil {
		t.Fatalf("create repository: %v", err)
	}

	if _, err := repositoryStore.UpdateLastSeenTag(ctx, repo.ID, "v1.0.0"); err != nil {
		t.Fatalf("seed repository last seen tag: %v", err)
	}

	rule, err := triggerStore.Create(ctx, trigger.CreateInput{
		RepositoryID: repo.ID,
		Name:         "progressive-poll",
		Source:       trigger.SourcePoll,
	})
	if err != nil {
		t.Fatalf("create trigger rule: %v", err)
	}

	result, err := poller.PollRule(ctx, rule.ID)
	if err != nil {
		t.Fatalf("poll rule: %v", err)
	}

	if len(result.Tags) != 2 {
		t.Fatalf("expected two unseen tags after baseline, got %#v", result.Tags)
	}
	if result.Tags[0].Name != "v1.1.0" || result.Tags[1].Name != "v1.2.0" {
		t.Fatalf("expected queued tags v1.1.0 then v1.2.0, got %#v", result.Tags)
	}
	if len(result.Releases) != 2 {
		t.Fatalf("expected two queued releases, got %#v", result.Releases)
	}
	if len(queue.payloads) != 2 {
		t.Fatalf("expected two queued payloads, got %d", len(queue.payloads))
	}

	loadedRepo, err := repositoryStore.Get(ctx, repo.ID)
	if err != nil {
		t.Fatalf("get repository: %v", err)
	}
	if loadedRepo.LastSeenTag == nil || *loadedRepo.LastSeenTag != "v1.2.0" {
		t.Fatalf("expected repository last seen tag v1.2.0, got %#v", loadedRepo.LastSeenTag)
	}
}

func TestPollerRepairsLastSeenTagWhenReleaseAlreadyExists(t *testing.T) {
	t.Parallel()

	ctx := context.Background()
	database := newDispatchTestDatabase(t)
	repositoryStore := repository.NewStore(database)
	triggerStore := trigger.NewStore(database)
	releaseStore := release.NewStore(database)
	queue := &queueStub{}
	poller := release.NewPoller(
		repositoryStore,
		triggerStore,
		release.NewDispatcher(releaseStore, queue),
		&tagSourceStub{tags: []internalgit.Tag{{Name: "v1.0.0", Commit: "111"}}},
	)

	repo, err := repositoryStore.Create(ctx, repository.CreateInput{
		Name:    "repair-repo",
		RepoURL: "https://example.com/org/repair.git",
	})
	if err != nil {
		t.Fatalf("create repository: %v", err)
	}

	rule, err := triggerStore.Create(ctx, trigger.CreateInput{
		RepositoryID: repo.ID,
		Name:         "repair-poll",
		Source:       trigger.SourcePoll,
	})
	if err != nil {
		t.Fatalf("create trigger rule: %v", err)
	}

	record, err := releaseStore.CreatePollDispatch(ctx, release.PollDispatchInput{
		RepositoryID:  repo.ID,
		TriggerRuleID: rule.ID,
		GitTag:        "v1.0.0",
		GitCommit:     "111",
		ObservedVia:   "seed",
	})
	if err != nil {
		t.Fatalf("seed poll dispatch: %v", err)
	}

	if _, err := releaseStore.MarkQueued(ctx, record.ID); err != nil {
		t.Fatalf("mark seeded release queued: %v", err)
	}

	result, err := poller.PollRule(ctx, rule.ID)
	if err != nil {
		t.Fatalf("poll rule: %v", err)
	}

	if result.Status != release.PollStatusAlreadySeen {
		t.Fatalf("expected already_seen poll status, got %q", result.Status)
	}

	loadedRepo, err := repositoryStore.Get(ctx, repo.ID)
	if err != nil {
		t.Fatalf("get repository: %v", err)
	}

	if loadedRepo.LastSeenTag == nil || *loadedRepo.LastSeenTag != "v1.0.0" {
		t.Fatalf("expected repaired last seen tag v1.0.0, got %#v", loadedRepo.LastSeenTag)
	}

	if len(queue.payloads) != 0 {
		t.Fatalf("expected no queued payloads for already seen tag, got %d", len(queue.payloads))
	}
}

func TestPollerReturnsBuildInProgressWithoutAdvancingBaseline(t *testing.T) {
	t.Parallel()

	ctx := context.Background()
	database := newDispatchTestDatabase(t)
	repositoryStore := repository.NewStore(database)
	triggerStore := trigger.NewStore(database)
	releaseStore := release.NewStore(database)
	buildStore := build.NewStore(database)
	queue := &queueStub{}
	poller := release.NewPoller(
		repositoryStore,
		triggerStore,
		release.NewDispatcher(releaseStore, queue),
		&tagSourceStub{tags: []internalgit.Tag{{Name: "v1.1.0", Commit: "222"}}},
	)

	repo, err := repositoryStore.Create(ctx, repository.CreateInput{
		Name:    "blocked-poll-repo",
		RepoURL: "https://example.com/org/blocked.git",
	})
	if err != nil {
		t.Fatalf("create repository: %v", err)
	}

	rule, err := triggerStore.Create(ctx, trigger.CreateInput{
		RepositoryID: repo.ID,
		Name:         "blocked-poll",
		Source:       trigger.SourcePoll,
	})
	if err != nil {
		t.Fatalf("create trigger rule: %v", err)
	}

	target, err := buildStore.CreateTarget(ctx, build.CreateTargetInput{
		RepositoryID:   repo.ID,
		Name:           "linux-player",
		Platform:       "linux",
		TimeoutSeconds: 3600,
	})
	if err != nil {
		t.Fatalf("create build target: %v", err)
	}

	firstRelease, err := releaseStore.CreateManualDispatch(ctx, release.ManualDispatchInput{
		RepositoryID: repo.ID,
		GitTag:       "v1.0.0",
	})
	if err != nil {
		t.Fatalf("create initial release: %v", err)
	}

	if _, err := releaseStore.MarkQueued(ctx, firstRelease.ID); err != nil {
		t.Fatalf("mark initial release queued: %v", err)
	}

	if _, err := database.ExecContext(
		ctx,
		`INSERT INTO build_runs (release_run_id, build_target_id, status) VALUES (?, ?, ?)`,
		firstRelease.ID,
		target.ID,
		build.StatusRunning,
	); err != nil {
		t.Fatalf("insert running build run: %v", err)
	}

	result, err := poller.PollRule(ctx, rule.ID)
	if err != nil {
		t.Fatalf("poll rule: %v", err)
	}

	if result.Status != release.PollStatusBuildInProgress {
		t.Fatalf("expected build_in_progress poll status, got %q", result.Status)
	}
	if result.Tag == nil || result.Tag.Name != "v1.1.0" {
		t.Fatalf("expected blocked tag v1.1.0, got %#v", result.Tag)
	}
	if len(result.Releases) != 0 {
		t.Fatalf("expected no queued releases, got %#v", result.Releases)
	}
	if len(queue.payloads) != 0 {
		t.Fatalf("expected no queued payloads, got %d", len(queue.payloads))
	}

	loadedRepo, err := repositoryStore.Get(ctx, repo.ID)
	if err != nil {
		t.Fatalf("get repository: %v", err)
	}
	if loadedRepo.LastSeenTag != nil {
		t.Fatalf("expected last seen tag to remain unset, got %#v", loadedRepo.LastSeenTag)
	}
}

type tagSourceStub struct {
	tags []internalgit.Tag
	err  error
}

func (s *tagSourceStub) ListTags(
	context.Context,
	string,
	internalgit.AuthOptions,
) ([]internalgit.Tag, error) {
	if s.err != nil {
		return nil, s.err
	}

	return append([]internalgit.Tag(nil), s.tags...), nil
}
