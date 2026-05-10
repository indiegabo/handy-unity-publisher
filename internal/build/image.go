package build

import (
	"fmt"
	"regexp"
	"strings"
)

const (
	// DefaultContainerRegistryRepository is the default GameCI image repository.
	DefaultContainerRegistryRepository = "unityci/editor"
	// DefaultContainerRegistryImageVersion is the rolling GameCI image tag
	// generation used by unity-builder.
	DefaultContainerRegistryImageVersion = "3"
	// DefaultImagePlatformPrefix selects the Linux-hosted GameCI image family.
	DefaultImagePlatformPrefix = "ubuntu"
)

// unityVersionPattern accepts Unity editor versions in the GameCI image tag
// format used by the resolver.
var unityVersionPattern = regexp.MustCompile(`^\d+\.\d+\.\d+[A-Za-z]\d+$`)

// imageReferenceResolver resolves the concrete container image used to execute
// one build target.
type imageReferenceResolver interface {
	Resolve(target Target, unityVersion string) (string, error)
}

// gameCIImageResolver builds deterministic GameCI image references from build
// target metadata and Unity version input.
type gameCIImageResolver struct {
	repository      string
	rollingVersion  string
	platformPrefix  string
}

// newGameCIImageResolver creates the default GameCI image resolver used by
// release planning.
func newGameCIImageResolver() imageReferenceResolver {
	return &gameCIImageResolver{
		repository:     DefaultContainerRegistryRepository,
		rollingVersion: DefaultContainerRegistryImageVersion,
		platformPrefix: DefaultImagePlatformPrefix,
	}
}

// Resolve returns either the explicit image override or a deterministic GameCI
// image reference compatible with the target platform and Unity version.
func (r *gameCIImageResolver) Resolve(
	target Target,
	unityVersion string,
) (string, error) {
	if target.ImageOverride != nil {
		override := strings.TrimSpace(*target.ImageOverride)
		if override != "" {
			return override, nil
		}
	}

	if strings.TrimSpace(target.RunnerType) != DefaultRunnerType {
		return "", fmt.Errorf(
			"%w: runner_type %q is not supported for image resolution",
			ErrImageResolutionUnavailable,
			target.RunnerType,
		)
	}

	unityVersion = strings.TrimSpace(unityVersion)
	if !unityVersionPattern.MatchString(unityVersion) {
		return "", fmt.Errorf(
			"%w: unity version %q is invalid",
			ErrImageResolutionUnavailable,
			unityVersion,
		)
	}

	suffix, err := resolveGameCIPlatformSuffix(target.Platform)
	if err != nil {
		return "", err
	}

	tag := fmt.Sprintf(
		"%s-%s-%s",
		r.platformPrefix,
		strings.Trim(strings.Join([]string{unityVersion, suffix}, "-"), "-"),
		r.rollingVersion,
	)

	return fmt.Sprintf("%s:%s", r.repository, tag), nil
}

// resolveGameCIPlatformSuffix maps the stored build platform name to the
// GameCI module suffix required in the image tag.
func resolveGameCIPlatformSuffix(platform string) (string, error) {
	normalized := strings.ToLower(strings.TrimSpace(platform))
	switch normalized {
	case "", "generic", "notarget", "linux", "standalonelinux64", "base":
		return "base", nil
	case "linux-il2cpp":
		return "linux-il2cpp", nil
	case "windows", "windows-mono", "standalonewindows", "standalonewindows64":
		return "windows-mono", nil
	case "windows-il2cpp":
		return "windows-il2cpp", nil
	case "wsa", "wsaplayer", "universal-windows-platform":
		return "universal-windows-platform", nil
	case "webgl":
		return "webgl", nil
	case "android":
		return "android", nil
	case "ios":
		return "ios", nil
	case "mac", "macos", "osx", "standaloneosx", "mac-mono":
		return "mac-mono", nil
	case "tvos", "appletv":
		return "appletv", nil
	case "visionos":
		return "visionos", nil
	case "facebook":
		return "facebook", nil
	default:
		return "", fmt.Errorf(
			"%w: platform %q is not supported for GameCI image resolution",
			ErrImageResolutionUnavailable,
			platform,
		)
	}
}

// resolveTargetUnityVersion prefers the target override and otherwise falls
// back to the Unity version persisted on the release run.
func resolveTargetUnityVersion(target Target, releaseUnityVersion string) string {
	if target.UnityVersionOverride != nil {
		override := strings.TrimSpace(*target.UnityVersionOverride)
		if override != "" {
			return override
		}
	}

	return strings.TrimSpace(releaseUnityVersion)
}