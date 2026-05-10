package pipelines

import (
	"context"
	"database/sql"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/indiegabo/handy-unity-bulder/internal/build"
	"github.com/indiegabo/handy-unity-bulder/internal/config"
	"github.com/indiegabo/handy-unity-bulder/internal/credentials"
	"github.com/indiegabo/handy-unity-bulder/internal/db"
	"github.com/indiegabo/handy-unity-bulder/internal/publish"
	"github.com/indiegabo/handy-unity-bulder/internal/repository"
)

func TestLoadDirAndApplySynchronizesPipelineManifest(t *testing.T) {
	t.Setenv("REVOLUTIONS_GIT_USERNAME", "git")
	t.Setenv("REVOLUTIONS_GIT_TOKEN", "secret-token")

	pipelinesDir := t.TempDir()
	manifestPath := filepath.Join(pipelinesDir, "revolutions.yml")
	writeTestManifest(t, manifestPath, strings.Join([]string{
		"apiVersion: handy.unity.builder/v1alpha1",
		"kind: Pipeline",
		"metadata:",
		"  name: revolutions",
		"spec:",
		"  repository:",
		"    url: https://example.com/org/revolutions.git",
		"    defaultBranch: main",
		"    pollingIntervalSeconds: 300",
		"    credentials: origin",
		"  credentials:",
		"    - name: origin",
		"      kind: git-http-basic",
		"      basic:",
		"        username:",
		"          env: REVOLUTIONS_GIT_USERNAME",
		"        password:",
		"          env: REVOLUTIONS_GIT_TOKEN",
		"  build:",
		"    targets:",
		"      - name: linux64",
		"        platform: StandaloneLinux64",
		"        buildMethod: Builder.BuildLinux64",
		"        runner:",
		"          unityVersion: 2022.3.14f1",
		"          timeoutSeconds: 5400",
		"        output:",
		"          kind: archive",
		"          path: Builds/Linux64",
		"        config:",
		"          compression: zip",
		"  publish:",
		"    targets:",
		"      - name: filesystem-release",
		"        kind: filesystem",
		"        config:",
		"          root_path: /exports/releases",
		"  bindings:",
		"    - buildTarget: linux64",
		"      publishTarget: filesystem-release",
		"      options:",
		"        channel: stable",
	}, "\n"))

	loadResult, err := LoadDir(pipelinesDir)
	if err != nil {
		t.Fatalf("LoadDir() error = %v", err)
	}
	if len(loadResult.Issues) != 0 {
		t.Fatalf("LoadDir() issues = %#v", loadResult.Issues)
	}
	if len(loadResult.Manifests) != 1 {
		t.Fatalf("LoadDir() manifests = %d", len(loadResult.Manifests))
	}

	database := newTestDatabase(t)
	synchronizer := NewSynchronizer(
		credentials.NewStore(database),
		repository.NewStore(database),
		build.NewStore(database),
		publish.NewStore(database),
	)
	report, err := synchronizer.Apply(context.Background(), loadResult.Manifests, loadResult.Issues)
	if err != nil {
		t.Fatalf("Apply() error = %v", err)
	}
	if len(report.Pipelines) != 1 || !report.Pipelines[0].Applied {
		t.Fatalf("Apply() report = %#v", report.Pipelines)
	}

	credentialRecords, err := credentials.NewStore(database).List(context.Background())
	if err != nil {
		t.Fatalf("credentials.List() error = %v", err)
	}
	if len(credentialRecords) != 1 {
		t.Fatalf("credentials count = %d", len(credentialRecords))
	}
	if credentialRecords[0].Name != "revolutions/origin" {
		t.Fatalf("credential name = %q", credentialRecords[0].Name)
	}
	if credentialRecords[0].ConfigJSON != `{"password":"secret-token","username":"git"}` {
		t.Fatalf("credential config = %q", credentialRecords[0].ConfigJSON)
	}

	repositories, err := repository.NewStore(database).List(context.Background())
	if err != nil {
		t.Fatalf("repositories.List() error = %v", err)
	}
	if len(repositories) != 1 {
		t.Fatalf("repository count = %d", len(repositories))
	}
	if repositories[0].Name != "revolutions" {
		t.Fatalf("repository name = %q", repositories[0].Name)
	}
	if repositories[0].CredentialsID == nil || *repositories[0].CredentialsID != credentialRecords[0].ID {
		t.Fatalf("repository credentials_id = %#v", repositories[0].CredentialsID)
	}

	buildTargets, err := build.NewStore(database).ListTargetsByRepository(context.Background(), repositories[0].ID)
	if err != nil {
		t.Fatalf("build.ListTargetsByRepository() error = %v", err)
	}
	if len(buildTargets) != 1 {
		t.Fatalf("build target count = %d", len(buildTargets))
	}
	if buildTargets[0].ConfigJSON != `{"compression":"zip"}` {
		t.Fatalf("build config = %q", buildTargets[0].ConfigJSON)
	}

	publishTargets, err := publish.NewStore(database).ListTargetsByRepository(context.Background(), repositories[0].ID)
	if err != nil {
		t.Fatalf("publish.ListTargetsByRepository() error = %v", err)
	}
	if len(publishTargets) != 1 {
		t.Fatalf("publish target count = %d", len(publishTargets))
	}
	if publishTargets[0].ConfigJSON != `{"root_path":"/exports/releases"}` {
		t.Fatalf("publish config = %q", publishTargets[0].ConfigJSON)
	}

	bindings, err := publish.NewStore(database).ListBindingsByBuildTarget(context.Background(), buildTargets[0].ID)
	if err != nil {
		t.Fatalf("publish.ListBindingsByBuildTarget() error = %v", err)
	}
	if len(bindings) != 1 {
		t.Fatalf("binding count = %d", len(bindings))
	}
	if bindings[0].OptionsJSON != `{"channel":"stable"}` {
		t.Fatalf("binding options = %q", bindings[0].OptionsJSON)
	}
}

func TestApplyDisablesRemovedRepositoryAndTarget(t *testing.T) {
	pipelinesDir := t.TempDir()
	manifestPath := filepath.Join(pipelinesDir, "alpha.yml")
	writeTestManifest(t, manifestPath, strings.Join([]string{
		"apiVersion: handy.unity.builder/v1alpha1",
		"kind: Pipeline",
		"metadata:",
		"  name: alpha",
		"spec:",
		"  repository:",
		"    url: https://example.com/org/alpha.git",
		"    pollingIntervalSeconds: 300",
		"  build:",
		"    targets:",
		"      - name: linux64",
		"        platform: StandaloneLinux64",
		"        buildMethod: Builder.BuildLinux64",
		"        output:",
		"          kind: archive",
		"          path: Builds/Linux64",
		"  publish:",
		"    targets: []",
		"  bindings: []",
	}, "\n"))

	database := newTestDatabase(t)
	synchronizer := NewSynchronizer(
		credentials.NewStore(database),
		repository.NewStore(database),
		build.NewStore(database),
		publish.NewStore(database),
	)

	loadResult, err := LoadDir(pipelinesDir)
	if err != nil {
		t.Fatalf("LoadDir() error = %v", err)
	}
	if _, err := synchronizer.Apply(context.Background(), loadResult.Manifests, loadResult.Issues); err != nil {
		t.Fatalf("first Apply() error = %v", err)
	}

	writeTestManifest(t, manifestPath, `apiVersion: handy.unity.builder/v1alpha1
kind: Pipeline
metadata:
  name: alpha
spec:
  repository:
    url: https://example.com/org/alpha.git
    enabled: false
    pollingIntervalSeconds: 300
  build:
    targets: []
  publish:
    targets: []
  bindings: []
`)

	loadResult, err = LoadDir(pipelinesDir)
	if err != nil {
		t.Fatalf("LoadDir() second error = %v", err)
	}
	if _, err := synchronizer.Apply(context.Background(), loadResult.Manifests, loadResult.Issues); err != nil {
		t.Fatalf("second Apply() error = %v", err)
	}

	repositories, err := repository.NewStore(database).List(context.Background())
	if err != nil {
		t.Fatalf("repositories.List() error = %v", err)
	}
	if len(repositories) != 1 || repositories[0].Enabled {
		t.Fatalf("repositories = %#v", repositories)
	}

	buildTargets, err := build.NewStore(database).ListTargetsByRepository(context.Background(), repositories[0].ID)
	if err != nil {
		t.Fatalf("build.ListTargetsByRepository() error = %v", err)
	}
	if len(buildTargets) != 1 || buildTargets[0].Enabled {
		t.Fatalf("build targets = %#v", buildTargets)
	}
}

func TestLoadDirReportsIssueForArchiveOutputPathWithZipSuffix(t *testing.T) {
	t.Setenv("REVOLUTIONS_GIT_USERNAME", "git")
	t.Setenv("REVOLUTIONS_GIT_TOKEN", "secret-token")

	pipelinesDir := t.TempDir()
	manifestPath := filepath.Join(pipelinesDir, "revolutions.yml")
	writeTestManifest(t, manifestPath, `apiVersion: handy.unity.builder/v1alpha1
kind: Pipeline
metadata:
  name: revolutions
spec:
  repository:
    url: https://example.com/org/revolutions.git
  credentials:
    - name: origin
      kind: git-http-basic
      basic:
        username:
          env: REVOLUTIONS_GIT_USERNAME
        password:
          env: REVOLUTIONS_GIT_TOKEN
  build:
    targets:
      - name: webgl
        platform: WebGL
        buildMethod: Builder.PerformWebGL
        output:
          kind: archive
          path: Builds/WebGL.zip
  publish:
    targets: []
  bindings: []
`)

	loadResult, err := LoadDir(pipelinesDir)
	if err != nil {
		t.Fatalf("LoadDir() error = %v", err)
	}
	if len(loadResult.Manifests) != 0 {
		t.Fatalf("expected no valid manifests, got %d", len(loadResult.Manifests))
	}
	if len(loadResult.Issues) != 1 {
		t.Fatalf("expected one load issue, got %#v", loadResult.Issues)
	}
	if !strings.Contains(loadResult.Issues[0].Error, "remove the .zip suffix") {
		t.Fatalf("expected archive zip suffix guidance, got %q", loadResult.Issues[0].Error)
	}
}

func newTestDatabase(t *testing.T) *sql.DB {
	t.Helper()

	dataDir := t.TempDir()
	cfg := config.Config{
		DataDir:      dataDir,
		DatabasePath: filepath.Join(dataDir, "test.db"),
	}

	database, err := db.Open(context.Background(), cfg)
	if err != nil {
		t.Fatalf("db.Open() error = %v", err)
	}

	t.Cleanup(func() {
		if err := database.Close(); err != nil {
			t.Fatalf("database.Close() error = %v", err)
		}
	})

	return database
}

func writeTestManifest(t *testing.T, path string, contents string) {
	t.Helper()

	if err := os.WriteFile(path, []byte(contents), 0o644); err != nil {
		t.Fatalf("os.WriteFile(%q) error = %v", path, err)
	}
}
