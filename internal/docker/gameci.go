// Package docker executes GameCI-compatible Unity builds through the Docker
// CLI available next to the mounted Docker socket.
package docker

import (
	"context"
	"errors"
	"fmt"
	"os"
	"os/exec"
	"path"
	"path/filepath"
	"strconv"
	"strings"
	"time"

	"github.com/indiegabo/handy-unity-bulder/internal/build"
)

// dockerCommand, containerProjectPath, containerArtifactsPath, and
// defaultOutputPathEnvName define the fixed Docker CLI contract used by the
// GameCI executor.
const (
	dockerCommand                        = "docker"
	containerProjectPath                 = "/workspace"
	containerArtifactsPath               = "/artifacts"
	containerRuntimeHomePath             = "/tmp/hgb-home"
	containerSerialLicenseDirPath        = "/tmp/hgb-home/.local/share/unity3d/Unity"
	containerSerialLicenseFilePath       = "/tmp/hgb-home/.local/share/unity3d/Unity/Unity_lic.ulf"
	containerNamedUserLicenseDirPath     = "/tmp/hgb-home/.config/unity3d/Unity/licenses"
	containerNamedUserLicenseFilePath    = "/tmp/hgb-home/.config/unity3d/Unity/licenses/UnityEntitlementLicense.xml"
	containerUnityLicenseSourceMountPath = "/tmp/hgb-license-source"
	defaultOutputPathEnvName             = "HGB_OUTPUT_PATH"
)

const (
	unityLicenseEnvName     = "UNITY_LICENSE"
	unityLicenseFileEnvName = "UNITY_LICENSE_FILE"
	runtimeDataDirEnvName   = "DATA_DIR"
	hostDataDirEnvName      = "HOST_DATA_DIR"
)

// licensingEnvKeys lists Unity licensing variables that are forwarded from the
// worker process into the ephemeral GameCI container.
var licensingEnvKeys = []string{
	"UNITY_LICENSE",
	"UNITY_EMAIL",
	"UNITY_PASSWORD",
	"UNITY_SERIAL",
	"UNITY_LICENSING_SERVER",
}

// commandRunner abstracts Docker CLI execution for focused executor tests.
type commandRunner interface {
	Run(ctx context.Context, name string, args ...string) ([]byte, error)
}

// execRunner executes Docker CLI commands through `os/exec`.
type execRunner struct{}

// Run executes one host command and returns combined stdout and stderr.
func (execRunner) Run(
	ctx context.Context,
	name string,
	args ...string,
) ([]byte, error) {
	command := exec.CommandContext(ctx, name, args...)
	return command.CombinedOutput()
}

// GameCIExecutor runs one Unity build in a GameCI editor image.
type GameCIExecutor struct {
	runner commandRunner
}

// unityLicenseMount describes one optional host license file bind and the
// canonical target path Unity expects inside the nested GameCI container.
type unityLicenseMount struct {
	hostPath      string
	containerPath string
}

// NewGameCIExecutor creates the default Docker CLI-backed executor.
func NewGameCIExecutor() *GameCIExecutor {
	return &GameCIExecutor{runner: execRunner{}}
}

// newGameCIExecutorWithRunner injects a command runner for focused tests.
func newGameCIExecutorWithRunner(runner commandRunner) *GameCIExecutor {
	return &GameCIExecutor{runner: runner}
}

// Execute runs one prepared Unity build in a GameCI-compatible editor image
// and returns the captured container logs.
func (e *GameCIExecutor) Execute(
	ctx context.Context,
	request build.ExecuteRequest,
) ([]byte, error) {
	args, err := buildRunArgs(request)
	if err != nil {
		return nil, err
	}

	timeout := time.Duration(request.Plan.TimeoutSeconds) * time.Second
	if timeout > 0 {
		var cancel context.CancelFunc
		ctx, cancel = context.WithTimeout(ctx, timeout)
		defer cancel()
	}

	output, err := e.runner.Run(ctx, dockerCommand, args...)
	if err != nil {
		if errors.Is(ctx.Err(), context.DeadlineExceeded) {
			return output, fmt.Errorf(
				"GameCI build timed out after %s: %w",
				timeout,
				err,
			)
		}

		return output, fmt.Errorf("run GameCI build container: %w", err)
	}

	return output, nil
}

// buildRunArgs constructs the Docker CLI invocation used to execute one build
// inside a GameCI editor image.
func buildRunArgs(request build.ExecuteRequest) ([]string, error) {
	plan := request.Plan
	workspace := request.Workspace

	if strings.TrimSpace(plan.ImageRef) == "" {
		return nil, fmt.Errorf("%w: image_ref must not be empty", build.ErrInvalid)
	}
	if strings.TrimSpace(plan.RunnerType) != build.DefaultRunnerType {
		return nil, fmt.Errorf(
			"%w: runner_type %q is not supported by the GameCI executor",
			build.ErrInvalid,
			plan.RunnerType,
		)
	}
	if plan.BuildMethod == nil || strings.TrimSpace(*plan.BuildMethod) == "" {
		return nil, fmt.Errorf(
			"%w: build target %d requires build_method for execution",
			build.ErrInvalid,
			plan.BuildTargetID,
		)
	}
	if strings.TrimSpace(workspace.HostSourcePath) == "" || strings.TrimSpace(workspace.HostArtifactRootPath) == "" {
		return nil, fmt.Errorf("%w: host workspace paths must not be empty", build.ErrInvalid)
	}

	unityTarget, err := resolveUnityBuildTarget(plan.Platform)
	if err != nil {
		return nil, err
	}

	containerOutputPath, err := resolveContainerOutputPath(plan.OutputPathTemplate)
	if err != nil {
		return nil, err
	}
	unityLicenseMount, hasUnityLicenseFile, err := resolveOptionalUnityLicenseFileMount()
	if err != nil {
		return nil, err
	}
	containerConfigPath := path.Join(containerRuntimeHomePath, ".config")
	containerCachePath := path.Join(containerRuntimeHomePath, ".cache")
	containerDataPath := path.Join(containerRuntimeHomePath, ".local", "share")

	args := []string{
		"run",
		"--rm",
		"--user", currentRuntimeUserSpec(),
		"--label", fmt.Sprintf("hgb.build_run_id=%d", plan.BuildRunID),
		"--label", fmt.Sprintf("hgb.release_run_id=%d", plan.ReleaseRunID),
		"--label", fmt.Sprintf("hgb.build_target_id=%d", plan.BuildTargetID),
		"--workdir", containerProjectPath,
		"--volume", fmt.Sprintf("%s:%s", workspace.HostSourcePath, containerProjectPath),
		"--volume", fmt.Sprintf("%s:%s", workspace.HostArtifactRootPath, containerArtifactsPath),
	}
	if hasUnityLicenseFile {
		args = append(
			args,
			"--volume",
			fmt.Sprintf(
				"%s:%s:ro",
				unityLicenseMount.hostPath,
				containerUnityLicenseSourceMountPath,
			),
		)
	}

	for _, envVar := range buildExecutionEnv(
		plan,
		containerOutputPath,
		containerConfigPath,
		containerCachePath,
		containerDataPath,
		hasUnityLicenseFile,
	) {
		args = append(args, "--env", envVar)
	}

	unityCommand := shellJoin(
		"unity-editor",
		"-batchmode",
		"-quit",
		"-nographics",
		"-logFile",
		"/dev/stdout",
		"-projectPath",
		containerProjectPath,
		"-buildTarget",
		unityTarget,
		"-executeMethod",
		strings.TrimSpace(*plan.BuildMethod),
		"-hgbOutputPath",
		containerOutputPath,
	)
	bootstrapCommand := shellJoin(
		"mkdir",
		"-p",
		containerRuntimeHomePath,
		containerConfigPath,
		containerCachePath,
		containerDataPath,
		containerSerialLicenseDirPath,
		containerNamedUserLicenseDirPath,
	) + " && exec " + unityCommand
	if hasUnityLicenseFile {
		bootstrapCommand = shellJoin(
			"mkdir",
			"-p",
			containerRuntimeHomePath,
			containerConfigPath,
			containerCachePath,
			containerDataPath,
			containerSerialLicenseDirPath,
			containerNamedUserLicenseDirPath,
		) + " && " + shellJoin(
			"cp",
			containerUnityLicenseSourceMountPath,
			unityLicenseMount.containerPath,
		) + " && exec " + unityCommand
	}

	args = append(
		args,
		plan.ImageRef,
		"sh",
		"-lc",
		bootstrapCommand,
	)

	return args, nil
}

// buildExecutionEnv returns the environment variables forwarded to the GameCI
// container for build metadata, output location, and Unity licensing.
func buildExecutionEnv(
	plan build.ExecutionPlan,
	containerOutputPath string,
	containerConfigPath string,
	containerCachePath string,
	containerDataPath string,
	hasMountedLicenseFile bool,
) []string {
	envs := []string{
		fmt.Sprintf("HGB_BUILD_RUN_ID=%d", plan.BuildRunID),
		fmt.Sprintf("HGB_RELEASE_RUN_ID=%d", plan.ReleaseRunID),
		fmt.Sprintf("HGB_BUILD_TARGET_ID=%d", plan.BuildTargetID),
		fmt.Sprintf("HGB_TARGET_PLATFORM=%s", strings.TrimSpace(plan.Platform)),
		fmt.Sprintf("HGB_UNITY_VERSION=%s", strings.TrimSpace(plan.UnityVersion)),
		fmt.Sprintf("%s=%s", defaultOutputPathEnvName, containerOutputPath),
		fmt.Sprintf("HOME=%s", containerRuntimeHomePath),
		fmt.Sprintf("XDG_CONFIG_HOME=%s", containerConfigPath),
		fmt.Sprintf("XDG_CACHE_HOME=%s", containerCachePath),
		fmt.Sprintf("XDG_DATA_HOME=%s", containerDataPath),
		"TMPDIR=/tmp",
	}

	if plan.OutputKind != nil && strings.TrimSpace(*plan.OutputKind) != "" {
		envs = append(envs, fmt.Sprintf("HGB_OUTPUT_KIND=%s", strings.TrimSpace(*plan.OutputKind)))
	}
	if plan.GitCommit != nil && strings.TrimSpace(*plan.GitCommit) != "" {
		envs = append(envs, fmt.Sprintf("HGB_GIT_COMMIT=%s", strings.TrimSpace(*plan.GitCommit)))
	}

	for _, key := range licensingEnvKeys {
		if key == unityLicenseEnvName && hasMountedLicenseFile {
			continue
		}

		value := strings.TrimSpace(os.Getenv(key))
		if value == "" {
			continue
		}

		envs = append(envs, fmt.Sprintf("%s=%s", key, value))
	}

	return envs
}

// resolveOptionalUnityLicenseFileMount resolves the optional worker-visible
// Unity license file path into the host-visible bind source needed by the host
// Docker daemon that launches nested GameCI containers.
func resolveOptionalUnityLicenseFileMount() (unityLicenseMount, bool, error) {
	runtimePath := filepath.Clean(strings.TrimSpace(os.Getenv(unityLicenseFileEnvName)))
	if runtimePath == "" || runtimePath == "." {
		return unityLicenseMount{}, false, nil
	}

	info, err := os.Stat(runtimePath)
	if err != nil {
		if errors.Is(err, os.ErrNotExist) {
			return unityLicenseMount{}, false, fmt.Errorf(
				"%w: %s %q does not exist inside the build worker",
				build.ErrInvalid,
				unityLicenseFileEnvName,
				runtimePath,
			)
		}

		return unityLicenseMount{}, false, fmt.Errorf(
			"inspect %s %q: %w",
			unityLicenseFileEnvName,
			runtimePath,
			err,
		)
	}
	if info.IsDir() {
		return unityLicenseMount{}, false, fmt.Errorf(
			"%w: %s %q must point to a file",
			build.ErrInvalid,
			unityLicenseFileEnvName,
			runtimePath,
		)
	}

	containerPath, err := resolveContainerUnityLicenseTargetPath(runtimePath)
	if err != nil {
		return unityLicenseMount{}, false, err
	}

	return unityLicenseMount{
		hostPath:      translateRuntimePathToHost(runtimePath),
		containerPath: containerPath,
	}, true, nil
}

// resolveContainerUnityLicenseTargetPath maps the configured runtime license
// file to the canonical Unity path required inside the nested build container.
func resolveContainerUnityLicenseTargetPath(runtimePath string) (string, error) {
	base := filepath.Base(strings.TrimSpace(runtimePath))
	switch strings.ToLower(filepath.Ext(base)) {
	case ".ulf":
		return containerSerialLicenseFilePath, nil
	case ".xml":
		if strings.EqualFold(base, filepath.Base(containerNamedUserLicenseFilePath)) {
			return containerNamedUserLicenseFilePath, nil
		}

		return path.Join(containerNamedUserLicenseDirPath, base), nil
	default:
		return "", fmt.Errorf(
			"%w: %s %q must point to a .ulf or .xml file",
			build.ErrInvalid,
			unityLicenseFileEnvName,
			runtimePath,
		)
	}
}

// translateRuntimePathToHost converts a worker-visible path under DATA_DIR to
// the matching host-visible path required by the host Docker daemon.
func translateRuntimePathToHost(runtimePath string) string {
	runtimePath = filepath.Clean(strings.TrimSpace(runtimePath))
	if runtimePath == "" || runtimePath == "." {
		return runtimePath
	}

	dataDir := filepath.Clean(strings.TrimSpace(os.Getenv(runtimeDataDirEnvName)))
	hostDataDir := filepath.Clean(strings.TrimSpace(os.Getenv(hostDataDirEnvName)))
	if dataDir == "" || dataDir == "." || hostDataDir == "" || hostDataDir == "." {
		return runtimePath
	}
	if runtimePath == dataDir {
		return hostDataDir
	}

	prefix := dataDir + string(filepath.Separator)
	if !strings.HasPrefix(runtimePath, prefix) {
		return runtimePath
	}

	return filepath.Join(hostDataDir, strings.TrimPrefix(runtimePath, prefix))
}

// shellJoin quotes each token for POSIX sh and concatenates them into one
// command string.
func shellJoin(tokens ...string) string {
	quoted := make([]string, 0, len(tokens))
	for _, token := range tokens {
		quoted = append(quoted, shellQuote(token))
	}

	return strings.Join(quoted, " ")
}

// shellQuote wraps one token for safe reuse inside `sh -lc`.
func shellQuote(token string) string {
	return "'" + strings.ReplaceAll(token, "'", "'\"'\"'") + "'"
}

// resolveContainerOutputPath validates and normalizes the build output path so
// the container only writes inside the mounted artifact root.
func resolveContainerOutputPath(outputPathTemplate *string) (string, error) {
	if outputPathTemplate == nil {
		return containerArtifactsPath, nil
	}

	raw := strings.TrimSpace(*outputPathTemplate)
	if raw == "" {
		return containerArtifactsPath, nil
	}

	normalized := strings.ReplaceAll(raw, `\`, "/")
	cleaned := strings.TrimPrefix(path.Clean("/"+normalized), "/")
	if cleaned == "." || cleaned == "" {
		return containerArtifactsPath, nil
	}
	if strings.HasPrefix(cleaned, "../") || cleaned == ".." {
		return "", fmt.Errorf(
			"%w: output_path_template must stay within artifact root",
			build.ErrInvalid,
		)
	}

	return path.Join(containerArtifactsPath, cleaned), nil
}

// resolveUnityBuildTarget maps the persisted platform label to the Unity CLI
// `-buildTarget` value expected by GameCI.
func resolveUnityBuildTarget(platform string) (string, error) {
	switch strings.ToLower(strings.TrimSpace(platform)) {
	case "linux", "standalonelinux64", "base", "linux-il2cpp":
		return "StandaloneLinux64", nil
	case "windows", "windows-mono", "windows-il2cpp", "standalonewindows", "standalonewindows64":
		return "StandaloneWindows64", nil
	case "webgl":
		return "WebGL", nil
	case "android":
		return "Android", nil
	case "ios":
		return "iOS", nil
	case "mac", "macos", "osx", "standaloneosx", "mac-mono":
		return "StandaloneOSX", nil
	case "tvos", "appletv":
		return "tvOS", nil
	default:
		return "", fmt.Errorf(
			"%w: unsupported Unity build target platform %q",
			build.ErrInvalid,
			platform,
		)
	}
}

// buildRunIDString formats one build run id for environments that still expect
// a decimal string value.
func buildRunIDString(id int64) string {
	return strconv.FormatInt(id, 10)
}

// currentRuntimeUserSpec returns the current process uid:gid pair so nested
// GameCI containers write files with the same host-visible ownership as the
// worker process driving them.
func currentRuntimeUserSpec() string {
	return strconv.Itoa(os.Getuid()) + ":" + strconv.Itoa(os.Getgid())
}
