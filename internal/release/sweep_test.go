package release_test

import (
	"context"
	"errors"
	"testing"

	internalgit "github.com/indiegabo/handy-unity-bulder/internal/git"
	"github.com/indiegabo/handy-unity-bulder/internal/release"
	"github.com/indiegabo/handy-unity-bulder/internal/repository"
	"github.com/indiegabo/handy-unity-bulder/internal/trigger"
)

func TestPollSweepRunOnceEvaluatesEnabledRules(t *testing.T) {
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
		&tagSourceByRepoStub{tagsByRepo: map[string][]internalgit.Tag{
			"https://example.com/org/repo-a.git": {
				{Name: "v1.0.0", Commit: "111"},
			},
			"https://example.com/org/repo-b.git": {
				{Name: "v2.0.0", Commit: "222"},
			},
		}},
	)
	sweep := release.NewPollSweep(triggerStore, poller)

	repoA, err := repositoryStore.Create(ctx, repository.CreateInput{
		Name:    "repo-a",
		RepoURL: "https://example.com/org/repo-a.git",
	})
	if err != nil {
		t.Fatalf("create repository A: %v", err)
	}

	repoB, err := repositoryStore.Create(ctx, repository.CreateInput{
		Name:    "repo-b",
		RepoURL: "https://example.com/org/repo-b.git",
	})
	if err != nil {
		t.Fatalf("create repository B: %v", err)
	}

	enabled := false
	if _, err := triggerStore.Create(ctx, trigger.CreateInput{
		RepositoryID: repoA.ID,
		Name:         "ignored-manual",
		Source:       trigger.SourceManual,
	}); err != nil {
		t.Fatalf("create manual trigger: %v", err)
	}

	if _, err := triggerStore.Create(ctx, trigger.CreateInput{
		RepositoryID: repoA.ID,
		Name:         "ignored-disabled-poll",
		Source:       trigger.SourcePoll,
		Enabled:      &enabled,
	}); err != nil {
		t.Fatalf("create disabled poll trigger: %v", err)
	}

	if _, err := triggerStore.Create(ctx, trigger.CreateInput{
		RepositoryID: repoA.ID,
		Name:         "poll-a",
		Source:       trigger.SourcePoll,
	}); err != nil {
		t.Fatalf("create poll trigger A: %v", err)
	}

	if _, err := triggerStore.Create(ctx, trigger.CreateInput{
		RepositoryID: repoB.ID,
		Name:         "poll-b",
		Source:       trigger.SourcePoll,
	}); err != nil {
		t.Fatalf("create poll trigger B: %v", err)
	}

	report, err := sweep.RunOnce(ctx)
	if err != nil {
		t.Fatalf("run poll sweep: %v", err)
	}

	if report.Evaluated != 2 {
		t.Fatalf("expected two evaluated rules, got %d", report.Evaluated)
	}

	if len(report.Results) != 2 {
		t.Fatalf("expected two successful results, got %d", len(report.Results))
	}

	if report.HasFailures() {
		t.Fatalf("expected no failures, got %#v", report.Failures)
	}

	if len(queue.payloads) != 2 {
		t.Fatalf("expected two queued payloads, got %d", len(queue.payloads))
	}
}

func TestPollSweepRunOnceContinuesAfterRuleFailure(t *testing.T) {
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
		&tagSourceByRepoStub{
			tagsByRepo: map[string][]internalgit.Tag{
				"https://example.com/org/good.git": {
					{Name: "v1.0.0", Commit: "111"},
				},
			},
			errsByRepo: map[string]error{
				"https://example.com/org/bad.git": errors.New("git unavailable"),
			},
		},
	)
	sweep := release.NewPollSweep(triggerStore, poller)

	goodRepo, err := repositoryStore.Create(ctx, repository.CreateInput{
		Name:    "good-repo",
		RepoURL: "https://example.com/org/good.git",
	})
	if err != nil {
		t.Fatalf("create good repository: %v", err)
	}

	badRepo, err := repositoryStore.Create(ctx, repository.CreateInput{
		Name:    "bad-repo",
		RepoURL: "https://example.com/org/bad.git",
	})
	if err != nil {
		t.Fatalf("create bad repository: %v", err)
	}

	if _, err := triggerStore.Create(ctx, trigger.CreateInput{
		RepositoryID: badRepo.ID,
		Name:         "poll-bad",
		Source:       trigger.SourcePoll,
	}); err != nil {
		t.Fatalf("create bad poll trigger: %v", err)
	}

	if _, err := triggerStore.Create(ctx, trigger.CreateInput{
		RepositoryID: goodRepo.ID,
		Name:         "poll-good",
		Source:       trigger.SourcePoll,
	}); err != nil {
		t.Fatalf("create good poll trigger: %v", err)
	}

	report, err := sweep.RunOnce(ctx)
	if err != nil {
		t.Fatalf("run poll sweep: %v", err)
	}

	if report.Evaluated != 2 {
		t.Fatalf("expected two evaluated rules, got %d", report.Evaluated)
	}

	if len(report.Failures) != 1 {
		t.Fatalf("expected one failure, got %d", len(report.Failures))
	}

	if len(report.Results) != 1 {
		t.Fatalf("expected one successful result, got %d", len(report.Results))
	}

	if len(queue.payloads) != 1 {
		t.Fatalf("expected one queued payload, got %d", len(queue.payloads))
	}
}

type tagSourceByRepoStub struct {
	tagsByRepo map[string][]internalgit.Tag
	errsByRepo map[string]error
}

func (s *tagSourceByRepoStub) ListTags(
	_ context.Context,
	repoURL string,
	_ internalgit.AuthOptions,
) ([]internalgit.Tag, error) {
	if err, ok := s.errsByRepo[repoURL]; ok {
		return nil, err
	}

	tags := s.tagsByRepo[repoURL]
	return append([]internalgit.Tag(nil), tags...), nil
}