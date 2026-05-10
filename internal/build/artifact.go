package build

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strings"
)

var (
	// ErrArtifactsNotFound reports that a build finished without producing any
	// regular files inside the prepared artifact root.
	ErrArtifactsNotFound = errors.New("build artifacts not found")
)

// discoverArtifacts walks the prepared artifact root, records every regular
// file, and returns the metadata needed for durable artifact registration.
func discoverArtifacts(rootPath string) ([]CreateArtifactInput, error) {
	rootPath = strings.TrimSpace(rootPath)
	if rootPath == "" {
		return nil, fmt.Errorf("%w: artifact_root_path must not be empty", ErrInvalid)
	}

	rootInfo, err := os.Stat(rootPath)
	if err != nil {
		return nil, fmt.Errorf("%w: stat artifact root %q: %v", ErrArtifactsNotFound, rootPath, err)
	}
	if !rootInfo.IsDir() {
		return nil, fmt.Errorf("%w: artifact root %q is not a directory", ErrArtifactsNotFound, rootPath)
	}

	artifacts := make([]CreateArtifactInput, 0)
	err = filepath.WalkDir(rootPath, func(currentPath string, entry os.DirEntry, walkErr error) error {
		if walkErr != nil {
			return walkErr
		}
		if entry.IsDir() {
			return nil
		}

		info, err := entry.Info()
		if err != nil {
			return err
		}
		if !info.Mode().IsRegular() {
			return nil
		}

		relPath, err := filepath.Rel(rootPath, currentPath)
		if err != nil {
			return err
		}

		sizeBytes := info.Size()
		normalizedPath := filepath.ToSlash(relPath)
		artifacts = append(artifacts, CreateArtifactInput{
			Name:      normalizedPath,
			Kind:      detectArtifactKind(normalizedPath),
			Path:      normalizedPath,
			SizeBytes: &sizeBytes,
		})

		return nil
	})
	if err != nil {
		return nil, fmt.Errorf("discover artifacts under %q: %w", rootPath, err)
	}

	if len(artifacts) == 0 {
		return nil, fmt.Errorf("%w: no files found under %q", ErrArtifactsNotFound, rootPath)
	}

	sort.Slice(artifacts, func(left int, right int) bool {
		return artifacts[left].Path < artifacts[right].Path
	})

	return artifacts, nil
}

// detectArtifactKind infers a coarse artifact class from the file extension so
// later publish paths can distinguish archives, binaries, and generic files.
func detectArtifactKind(path string) string {
	switch strings.ToLower(filepath.Ext(path)) {
	case ".zip", ".tar", ".gz", ".tgz", ".bz2", ".xz", ".7z":
		return "archive"
	case ".apk", ".aab", ".ipa", ".exe", ".appimage", ".pkg", ".dmg":
		return "binary"
	default:
		return "file"
	}
}