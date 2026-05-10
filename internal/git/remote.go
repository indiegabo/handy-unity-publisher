// Package git contains narrow Git integration used by polling and repository
// inspection workflows.
package git

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"os/exec"
	"strings"
)

var (
	// ErrInvalidRepositoryURL reports missing or malformed repository locations
	// passed to Git-backed discovery helpers.
	ErrInvalidRepositoryURL = errors.New("invalid git repository url")
)

// Tag is one discovered Git tag and the commit it currently resolves to.
type Tag struct {
	Name   string `json:"name"`
	Commit string `json:"commit"`
}

// TagSource lists repository tags in ascending version order so pollers can
// decide which release candidate should be processed next.
type TagSource interface {
	ListTags(ctx context.Context, repoURL string, auth AuthOptions) ([]Tag, error)
}

// RemoteTagSource lists remote tags through the local Git binary.
type RemoteTagSource struct {
	runner commandRunner
}

// commandRunner abstracts Git CLI execution for focused remote tag tests.
type commandRunner interface {
	Run(ctx context.Context, name string, args ...string) ([]byte, error)
}

// execRunner executes Git CLI commands through `os/exec`.
type execRunner struct{}

// Run executes one host command and returns combined stdout and stderr.
func (execRunner) Run(
	ctx context.Context,
	name string,
	args ...string,
) ([]byte, error) {
	command := exec.CommandContext(ctx, name, args...)
	return command.CombinedOutput()
}

// NewRemoteTagSource creates the default Git-backed tag discovery client.
func NewRemoteTagSource() *RemoteTagSource {
	return &RemoteTagSource{runner: execRunner{}}
}

// newRemoteTagSourceWithRunner injects a custom runner for focused tag source
// tests.
func newRemoteTagSourceWithRunner(runner commandRunner) *RemoteTagSource {
	return &RemoteTagSource{runner: runner}
}

// ListTags returns repository tags ordered by Git version sorting so callers
// can step forward from the durable last-seen tag state.
func (s *RemoteTagSource) ListTags(
	ctx context.Context,
	repoURL string,
	auth AuthOptions,
) ([]Tag, error) {
	repoURL = strings.TrimSpace(repoURL)
	if repoURL == "" {
		return nil, fmt.Errorf(
			"%w: repo_url must not be empty",
			ErrInvalidRepositoryURL,
		)
	}

	output, err := s.runner.Run(
		ctx,
		"git",
		auth.AppendGitArgs(
			"ls-remote",
			"--refs",
			"--tags",
			"--sort=version:refname",
			repoURL,
		)...,
	)
	if err != nil {
		return nil, fmt.Errorf("list remote tags: %w", err)
	}

	return parseTags(output)
}

// parseTags decodes `git ls-remote --tags` output into the normalized tag
// records consumed by pollers.
func parseTags(output []byte) ([]Tag, error) {
	trimmed := bytes.TrimSpace(output)
	if len(trimmed) == 0 {
		return []Tag{}, nil
	}

	lines := strings.Split(string(trimmed), "\n")
	tags := make([]Tag, 0, len(lines))
	for _, line := range lines {
		fields := strings.Fields(line)
		if len(fields) != 2 {
			return nil, fmt.Errorf("parse git ls-remote line %q", line)
		}

		ref := fields[1]
		if !strings.HasPrefix(ref, "refs/tags/") {
			return nil, fmt.Errorf("parse git tag ref %q", ref)
		}

		name := strings.TrimPrefix(ref, "refs/tags/")
		if name == "" {
			return nil, fmt.Errorf("parse git tag ref %q", ref)
		}

		tags = append(tags, Tag{Name: name, Commit: fields[0]})
	}

	return tags, nil
}