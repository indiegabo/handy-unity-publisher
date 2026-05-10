// Package git contains narrow Git integration used by polling, repository
// inspection, and workspace preparation workflows.
package git

import (
	"context"
	"errors"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
)

var (
	// ErrInvalidWorkspacePath reports missing or malformed workspace locations
	// passed to Git-backed workspace preparation helpers.
	ErrInvalidWorkspacePath = errors.New("invalid git workspace path")
)

// WorkspaceSyncer materializes one repository tag into a deterministic local
// workspace directory.
type WorkspaceSyncer interface {
	SyncTag(ctx context.Context, repoURL string, workspacePath string, gitTag string, auth AuthOptions) error
}

// RepositoryWorkspaceSyncer prepares local workspaces through the Git CLI.
type RepositoryWorkspaceSyncer struct {
	runner workspaceCommandRunner
}

// workspaceCommandRunner abstracts Git CLI execution for focused workspace
// synchronization tests.
type workspaceCommandRunner interface {
	Run(ctx context.Context, workingDir string, name string, args ...string) ([]byte, error)
}

// execWorkspaceRunner executes Git CLI commands through `os/exec`.
type execWorkspaceRunner struct{}

// Run executes one host command inside the optional working directory and
// returns combined stdout and stderr.
func (execWorkspaceRunner) Run(
	ctx context.Context,
	workingDir string,
	name string,
	args ...string,
) ([]byte, error) {
	command := exec.CommandContext(ctx, name, args...)
	if strings.TrimSpace(workingDir) != "" {
		command.Dir = workingDir
	}

	return command.CombinedOutput()
}

// NewWorkspaceSyncer creates the default Git-backed workspace preparer.
func NewWorkspaceSyncer() *RepositoryWorkspaceSyncer {
	return &RepositoryWorkspaceSyncer{runner: execWorkspaceRunner{}}
}

// newWorkspaceSyncerWithRunner injects a custom runner for focused workspace
// synchronization tests.
func newWorkspaceSyncerWithRunner(
	runner workspaceCommandRunner,
) *RepositoryWorkspaceSyncer {
	return &RepositoryWorkspaceSyncer{runner: runner}
}

// SyncTag clones or refreshes one local workspace and checks out the requested
// tag in detached HEAD state.
func (s *RepositoryWorkspaceSyncer) SyncTag(
	ctx context.Context,
	repoURL string,
	workspacePath string,
	gitTag string,
	auth AuthOptions,
) error {
	repoURL = strings.TrimSpace(repoURL)
	workspacePath = filepath.Clean(strings.TrimSpace(workspacePath))
	gitTag = strings.TrimSpace(gitTag)

	if repoURL == "" {
		return fmt.Errorf(
			"%w: repo_url must not be empty",
			ErrInvalidRepositoryURL,
		)
	}
	if workspacePath == "" || workspacePath == "." {
		return fmt.Errorf(
			"%w: workspace_path must not be empty",
			ErrInvalidWorkspacePath,
		)
	}
	if gitTag == "" {
		return fmt.Errorf("%w: git tag must not be empty", ErrInvalidRepositoryURL)
	}

	if err := os.MkdirAll(filepath.Dir(workspacePath), 0o755); err != nil {
		return fmt.Errorf("prepare workspace parent: %w", err)
	}

	gitDir := filepath.Join(workspacePath, ".git")
	if _, err := os.Stat(gitDir); err != nil {
		if !errors.Is(err, os.ErrNotExist) {
			return fmt.Errorf("inspect workspace git dir: %w", err)
		}

		if err := os.RemoveAll(workspacePath); err != nil {
			return fmt.Errorf("reset workspace %q: %w", workspacePath, err)
		}

		if _, err := s.runner.Run(
			ctx,
			"",
			"git",
			auth.AppendGitArgs(
				"clone",
				"--depth=1",
				"--single-branch",
				"--branch",
				gitTag,
				"--no-checkout",
				repoURL,
				workspacePath,
			)...,
		); err != nil {
			return fmt.Errorf("clone repository into workspace: %w", err)
		}
	} else {
		if _, err := s.runner.Run(
			ctx,
			workspacePath,
			"git",
			"remote",
			"set-url",
			"origin",
			repoURL,
		); err != nil {
			return fmt.Errorf("set workspace remote origin: %w", err)
		}
	}

	if _, err := s.runner.Run(
		ctx,
		workspacePath,
		"git",
		auth.AppendGitArgs(
			"fetch",
			"--depth=1",
			"origin",
			fmt.Sprintf("refs/tags/%s:refs/tags/%s", gitTag, gitTag),
		)...,
	); err != nil {
		return fmt.Errorf("fetch repository tag %q: %w", gitTag, err)
	}

	if _, err := s.runner.Run(
		ctx,
		workspacePath,
		"git",
		"checkout",
		"--detach",
		"--force",
		fmt.Sprintf("refs/tags/%s", gitTag),
	); err != nil {
		return fmt.Errorf("checkout repository tag %q: %w", gitTag, err)
	}

	if _, err := s.runner.Run(ctx, workspacePath, "git", "clean", "-fdx"); err != nil {
		return fmt.Errorf("clean workspace after checkout: %w", err)
	}

	return nil
}