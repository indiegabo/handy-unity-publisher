package build

import (
	"net/url"
	"path"
	"path/filepath"
	"strings"
	"unicode"

	"golang.org/x/text/unicode/norm"
)

// artifactReleaseDirName returns the canonical repository-and-tag directory
// name used to group release artifacts on disk.
func artifactReleaseDirName(repositoryName string, repositoryURL string, gitTag string) string {
	return joinArtifactNameParts(
		artifactRepositoryName(repositoryName, repositoryURL),
		gitTag,
	)
}

// artifactOutputRelativePath returns the canonical artifact file name for one
// execution plan relative to the prepared artifact root.
func artifactOutputRelativePath(plan ExecutionPlan) string {
	baseName := joinArtifactNameParts(
		artifactRepositoryName(plan.RepositoryName, plan.RepositoryURL),
		plan.GitTag,
		plan.TargetName,
	)
	extension := artifactOutputExtension(plan)
	if extension == "" {
		return baseName
	}

	return baseName + extension
}

// artifactOutputExtension chooses the operator-facing file extension from the
// output contract, forcing archive outputs to `.zip`.
func artifactOutputExtension(plan ExecutionPlan) string {
	if strings.EqualFold(strings.TrimSpace(pointerString(plan.OutputKind)), "archive") {
		return ".zip"
	}

	trimmedPath := strings.TrimSpace(pointerString(plan.OutputPathTemplate))
	if trimmedPath == "" {
		return ""
	}

	return strings.ToLower(filepath.Ext(trimmedPath))
}

// artifactRepositoryName resolves the repository segment used in canonical
// artifact naming from the stored name or repository URL fallback.
func artifactRepositoryName(repositoryName string, repositoryURL string) string {
	trimmedName := strings.TrimSpace(repositoryName)
	if trimmedName != "" {
		return normalizeRepositoryArtifactName(trimmedName)
	}

	trimmedURL := strings.TrimSpace(repositoryURL)
	if trimmedURL == "" {
		return "repository"
	}

	if parsed, err := url.Parse(trimmedURL); err == nil {
		base := path.Base(strings.TrimSuffix(parsed.Path, "/"))
		if base != "" && base != "." && base != "/" {
			return normalizeRepositoryArtifactName(strings.TrimSuffix(base, ".git"))
		}
	}

	return normalizeRepositoryArtifactName(strings.TrimSuffix(filepath.Base(trimmedURL), ".git"))
}

// normalizeRepositoryArtifactName converts a durable repository name into the
// lowercase ASCII slug used in canonical artifact names.
func normalizeRepositoryArtifactName(input string) string {
	trimmed := strings.TrimSpace(strings.ToLower(input))
	if trimmed == "" {
		return "repository"
	}

	decomposed := norm.NFD.String(trimmed)
	var builder strings.Builder
	previousSeparator := false

	for _, character := range decomposed {
		if unicode.Is(unicode.Mn, character) {
			continue
		}

		switch {
		case character >= 'a' && character <= 'z', character >= '0' && character <= '9':
			builder.WriteRune(character)
			previousSeparator = false
		case unicode.IsSpace(character) || character == '-' || character == '_' || character == '.':
			if previousSeparator {
				continue
			}

			builder.WriteRune('-')
			previousSeparator = true
		default:
			if previousSeparator {
				continue
			}

			builder.WriteRune('-')
			previousSeparator = true
		}
	}

	normalized := strings.Trim(builder.String(), "-")
	if normalized == "" {
		return "repository"
	}

	return normalized
}

// joinArtifactNameParts normalizes and joins canonical artifact name segments
// with `.` separators.
func joinArtifactNameParts(parts ...string) string {
	cleaned := make([]string, 0, len(parts))
	for _, part := range parts {
		normalized := normalizeArtifactNamePart(part)
		if normalized == "" {
			continue
		}

		cleaned = append(cleaned, normalized)
	}

	if len(cleaned) == 0 {
		return "artifact"
	}

	return strings.Join(cleaned, ".")
}

// normalizeArtifactNamePart removes path-hostile characters from one generic
// artifact name segment while preserving letters, digits, and common
// separators.
func normalizeArtifactNamePart(input string) string {
	trimmed := strings.TrimSpace(input)
	if trimmed == "" {
		return ""
	}

	var builder strings.Builder
	previousSeparator := false
	for _, character := range trimmed {
		switch {
		case unicode.IsLetter(character), unicode.IsDigit(character):
			builder.WriteRune(character)
			previousSeparator = false
		case character == '.', character == '-', character == '_':
			builder.WriteRune(character)
			previousSeparator = false
		default:
			if previousSeparator {
				continue
			}

			builder.WriteRune('-')
			previousSeparator = true
		}
	}

	normalized := strings.Trim(builder.String(), ".-_")
	if normalized == "" {
		return "artifact"
	}

	return normalized
}

// pointerString dereferences a string pointer and returns the empty string for
// nil values.
func pointerString(value *string) string {
	if value == nil {
		return ""
	}

	return *value
}

// plainStringPointer copies one string into a stable pointer value.
func plainStringPointer(value string) *string {
	copy := value
	return &copy
}
