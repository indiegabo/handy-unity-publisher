package build

import (
	"context"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/indiegabo/handy-unity-bulder/internal/config"
)

func TestWorkspacePreparerCreatesIsolatedRunDirectories(t *testing.T) {
	t.Parallel()

	dataDir := t.TempDir()
	cfg := config.Config{DataDir: dataDir}
	repositoryPath := newUnityTaggedRepository(t, "2022.3.14f1", "v5.0.0")
	preparer := NewWorkspacePreparer(cfg)

	prepared, err := preparer.Prepare(context.Background(), WorkspacePreparationInput{
		BuildRunID:     42,
		RepositoryName: "revolutions",
		RepositoryURL:  "file://" + repositoryPath,
		GitTag:         "v5.0.0",
	})
	if err != nil {
		t.Fatalf("prepare workspace: %v", err)
	}

	expectedRoot := filepath.Join(cfg.WorkspacesDir(), "build-run-42")
	if prepared.RootPath != expectedRoot {
		t.Fatalf("expected root path %q, got %q", expectedRoot, prepared.RootPath)
	}

	if prepared.HostRootPath != expectedRoot {
		t.Fatalf("expected host root path %q, got %q", expectedRoot, prepared.HostRootPath)
	}

	projectVersionPath := filepath.Join(prepared.SourcePath, projectVersionFilePath)
	contents, err := os.ReadFile(projectVersionPath)
	if err != nil {
		t.Fatalf("read prepared project version file: %v", err)
	}

	if !strings.Contains(string(contents), "m_EditorVersion: 2022.3.14f1") {
		t.Fatalf("expected prepared workspace to contain requested tag contents, got %q", string(contents))
	}

	for _, path := range []string{prepared.RootPath, prepared.SourcePath, prepared.ArtifactRootPath} {
		info, err := os.Stat(path)
		if err != nil {
			t.Fatalf("stat prepared path %q: %v", path, err)
		}
		if !info.IsDir() {
			t.Fatalf("expected prepared path %q to be a directory", path)
		}
	}

	if filepath.Dir(prepared.LogPath) != cfg.LogsDir() {
		t.Fatalf("expected log path under %q, got %q", cfg.LogsDir(), prepared.LogPath)
	}

	expectedArtifactRoot := filepath.Join(cfg.ArtifactsDir(), "revolutions.v5.0.0")
	if prepared.ArtifactRootPath != expectedArtifactRoot {
		t.Fatalf("expected artifact root %q, got %q", expectedArtifactRoot, prepared.ArtifactRootPath)
	}

	if prepared.HostArtifactRootPath != prepared.ArtifactRootPath {
		t.Fatalf("expected host artifact root %q, got %q", prepared.ArtifactRootPath, prepared.HostArtifactRootPath)
	}
}

func TestWorkspacePreparerGroupsArtifactsByRepositoryAndTag(t *testing.T) {
	t.Parallel()

	dataDir := t.TempDir()
	cfg := config.Config{DataDir: dataDir}
	repositoryPath := newUnityTaggedRepository(t, "2021.3.18f1", "v6.0.0")
	preparer := NewWorkspacePreparer(cfg)

	first, err := preparer.Prepare(context.Background(), WorkspacePreparationInput{
		BuildRunID:     7,
		RepositoryName: "revolutions",
		RepositoryURL:  "file://" + repositoryPath,
		GitTag:         "v6.0.0",
	})
	if err != nil {
		t.Fatalf("prepare first workspace: %v", err)
	}

	second, err := preparer.Prepare(context.Background(), WorkspacePreparationInput{
		BuildRunID:     8,
		RepositoryName: "revolutions",
		RepositoryURL:  "file://" + repositoryPath,
		GitTag:         "v6.0.0",
	})
	if err != nil {
		t.Fatalf("prepare second workspace: %v", err)
	}

	if first.RootPath == second.RootPath {
		t.Fatalf("expected distinct workspace roots, got %q", first.RootPath)
	}

	if first.ArtifactRootPath != second.ArtifactRootPath {
		t.Fatalf("expected shared artifact root for same repository tag, got %q and %q", first.ArtifactRootPath, second.ArtifactRootPath)
	}

	if first.LogPath == second.LogPath {
		t.Fatalf("expected distinct log paths, got %q", first.LogPath)
	}
}
