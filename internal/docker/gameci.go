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
	"strconv"
	"strings"
	"time"

	"github.com/indiegabo/handy-unity-bulder/internal/build"
)

// dockerCommand, containerProjectPath, containerArtifactsPath, and
// defaultOutputPathEnvName define the fixed Docker CLI contract used by the
// GameCI executor.
const (
	dockerCommand            = "docker"
	containerProjectPath     = "/workspace"
	containerArtifactsPath   = "/artifacts"
	defaultOutputPathEnvName = "HGB_OUTPUT_PATH"
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

	for _, envVar := range buildExecutionEnv(plan, containerOutputPath) {
		args = append(args, "--env", envVar)
	}

	args = append(
		args,
		plan.ImageRef,
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

	return args, nil
}

// buildExecutionEnv returns the environment variables forwarded to the GameCI
// container for build metadata, output location, and Unity licensing.
func buildExecutionEnv(plan build.ExecutionPlan, containerOutputPath string) []string {
	envs := []string{
		fmt.Sprintf("HGB_BUILD_RUN_ID=%d", plan.BuildRunID),
		fmt.Sprintf("HGB_RELEASE_RUN_ID=%d", plan.ReleaseRunID),
		fmt.Sprintf("HGB_BUILD_TARGET_ID=%d", plan.BuildTargetID),
		fmt.Sprintf("HGB_TARGET_PLATFORM=%s", strings.TrimSpace(plan.Platform)),
		fmt.Sprintf("HGB_UNITY_VERSION=%s", strings.TrimSpace(plan.UnityVersion)),
		fmt.Sprintf("%s=%s", defaultOutputPathEnvName, containerOutputPath),
	}

	if plan.OutputKind != nil && strings.TrimSpace(*plan.OutputKind) != "" {
		envs = append(envs, fmt.Sprintf("HGB_OUTPUT_KIND=%s", strings.TrimSpace(*plan.OutputKind)))
	}
	if plan.GitCommit != nil && strings.TrimSpace(*plan.GitCommit) != "" {
		envs = append(envs, fmt.Sprintf("HGB_GIT_COMMIT=%s", strings.TrimSpace(*plan.GitCommit)))
	}

	for _, key := range licensingEnvKeys {
		value := strings.TrimSpace(os.Getenv(key))
		if value == "" {
			continue
		}

		envs = append(envs, fmt.Sprintf("%s=%s", key, value))
	}

	return envs
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