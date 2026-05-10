package docker

import (
	"context"
	"fmt"
	"os"
	"strings"
	"testing"

	"github.com/indiegabo/handy-unity-bulder/internal/build"
)

func TestGameCIExecutorBuildsDockerCommandWithWorkspaceMounts(t *testing.T) {
	t.Setenv("UNITY_LICENSE", "encoded-license")

	runner := &commandRunnerStub{}
	executor := newGameCIExecutorWithRunner(runner)
	buildMethod := "Builder.PerformLinux"
	outputPathTemplate := "Builds/Linux"

	_, err := executor.Execute(context.Background(), build.ExecuteRequest{
		Plan: build.ExecutionPlan{
			BuildRunID:         12,
			ReleaseRunID:       8,
			BuildTargetID:      3,
			Platform:           "linux",
			RunnerType:         build.DefaultRunnerType,
			BuildMethod:        &buildMethod,
			OutputPathTemplate: &outputPathTemplate,
			UnityVersion:       "2022.3.14f1",
			ImageRef:           "unityci/editor:ubuntu-2022.3.14f1-base-3",
			TimeoutSeconds:     30,
		},
		Workspace: build.PreparedWorkspace{
			HostSourcePath:       "/host/data/workspaces/build-run-12/source",
			HostArtifactRootPath: "/host/data/artifacts/build-run-12",
		},
	})
	if err != nil {
		t.Fatalf("execute GameCI build: %v", err)
	}

	if runner.name != dockerCommand {
		t.Fatalf("expected docker command %q, got %q", dockerCommand, runner.name)
	}

	args := strings.Join(runner.args, " ")
	for _, fragment := range []string{
		"run --rm",
		fmt.Sprintf("--user %d:%d", os.Getuid(), os.Getgid()),
		"/host/data/workspaces/build-run-12/source:/workspace",
		"/host/data/artifacts/build-run-12:/artifacts",
		"unityci/editor:ubuntu-2022.3.14f1-base-3",
		"unity-editor -batchmode -quit -nographics",
		"-buildTarget StandaloneLinux64",
		"-executeMethod Builder.PerformLinux",
		"HGB_OUTPUT_PATH=/artifacts/Builds/Linux",
		"UNITY_LICENSE=encoded-license",
	} {
		if !strings.Contains(args, fragment) {
			t.Fatalf("expected docker args to contain %q, got %q", fragment, args)
		}
	}
}

func TestGameCIExecutorRejectsMissingBuildMethod(t *testing.T) {
	t.Parallel()

	executor := newGameCIExecutorWithRunner(&commandRunnerStub{})
	_, err := executor.Execute(context.Background(), build.ExecuteRequest{
		Plan: build.ExecutionPlan{
			BuildRunID:     5,
			BuildTargetID:  2,
			Platform:       "webgl",
			RunnerType:     build.DefaultRunnerType,
			UnityVersion:   "2022.3.14f1",
			ImageRef:       "unityci/editor:ubuntu-2022.3.14f1-webgl-3",
			TimeoutSeconds: 10,
		},
		Workspace: build.PreparedWorkspace{
			HostSourcePath:       "/host/source",
			HostArtifactRootPath: "/host/artifacts",
		},
	})
	if err == nil {
		t.Fatal("expected missing build method error")
	}
}

type commandRunnerStub struct {
	name string
	args []string
	data []byte
	err  error
}

func (s *commandRunnerStub) Run(
	_ context.Context,
	name string,
	args ...string,
) ([]byte, error) {
	s.name = name
	s.args = append([]string(nil), args...)
	return append([]byte(nil), s.data...), s.err
}