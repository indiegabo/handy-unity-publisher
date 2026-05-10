package publish

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"
)

// filesystemTargetConfig holds the JSON configuration accepted by the
// filesystem publisher.
type filesystemTargetConfig struct {
	RootPath string `json:"root_path"`
}

// FilesystemPublisher copies one build artifact into a configured filesystem
// root using repository and tag names to keep releases separated.
type FilesystemPublisher struct{}

// NewFilesystemPublisher creates the local filesystem publisher used by V1.
func NewFilesystemPublisher() *FilesystemPublisher {
	return &FilesystemPublisher{}
}

// Publish copies one artifact into the configured filesystem destination.
func (p *FilesystemPublisher) Publish(
	ctx context.Context,
	plan ExecutionPlan,
) (ExecutionResult, error) {
	config, err := parseFilesystemTargetConfig(plan.PublishTargetConfigJSON)
	if err != nil {
		return ExecutionResult{}, err
	}

	repositorySegment, err := sanitizePathSegment(plan.RepositoryName, "repository_name")
	if err != nil {
		return ExecutionResult{}, err
	}

	gitTagSegment, err := sanitizePathSegment(plan.GitTag, "git_tag")
	if err != nil {
		return ExecutionResult{}, err
	}

	relativeArtifactPath, err := normalizeRelativeArtifactPath(plan.ArtifactPath)
	if err != nil {
		return ExecutionResult{}, err
	}

	destinationPath := filepath.Join(
		config.RootPath,
		repositorySegment,
		gitTagSegment,
		filepath.FromSlash(relativeArtifactPath),
	)
	if err := copyRegularFile(ctx, plan.SourcePath, destinationPath); err != nil {
		return ExecutionResult{}, err
	}

	return ExecutionResult{DestinationRef: destinationPath}, nil
}

// parseFilesystemTargetConfig decodes and validates filesystem publish target
// configuration.
func parseFilesystemTargetConfig(raw string) (filesystemTargetConfig, error) {
	trimmed := strings.TrimSpace(raw)
	if trimmed == "" {
		trimmed = `{}`
	}

	var config filesystemTargetConfig
	if err := json.Unmarshal([]byte(trimmed), &config); err != nil {
		return filesystemTargetConfig{}, fmt.Errorf(
			"%w: filesystem target config must be valid JSON: %v",
			ErrInvalid,
			err,
		)
	}

	config.RootPath = filepath.Clean(strings.TrimSpace(config.RootPath))
	if config.RootPath == "" || config.RootPath == "." {
		return filesystemTargetConfig{}, fmt.Errorf(
			"%w: filesystem target root_path must not be empty",
			ErrInvalid,
		)
	}
	if !filepath.IsAbs(config.RootPath) {
		return filesystemTargetConfig{}, fmt.Errorf(
			"%w: filesystem target root_path must be absolute",
			ErrInvalid,
		)
	}

	return config, nil
}

// sanitizePathSegment normalizes a repository or tag name into one safe
// destination path segment.
func sanitizePathSegment(value string, fieldName string) (string, error) {
	segment := strings.TrimSpace(value)
	segment = strings.NewReplacer("/", "-", "\\", "-").Replace(segment)
	if segment == "" || segment == "." || segment == ".." {
		return "", fmt.Errorf("%w: %s must not be empty", ErrInvalid, fieldName)
	}

	return segment, nil
}

// normalizeRelativeArtifactPath validates that the stored artifact path stays
// relative to the artifact root.
func normalizeRelativeArtifactPath(path string) (string, error) {
	trimmed := strings.TrimSpace(path)
	if trimmed == "" {
		return "", fmt.Errorf("%w: artifact path must not be empty", ErrInvalid)
	}

	normalized := filepath.Clean(filepath.FromSlash(trimmed))
	if normalized == "." || filepath.IsAbs(normalized) {
		return "", fmt.Errorf("%w: artifact path must be relative", ErrInvalid)
	}
	if normalized == ".." || strings.HasPrefix(normalized, ".."+string(os.PathSeparator)) {
		return "", fmt.Errorf("%w: artifact path must not escape the artifact root", ErrInvalid)
	}

	return filepath.ToSlash(normalized), nil
}

// copyRegularFile copies one artifact into place through a temporary file so
// consumers never observe partial output.
func copyRegularFile(ctx context.Context, sourcePath string, destinationPath string) error {
	if err := ctx.Err(); err != nil {
		return err
	}

	info, err := os.Stat(sourcePath)
	if err != nil {
		return fmt.Errorf("stat publish source %q: %w", sourcePath, err)
	}
	if !info.Mode().IsRegular() {
		return fmt.Errorf("%w: publish source %q is not a regular file", ErrInvalid, sourcePath)
	}

	if err := os.MkdirAll(filepath.Dir(destinationPath), 0o755); err != nil {
		return fmt.Errorf("create publish destination directory for %q: %w", destinationPath, err)
	}

	temporaryPath := filepath.Join(
		filepath.Dir(destinationPath),
		"."+filepath.Base(destinationPath)+".tmp",
	)

	sourceFile, err := os.Open(sourcePath)
	if err != nil {
		return fmt.Errorf("open publish source %q: %w", sourcePath, err)
	}
	defer sourceFile.Close()

	temporaryFile, err := os.OpenFile(
		temporaryPath,
		os.O_CREATE|os.O_TRUNC|os.O_WRONLY,
		info.Mode().Perm(),
	)
	if err != nil {
		return fmt.Errorf("open publish temporary file %q: %w", temporaryPath, err)
	}

	copyErr := func() error {
		defer temporaryFile.Close()

		if _, err := io.Copy(temporaryFile, sourceFile); err != nil {
			return fmt.Errorf("copy publish file to %q: %w", temporaryPath, err)
		}
		if err := ctx.Err(); err != nil {
			return err
		}
		if err := temporaryFile.Sync(); err != nil {
			return fmt.Errorf("sync publish temporary file %q: %w", temporaryPath, err)
		}

		return nil
	}()
	if copyErr != nil {
		_ = os.Remove(temporaryPath)
		return copyErr
	}

	if err := os.Rename(temporaryPath, destinationPath); err != nil {
		_ = os.Remove(temporaryPath)
		return fmt.Errorf("move publish file into place %q: %w", destinationPath, err)
	}

	return nil
}