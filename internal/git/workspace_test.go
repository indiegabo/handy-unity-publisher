package git

import (
	"context"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
)

func TestWorkspaceSyncerClonesAndChecksOutRequestedTag(t *testing.T) {
	t.Parallel()

	repositoryPath := newTaggedRepository(t, map[string]string{
		"v1.0.0": "first",
		"v1.1.0": "second",
	})
	workspacePath := filepath.Join(t.TempDir(), "workspace")
	syncer := NewWorkspaceSyncer()

	if err := syncer.SyncTag(
		context.Background(),
		"file://"+repositoryPath,
		workspacePath,
		"v1.0.0",
		AuthOptions{},
	); err != nil {
		t.Fatalf("sync repository tag: %v", err)
	}

	contents, err := os.ReadFile(filepath.Join(workspacePath, "version.txt"))
	if err != nil {
		t.Fatalf("read synced file: %v", err)
	}

	if strings.TrimSpace(string(contents)) != "first" {
		t.Fatalf("expected version file contents first, got %q", string(contents))
	}
}

func TestWorkspaceSyncerRefreshesExistingWorkspaceToNewTag(t *testing.T) {
	t.Parallel()

	repositoryPath := newTaggedRepository(t, map[string]string{
		"v2.0.0": "stable",
		"v2.1.0": "patched",
	})
	workspacePath := filepath.Join(t.TempDir(), "workspace")
	syncer := NewWorkspaceSyncer()

	if err := syncer.SyncTag(
		context.Background(),
		"file://"+repositoryPath,
		workspacePath,
		"v2.0.0",
		AuthOptions{},
	); err != nil {
		t.Fatalf("sync first repository tag: %v", err)
	}

	if err := os.WriteFile(filepath.Join(workspacePath, "generated.tmp"), []byte("junk"), 0o644); err != nil {
		t.Fatalf("write generated file: %v", err)
	}

	if err := syncer.SyncTag(
		context.Background(),
		"file://"+repositoryPath,
		workspacePath,
		"v2.1.0",
		AuthOptions{},
	); err != nil {
		t.Fatalf("sync second repository tag: %v", err)
	}

	contents, err := os.ReadFile(filepath.Join(workspacePath, "version.txt"))
	if err != nil {
		t.Fatalf("read synced file: %v", err)
	}

	if strings.TrimSpace(string(contents)) != "patched" {
		t.Fatalf("expected version file contents patched, got %q", string(contents))
	}

	if _, err := os.Stat(filepath.Join(workspacePath, "generated.tmp")); !os.IsNotExist(err) {
		t.Fatalf("expected generated file to be cleaned, got err=%v", err)
	}
}

func newTaggedRepository(t *testing.T, tags map[string]string) string {
	t.Helper()

	repositoryPath := t.TempDir()
	runGit(t, repositoryPath, "init")
	runGit(t, repositoryPath, "config", "user.name", "Git Workspace Tests")
	runGit(t, repositoryPath, "config", "user.email", "git-workspace-tests@example.com")

	orderedTags := []string{"v1.0.0", "v1.1.0", "v2.0.0", "v2.1.0"}
	for _, gitTag := range orderedTags {
		contents, ok := tags[gitTag]
		if !ok {
			continue
		}

		if err := os.WriteFile(
			filepath.Join(repositoryPath, "version.txt"),
			[]byte(contents+"\n"),
			0o644,
		); err != nil {
			t.Fatalf("write version file: %v", err)
		}

		runGit(t, repositoryPath, "add", "version.txt")
		runGit(t, repositoryPath, "commit", "-m", "set "+gitTag)
		runGit(t, repositoryPath, "tag", gitTag)
	}

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

func TestWorkspaceSyncerPrefixesGitAuthConfiguration(t *testing.T) {
	t.Parallel()

	runner := &workspaceRunnerStub{}
	syncer := newWorkspaceSyncerWithRunner(runner)
	workspacePath := filepath.Join(t.TempDir(), "workspace")

	if err := syncer.SyncTag(
		context.Background(),
		"https://example.com/org/repo.git",
		workspacePath,
		"v1.0.0",
		AuthOptions{ExtraHeaders: []string{"Authorization: Bearer secret"}},
	); err != nil {
		t.Fatalf("sync repository tag: %v", err)
	}

	if len(runner.calls) < 4 {
		t.Fatalf("expected clone, fetch, checkout, and clean calls, got %d", len(runner.calls))
	}

	cloneArgs := runner.calls[0].args
	if len(cloneArgs) < 2 || cloneArgs[0] != "-c" || cloneArgs[1] != "http.extraHeader=Authorization: Bearer secret" {
		t.Fatalf("expected clone auth prefix, got %#v", cloneArgs)
	}

	fetchArgs := runner.calls[1].args
	if len(fetchArgs) < 2 || fetchArgs[0] != "-c" || fetchArgs[1] != "http.extraHeader=Authorization: Bearer secret" {
		t.Fatalf("expected fetch auth prefix, got %#v", fetchArgs)
	}
}

func TestWorkspaceSyncerUsesShallowCloneForInitialTagCheckout(t *testing.T) {
	t.Parallel()

	runner := &workspaceRunnerStub{}
	syncer := newWorkspaceSyncerWithRunner(runner)
	workspacePath := filepath.Join(t.TempDir(), "workspace")

	if err := syncer.SyncTag(
		context.Background(),
		"https://example.com/org/repo.git",
		workspacePath,
		"v1.2.3",
		AuthOptions{},
	); err != nil {
		t.Fatalf("sync repository tag: %v", err)
	}

	if len(runner.calls) < 1 {
		t.Fatal("expected at least one git call")
	}

	cloneArgs := runner.calls[0].args
	joined := strings.Join(cloneArgs, " ")
	for _, want := range []string{
		"clone",
		"--depth=1",
		"--single-branch",
		"--branch v1.2.3",
		"--no-checkout",
	} {
		if !strings.Contains(joined, want) {
			t.Fatalf("expected clone args to contain %q, got %#v", want, cloneArgs)
		}
	}
}

type workspaceRunnerStub struct {
	calls []workspaceRunnerCall
}

type workspaceRunnerCall struct {
	workingDir string
	name       string
	args       []string
}

func (s *workspaceRunnerStub) Run(
	_ context.Context,
	workingDir string,
	name string,
	args ...string,
) ([]byte, error) {
	s.calls = append(s.calls, workspaceRunnerCall{
		workingDir: workingDir,
		name:       name,
		args:       append([]string(nil), args...),
	})

	if len(args) > 0 && args[len(args)-1] != "-fdx" && strings.Contains(strings.Join(args, " "), "clone") {
		workspacePath := args[len(args)-1]
		if err := os.MkdirAll(filepath.Join(workspacePath, ".git"), 0o755); err != nil {
			return nil, err
		}
	}

	return nil, nil
}