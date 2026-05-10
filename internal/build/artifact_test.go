package build

import (
	"errors"
	"os"
	"path/filepath"
	"testing"
)

func TestDiscoverArtifactsCollectsRelativeFiles(t *testing.T) {
	t.Parallel()

	rootPath := t.TempDir()
	archivePath := filepath.Join(rootPath, "Builds", "linux-player.zip")
	textPath := filepath.Join(rootPath, "Builds", "notes.txt")

	for path, contents := range map[string]string{
		archivePath: "zip-bytes",
		textPath:    "notes",
	} {
		if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
			t.Fatalf("create artifact parent directory: %v", err)
		}
		if err := os.WriteFile(path, []byte(contents), 0o644); err != nil {
			t.Fatalf("write artifact fixture: %v", err)
		}
	}

	artifacts, err := discoverArtifacts(rootPath)
	if err != nil {
		t.Fatalf("discover artifacts: %v", err)
	}

	if len(artifacts) != 2 {
		t.Fatalf("expected two discovered artifacts, got %d", len(artifacts))
	}

	if artifacts[0].Path != "Builds/linux-player.zip" {
		t.Fatalf("expected first artifact path %q, got %q", "Builds/linux-player.zip", artifacts[0].Path)
	}

	if artifacts[0].Kind != "archive" {
		t.Fatalf("expected archive kind, got %q", artifacts[0].Kind)
	}

	if artifacts[1].Path != "Builds/notes.txt" {
		t.Fatalf("expected second artifact path %q, got %q", "Builds/notes.txt", artifacts[1].Path)
	}
}

func TestDiscoverArtifactsFailsWhenNoFilesExist(t *testing.T) {
	t.Parallel()

	_, err := discoverArtifacts(t.TempDir())
	if !errors.Is(err, ErrArtifactsNotFound) {
		t.Fatalf("expected artifacts not found error, got %v", err)
	}
}