package release

import (
	"context"
	"errors"
	"fmt"
	"strings"

	"github.com/indiegabo/handy-unity-bulder/internal/credentials"
	internalgit "github.com/indiegabo/handy-unity-bulder/internal/git"
	"github.com/indiegabo/handy-unity-bulder/internal/repository"
	"github.com/indiegabo/handy-unity-bulder/internal/trigger"
)

const (
	// PollStatusNoTags reports that the repository currently exposes no tags.
	PollStatusNoTags = "no_tags"
	// PollStatusBuildInProgress reports that polling found a new tag but the
	// repository is still executing queued or running build work.
	PollStatusBuildInProgress = "build_in_progress"
	// PollStatusUnchanged reports that polling found no tag newer than the
	// durable repository baseline.
	PollStatusUnchanged = "unchanged"
	// PollStatusQueued reports that polling created and queued one or more new
	// release runs.
	PollStatusQueued = "queued"
	// PollStatusAlreadySeen reports that polling found a tag whose release run
	// already existed and repaired the repository baseline if needed.
	PollStatusAlreadySeen = "already_seen"
)

// PollResult summarizes one manual polling execution for a trigger rule.
type PollResult struct {
	TriggerRuleID int64             `json:"trigger_rule_id"`
	RepositoryID  int64             `json:"repository_id"`
	Status        string            `json:"status"`
	Tag           *internalgit.Tag  `json:"tag,omitempty"`
	Release       *Record           `json:"release,omitempty"`
	Tags          []internalgit.Tag `json:"tags,omitempty"`
	Releases      []Record          `json:"releases,omitempty"`
}

// SelectQueuedRepositoryTags exposes the repository polling selection logic to
// runtime automation that polls repositories directly.
func SelectQueuedRepositoryTags(
	tags []internalgit.Tag,
	lastSeenTag *string,
) ([]internalgit.Tag, string, bool) {
	return selectQueuedPollTags(tags, lastSeenTag)
}

// SelectNextRepositoryTag exposes the next actionable repository tag for
// callers that still operate on one discovered tag at a time.
func SelectNextRepositoryTag(
	tags []internalgit.Tag,
	lastSeenTag *string,
) (internalgit.Tag, string, bool) {
	selected, status, ok := selectQueuedPollTags(tags, lastSeenTag)
	if !ok {
		return internalgit.Tag{}, status, false
	}

	return selected[0], "", true
}

// Poller executes one poll-trigger evaluation through the shared release
// dispatcher.
type Poller struct {
	repositories repository.Store
	triggers     trigger.Store
	dispatcher   *Dispatcher
	tags         internalgit.TagSource
	credentials  credentials.Store
}

// NewPoller creates a poller over durable repository and trigger state,
// a shared release dispatcher, and a Git-backed tag source.
func NewPoller(
	repositories repository.Store,
	triggers trigger.Store,
	dispatcher *Dispatcher,
	tags internalgit.TagSource,
) *Poller {
	return &Poller{
		repositories: repositories,
		triggers:     triggers,
		dispatcher:   dispatcher,
		tags:         tags,
	}
}

// WithCredentials configures the credentials store used to authenticate Git
// polling for private repositories.
func (p *Poller) WithCredentials(store credentials.Store) *Poller {
	p.credentials = store
	return p
}

// PollRule runs one poll trigger rule, creates new release runs in ascending
// tag order, and advances the repository last-seen tag whenever one queued or
// already-seen tag is accepted into the durable baseline.
func (p *Poller) PollRule(
	ctx context.Context,
	triggerRuleID int64,
) (PollResult, error) {
	if triggerRuleID <= 0 {
		return PollResult{}, fmt.Errorf(
			"%w: trigger_rule_id must be greater than zero",
			ErrInvalid,
		)
	}

	rule, err := p.triggers.Get(ctx, triggerRuleID)
	if err != nil {
		return PollResult{}, err
	}

	if rule.Source != trigger.SourcePoll {
		return PollResult{}, fmt.Errorf(
			"%w: trigger rule %d is not a poll rule",
			ErrInvalid,
			rule.ID,
		)
	}

	if !rule.Enabled {
		return PollResult{}, fmt.Errorf(
			"%w: trigger rule %d is disabled",
			ErrInvalid,
			rule.ID,
		)
	}

	repo, err := p.repositories.Get(ctx, rule.RepositoryID)
	if err != nil {
		return PollResult{}, err
	}

	if !repo.Enabled {
		return PollResult{}, fmt.Errorf(
			"%w: repository %d is disabled",
			ErrInvalid,
			repo.ID,
		)
	}

	auth, err := p.resolveRepositoryGitAuth(ctx, repo.CredentialsID)
	if err != nil {
		return PollResult{}, err
	}

	tags, err := p.tags.ListTags(ctx, repo.RepoURL, auth)
	if err != nil {
		return PollResult{}, fmt.Errorf("discover repository tags: %w", err)
	}

	result := PollResult{
		TriggerRuleID: rule.ID,
		RepositoryID:  repo.ID,
	}

	selectedTags, status, ok := selectQueuedPollTags(tags, repo.LastSeenTag)
	if !ok {
		result.Status = status
		return result, nil
	}

	result.Tags = make([]internalgit.Tag, 0, len(selectedTags))
	result.Releases = make([]Record, 0, len(selectedTags))
	advancedBaseline := false

	for _, tag := range selectedTags {
		record, err := p.dispatcher.DispatchPoll(ctx, PollDispatchInput{
			RepositoryID:  repo.ID,
			TriggerRuleID: rule.ID,
			GitTag:        tag.Name,
			GitCommit:     tag.Commit,
			ObservedVia:   "poller",
		})
		if err != nil {
			if errors.Is(err, ErrBuildInProgress) {
				result.Tag = &tag
				result.Status = PollStatusBuildInProgress
				return result, nil
			}

			if errors.Is(err, ErrConflict) {
				if _, updateErr := p.repositories.UpdateLastSeenTag(
					ctx,
					repo.ID,
					tag.Name,
				); updateErr != nil {
					return PollResult{}, fmt.Errorf(
						"update repository last seen tag after duplicate release: %w",
						updateErr,
					)
				}

				result.Tags = append(result.Tags, tag)
				advancedBaseline = true
				continue
			}

			return PollResult{}, err
		}

		if _, err := p.repositories.UpdateLastSeenTag(ctx, repo.ID, tag.Name); err != nil {
			return PollResult{}, fmt.Errorf("update repository last seen tag: %w", err)
		}

		result.Tags = append(result.Tags, tag)
		result.Releases = append(result.Releases, record)
		advancedBaseline = true
	}

	if len(result.Tags) > 0 {
		latestTag := result.Tags[len(result.Tags)-1]
		result.Tag = &latestTag
	}
	if len(result.Releases) > 0 {
		latestRelease := result.Releases[len(result.Releases)-1]
		result.Release = &latestRelease
		result.Status = PollStatusQueued
		return result, nil
	}
	if advancedBaseline {
		result.Status = PollStatusAlreadySeen
		return result, nil
	}

	result.Status = PollStatusUnchanged
	return result, nil
}

// resolveRepositoryGitAuth loads optional repository credentials and converts
// them into Git authentication flags for polling.
func (p *Poller) resolveRepositoryGitAuth(
	ctx context.Context,
	credentialsID *int64,
) (internalgit.AuthOptions, error) {
	if credentialsID == nil {
		return internalgit.AuthOptions{}, nil
	}
	if p.credentials == nil {
		return internalgit.AuthOptions{}, fmt.Errorf(
			"%w: poller credentials store is required",
			ErrInvalid,
		)
	}

	record, err := p.credentials.Get(ctx, *credentialsID)
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

// selectQueuedPollTags returns the actionable tag sequence after the durable
// repository baseline, preserving ascending version order.
func selectQueuedPollTags(
	tags []internalgit.Tag,
	lastSeenTag *string,
) ([]internalgit.Tag, string, bool) {
	if len(tags) == 0 {
		return nil, PollStatusNoTags, false
	}

	normalizedLastSeen := ""
	if lastSeenTag != nil {
		normalizedLastSeen = strings.TrimSpace(*lastSeenTag)
	}

	if normalizedLastSeen == "" {
		return append([]internalgit.Tag(nil), tags...), "", true
	}

	for index, tag := range tags {
		if tag.Name != normalizedLastSeen {
			continue
		}

		if index == len(tags)-1 {
			return nil, PollStatusUnchanged, false
		}

		return append([]internalgit.Tag(nil), tags[index+1:]...), "", true
	}

	if tags[len(tags)-1].Name == normalizedLastSeen {
		return nil, PollStatusUnchanged, false
	}

	return []internalgit.Tag{tags[len(tags)-1]}, "", true
}
