package build

import (
	"context"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"

	internalgit "github.com/indiegabo/handy-unity-bulder/internal/git"
)

// projectVersionFilePath is the Unity metadata file that declares the editor
// version required for one repository snapshot.
const projectVersionFilePath = "ProjectSettings/ProjectVersion.txt"

// unityVersionDetector resolves the Unity editor version required by one
// repository tag.
type unityVersionDetector interface {
	Detect(
		ctx context.Context,
		repositoryURL string,
		gitTag string,
		auth internalgit.AuthOptions,
	) (string, error)
}

// gitProjectVersionDetector reads Unity version metadata from a temporary
// Git-backed workspace.
type gitProjectVersionDetector struct {
	runner unityVersionCommandRunner
}

// unityVersionCommandRunner executes Git commands used to materialize the
// smallest possible workspace slice needed for Unity version detection.
type unityVersionCommandRunner interface {
	Run(ctx context.Context, workingDir string, name string, args ...string) ([]byte, error)
}

// execUnityVersionRunner executes one host command through `os/exec`.
type execUnityVersionRunner struct{}

// Run executes one host command inside the optional working directory and
// returns combined stdout and stderr.
func (execUnityVersionRunner) Run(
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

// newGitProjectVersionDetector creates the Git-backed Unity version detector
// used during release planning.
func newGitProjectVersionDetector() unityVersionDetector {
	return newGitProjectVersionDetectorWithRunner(execUnityVersionRunner{})
}

// newGitProjectVersionDetectorWithRunner injects a Git command runner for
// focused Unity version detection tests.
func newGitProjectVersionDetectorWithRunner(
	runner unityVersionCommandRunner,
) unityVersionDetector {
	return &gitProjectVersionDetector{runner: runner}
}

// Detect materializes the requested repository tag and extracts the Unity
// editor version declared in `ProjectSettings/ProjectVersion.txt`.
func (d *gitProjectVersionDetector) Detect(
	ctx context.Context,
	repositoryURL string,
	gitTag string,
	auth internalgit.AuthOptions,
) (string, error) {
	repositoryURL = strings.TrimSpace(repositoryURL)
	gitTag = strings.TrimSpace(gitTag)

	if repositoryURL == "" {
		return "", fmt.Errorf(
			"%w: repository url must not be empty",
			ErrUnityVersionUnavailable,
		)
	}

	if gitTag == "" {
		return "", fmt.Errorf(
			"%w: git tag must not be empty",
			ErrUnityVersionUnavailable,
		)
	}

	workspacePath, err := os.MkdirTemp("", "hgb-unity-version-")
	if err != nil {
		return "", fmt.Errorf(
			"%w: create temporary unity version workspace: %v",
			ErrUnityVersionUnavailable,
			err,
		)
	}
	defer os.RemoveAll(workspacePath)

	if err := d.prepareWorkspace(
		ctx,
		repositoryURL,
		workspacePath,
		gitTag,
		auth,
	); err != nil {
		return "", fmt.Errorf(
			"%w: materialize repository tag %q: %v",
			ErrUnityVersionUnavailable,
			gitTag,
			err,
		)
	}

	contents, err := os.ReadFile(filepath.Join(workspacePath, projectVersionFilePath))
	if err != nil {
		return "", fmt.Errorf(
			"%w: read %s from prepared workspace: %v",
			ErrUnityVersionUnavailable,
			projectVersionFilePath,
			err,
		)
	}

	return parseUnityVersion(contents)
}

// prepareWorkspace performs a partial sparse clone that only checks out the
// Unity version metadata file needed for release planning.
func (d *gitProjectVersionDetector) prepareWorkspace(
	ctx context.Context,
	repositoryURL string,
	workspacePath string,
	gitTag string,
	auth internalgit.AuthOptions,
) error {
	if _, err := d.runner.Run(
		ctx,
		"",
		"git",
		auth.AppendGitArgs(
			"clone",
			"--filter=blob:none",
			"--sparse",
			"--depth=1",
			"--single-branch",
			"--branch",
			gitTag,
			"--no-checkout",
			repositoryURL,
			workspacePath,
		)...,
	); err != nil {
		return fmt.Errorf("clone repository into workspace: %w", err)
	}

	if _, err := d.runner.Run(
		ctx,
		workspacePath,
		"git",
		"sparse-checkout",
		"set",
		"--no-cone",
		projectVersionFilePath,
	); err != nil {
		return fmt.Errorf("configure sparse checkout for %s: %w", projectVersionFilePath, err)
	}

	if _, err := d.runner.Run(
		ctx,
		workspacePath,
		"git",
		auth.AppendGitArgs(
			"checkout",
			"--detach",
			"--force",
		)...,
	); err != nil {
		return fmt.Errorf("checkout repository tag %q: %w", gitTag, err)
	}

	return nil
}

// parseUnityVersion extracts the `m_EditorVersion` value from the Unity
// project version file contents.
func parseUnityVersion(contents []byte) (string, error) {
	for _, line := range strings.Split(string(contents), "\n") {
		trimmed := strings.TrimSpace(line)
		if !strings.HasPrefix(trimmed, "m_EditorVersion:") {
			continue
		}

		version := strings.TrimSpace(
			strings.TrimPrefix(trimmed, "m_EditorVersion:"),
		)
		if version == "" {
			break
		}

		return version, nil
	}

	return "", fmt.Errorf(
		"%w: %s does not define m_EditorVersion",
		ErrUnityVersionUnavailable,
		projectVersionFilePath,
	)
}
