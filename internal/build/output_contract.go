package build

import (
	"fmt"
	"strings"
)

// ValidateRequestedOutputPath validates the operator-provided output path
// template used to communicate artifact shape to one build method.
//
// The value is not the final operator-visible artifact filename on disk. The
// runtime rewrites stored outputs to canonical names before execution. Archive
// outputs therefore must not use a `.zip` suffix in the requested path
// template, because that suffix falsely suggests control over the final file
// name.
func ValidateRequestedOutputPath(outputKind string, outputPathTemplate string) error {
	trimmedKind := strings.TrimSpace(outputKind)
	trimmedPath := strings.TrimSpace(outputPathTemplate)
	if trimmedKind == "" || trimmedPath == "" {
		return nil
	}

	normalizedPath := strings.ToLower(strings.ReplaceAll(trimmedPath, `\`, "/"))
	if strings.EqualFold(trimmedKind, "archive") && strings.HasSuffix(normalizedPath, ".zip") {
		return fmt.Errorf(
			"%w: archive output_path_template is a requested build path, not the final artifact filename; remove the .zip suffix",
			ErrInvalid,
		)
	}

	return nil
}