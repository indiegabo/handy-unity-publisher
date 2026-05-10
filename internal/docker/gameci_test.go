package docker

import (
	"context"
	"fmt"
	"os"
	"path/filepath"
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
		"sh -lc",
		"'mkdir' '-p' '/tmp/hgb-home'",
		"exec 'unity-editor' '-batchmode' '-quit' '-nographics'",
		"'-buildTarget' 'StandaloneLinux64'",
		"'-executeMethod' 'Builder.PerformLinux'",
		"HGB_OUTPUT_PATH=/artifacts/Builds/Linux",
		"HOME=/tmp/hgb-home",
		"XDG_CONFIG_HOME=/tmp/hgb-home/.config",
		"XDG_CACHE_HOME=/tmp/hgb-home/.cache",
		"XDG_DATA_HOME=/tmp/hgb-home/.local/share",
		"TMPDIR=/tmp",
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

func TestGameCIExecutorMountsUnityLicenseFileFromHostDataDir(t *testing.T) {
	runtimeDataDir := t.TempDir()
	hostDataDir := t.TempDir()
	runtimeLicensePath := filepath.Join(runtimeDataDir, "licenses", "unity", "Unity_lic.ulf")
	licenseHostPath := filepath.Join(hostDataDir, "licenses", "unity", "Unity_lic.ulf")
	for _, directory := range []string{
		filepath.Dir(runtimeLicensePath),
		filepath.Dir(licenseHostPath),
	} {
		if err := os.MkdirAll(directory, 0o755); err != nil {
			t.Fatalf("create license directory %q: %v", directory, err)
		}
	}
	if err := os.WriteFile(runtimeLicensePath, []byte("ulf-license"), 0o644); err != nil {
		t.Fatalf("write runtime license file: %v", err)
	}
	if err := os.WriteFile(licenseHostPath, []byte("ulf-license"), 0o644); err != nil {
		t.Fatalf("write host license file: %v", err)
	}

	t.Setenv("DATA_DIR", runtimeDataDir)
	t.Setenv("HOST_DATA_DIR", hostDataDir)
	t.Setenv("UNITY_LICENSE_FILE", runtimeLicensePath)
	t.Setenv("UNITY_LICENSE", "legacy-inline-license-should-be-ignored")

	runner := &commandRunnerStub{}
	executor := newGameCIExecutorWithRunner(runner)
	buildMethod := "Builder.PerformWebGL"

	_, err := executor.Execute(context.Background(), build.ExecuteRequest{
		Plan: build.ExecutionPlan{
			BuildRunID:     21,
			ReleaseRunID:   13,
			BuildTargetID:  5,
			Platform:       "webgl",
			RunnerType:     build.DefaultRunnerType,
			BuildMethod:    &buildMethod,
			UnityVersion:   "6000.4.3f1",
			ImageRef:       "unityci/editor:ubuntu-6000.4.3f1-webgl-3",
			TimeoutSeconds: 30,
		},
		Workspace: build.PreparedWorkspace{
			HostSourcePath:       "/host/data/workspaces/build-run-21/source",
			HostArtifactRootPath: "/host/data/artifacts/revolutions.v1.0.0",
		},
	})
	if err != nil {
		t.Fatalf("execute GameCI build with license file: %v", err)
	}

	args := strings.Join(runner.args, " ")
	wantMount := fmt.Sprintf(
		"%s:%s:ro",
		licenseHostPath,
		containerUnityLicenseSourceMountPath,
	)
	if !strings.Contains(args, wantMount) {
		t.Fatalf("expected docker args to contain unity license mount %q, got %q", wantMount, args)
	}
	if !strings.Contains(args, fmt.Sprintf("'cp' '%s' '%s'", containerUnityLicenseSourceMountPath, containerSerialLicenseFilePath)) {
		t.Fatalf("expected bootstrap command to install the ULF license, got %q", args)
	}
	if strings.Contains(args, "UNITY_LICENSE=legacy-inline-license-should-be-ignored") {
		t.Fatalf("expected inline UNITY_LICENSE to be skipped when UNITY_LICENSE_FILE is configured, got %q", args)
	}
}

func TestGameCIExecutorMountsNamedUserLicenseXML(t *testing.T) {
	runtimeDataDir := t.TempDir()
	hostDataDir := t.TempDir()
	runtimeLicensePath := filepath.Join(
		runtimeDataDir,
		"licenses",
		"unity",
		"UnityEntitlementLicense.xml",
	)
	licenseHostPath := filepath.Join(
		hostDataDir,
		"licenses",
		"unity",
		"UnityEntitlementLicense.xml",
	)
	for _, directory := range []string{
		filepath.Dir(runtimeLicensePath),
		filepath.Dir(licenseHostPath),
	} {
		if err := os.MkdirAll(directory, 0o755); err != nil {
			t.Fatalf("create license directory %q: %v", directory, err)
		}
	}
	if err := os.WriteFile(runtimeLicensePath, []byte("xml-license"), 0o644); err != nil {
		t.Fatalf("write runtime license file: %v", err)
	}
	if err := os.WriteFile(licenseHostPath, []byte("xml-license"), 0o644); err != nil {
		t.Fatalf("write host license file: %v", err)
	}

	t.Setenv("DATA_DIR", runtimeDataDir)
	t.Setenv("HOST_DATA_DIR", hostDataDir)
	t.Setenv("UNITY_LICENSE_FILE", runtimeLicensePath)
	t.Setenv("UNITY_LICENSE", "legacy-inline-license-should-be-ignored")

	runner := &commandRunnerStub{}
	executor := newGameCIExecutorWithRunner(runner)
	buildMethod := "Builder.PerformWebGL"

	_, err := executor.Execute(context.Background(), build.ExecuteRequest{
		Plan: build.ExecutionPlan{
			BuildRunID:     34,
			ReleaseRunID:   21,
			BuildTargetID:  8,
			Platform:       "webgl",
			RunnerType:     build.DefaultRunnerType,
			BuildMethod:    &buildMethod,
			UnityVersion:   "6000.4.3f1",
			ImageRef:       "unityci/editor:ubuntu-6000.4.3f1-webgl-3",
			TimeoutSeconds: 30,
		},
		Workspace: build.PreparedWorkspace{
			HostSourcePath:       "/host/data/workspaces/build-run-34/source",
			HostArtifactRootPath: "/host/data/artifacts/revolutions.v1.0.0",
		},
	})
	if err != nil {
		t.Fatalf("execute GameCI build with named user license: %v", err)
	}

	args := strings.Join(runner.args, " ")
	wantMount := fmt.Sprintf(
		"%s:%s:ro",
		licenseHostPath,
		containerUnityLicenseSourceMountPath,
	)
	if !strings.Contains(args, wantMount) {
		t.Fatalf("expected docker args to contain named user license mount %q, got %q", wantMount, args)
	}
	if !strings.Contains(args, fmt.Sprintf("'cp' '%s' '%s'", containerUnityLicenseSourceMountPath, containerNamedUserLicenseFilePath)) {
		t.Fatalf("expected bootstrap command to install the named user license XML, got %q", args)
	}
	if strings.Contains(args, "UNITY_LICENSE=legacy-inline-license-should-be-ignored") {
		t.Fatalf("expected inline UNITY_LICENSE to be skipped when UNITY_LICENSE_FILE is configured, got %q", args)
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
