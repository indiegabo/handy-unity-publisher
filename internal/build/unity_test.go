package build

import (
	"context"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"

	internalgit "github.com/indiegabo/handy-unity-bulder/internal/git"
)

func TestParseUnityVersionExtractsEditorVersion(t *testing.T) {
	t.Parallel()

	version, err := parseUnityVersion([]byte(
		"m_EditorVersion: 2022.3.14f1\n" +
			"m_EditorVersionWithRevision: 2022.3.14f1 (abc123)\n",
	))
	if err != nil {
		t.Fatalf("parse unity version: %v", err)
	}

	if version != "2022.3.14f1" {
		t.Fatalf("expected unity version 2022.3.14f1, got %q", version)
	}
}

func TestGitProjectVersionDetectorDetectsTaggedUnityVersion(t *testing.T) {
	t.Parallel()

	repositoryPath := newUnityTaggedRepository(
		t,
		"2021.3.18f1",
		"v1.0.0",
	)
	detector := newGitProjectVersionDetector()

	version, err := detector.Detect(
		context.Background(),
		repositoryPath,
		"v1.0.0",
		internalgit.AuthOptions{},
	)
	if err != nil {
		t.Fatalf("detect unity version: %v", err)
	}

	if version != "2021.3.18f1" {
		t.Fatalf("expected unity version 2021.3.18f1, got %q", version)
	}
}

func TestGitProjectVersionDetectorDetectsTaggedUnityVersionFromFileURL(t *testing.T) {
	t.Parallel()

	repositoryPath := newUnityTaggedRepository(
		t,
		"2022.3.14f1",
		"v1.1.0",
	)

	detector := newGitProjectVersionDetector()

	version, err := detector.Detect(
		context.Background(),
		"file://"+repositoryPath,
		"v1.1.0",
		internalgit.AuthOptions{},
	)
	if err != nil {
		t.Fatalf("detect unity version from file url: %v", err)
	}

	if version != "2022.3.14f1" {
		t.Fatalf("expected unity version 2022.3.14f1, got %q", version)
	}
}

func TestGitProjectVersionDetectorUsesPartialSparseClone(t *testing.T) {
	t.Parallel()

	runner := &unityVersionRunnerStub{}
	detector := newGitProjectVersionDetectorWithRunner(runner)

	version, err := detector.Detect(
		context.Background(),
		"https://example.com/org/revolutions.git",
		"v1.2.3",
		internalgit.AuthOptions{ExtraHeaders: []string{"Authorization: Bearer secret"}},
	)
	if err != nil {
		t.Fatalf("detect unity version: %v", err)
	}

	if version != "2022.3.14f1" {
		t.Fatalf("expected unity version 2022.3.14f1, got %q", version)
	}

	if len(runner.calls) != 3 {
		t.Fatalf("expected clone, sparse-checkout, and checkout calls, got %d", len(runner.calls))
	}

	cloneArgs := strings.Join(runner.calls[0].args, " ")
	for _, want := range []string{
		"-c http.extraHeader=Authorization: Bearer secret",
		"clone",
		"--filter=blob:none",
		"--sparse",
		"--depth=1",
		"--single-branch",
		"--branch v1.2.3",
		"--no-checkout",
	} {
		if !strings.Contains(cloneArgs, want) {
			t.Fatalf("expected clone args to contain %q, got %q", want, cloneArgs)
		}
	}

	sparseArgs := strings.Join(runner.calls[1].args, " ")
	if !strings.Contains(sparseArgs, "sparse-checkout set --no-cone "+projectVersionFilePath) {
		t.Fatalf("expected sparse checkout call for %s, got %q", projectVersionFilePath, sparseArgs)
	}

	checkoutArgs := strings.Join(runner.calls[2].args, " ")
	if !strings.Contains(checkoutArgs, "-c http.extraHeader=Authorization: Bearer secret") {
		t.Fatalf("expected checkout auth prefix, got %q", checkoutArgs)
	}
	if !strings.Contains(checkoutArgs, "checkout --detach --force") {
		t.Fatalf("expected checkout call, got %q", checkoutArgs)
	}
}

func newUnityTaggedRepository(
	t *testing.T,
	unityVersion string,
	gitTag string,
) string {
	t.Helper()

	repositoryPath := t.TempDir()
	runGit(t, repositoryPath, "init")
	runGit(t, repositoryPath, "config", "user.name", "Build Tests")
	runGit(t, repositoryPath, "config", "user.email", "build-tests@example.com")

	projectSettingsDir := filepath.Join(repositoryPath, "ProjectSettings")
	if err := os.MkdirAll(projectSettingsDir, 0o755); err != nil {
		t.Fatalf("create ProjectSettings directory: %v", err)
	}

	projectVersionPath := filepath.Join(projectSettingsDir, "ProjectVersion.txt")
	if err := os.WriteFile(
		projectVersionPath,
		[]byte(
			"m_EditorVersion: "+unityVersion+"\n"+
				"m_EditorVersionWithRevision: "+unityVersion+" (revision)\n",
		),
		0o644,
	); err != nil {
		t.Fatalf("write ProjectVersion.txt: %v", err)
	}

	runGit(t, repositoryPath, "add", ".")
	runGit(t, repositoryPath, "commit", "-m", "add unity project version")
	runGit(t, repositoryPath, "tag", gitTag)

	return repositoryPath
}

func runGit(t *testing.T, repositoryPath string, args ...string) string {
	t.Helper()

	command := exec.Command("git", args...)
	command.Dir = repositoryPath
	output, err := command.CombinedOutput()
	if err != nil {
		t.Fatalf("run git %v: %v\n%s", args, err, string(output))
	}

	return strings.TrimSpace(string(output))
}

type unityVersionRunnerStub struct {
	calls         []unityVersionRunnerCall
	workspacePath string
}

type unityVersionRunnerCall struct {
	workingDir string
	name       string
	args       []string
}

func (s *unityVersionRunnerStub) Run(
	_ context.Context,
	workingDir string,
	name string,
	args ...string,
) ([]byte, error) {
	s.calls = append(s.calls, unityVersionRunnerCall{
		workingDir: workingDir,
		name:       name,
		args:       append([]string(nil), args...),
	})

	joined := strings.Join(args, " ")
	if strings.Contains(joined, " clone ") || (len(args) > 0 && args[0] == "clone") {
		s.workspacePath = args[len(args)-1]
		if err := os.MkdirAll(filepath.Join(s.workspacePath, ".git"), 0o755); err != nil {
			return nil, err
		}
		return nil, nil
	}

	if strings.Contains(joined, "checkout --detach --force") {
		projectVersionPath := filepath.Join(workingDir, projectVersionFilePath)
		if err := os.MkdirAll(filepath.Dir(projectVersionPath), 0o755); err != nil {
			return nil, err
		}
		if err := os.WriteFile(
			projectVersionPath,
			[]byte("m_EditorVersion: 2022.3.14f1\n"),
			0o644,
		); err != nil {
			return nil, err
		}
	}

	return nil, nil
}
