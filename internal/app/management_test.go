package app

import (
	"bytes"
	"net/http"
	"net/http/httptest"
	"testing"

	"github.com/indiegabo/handy-unity-bulder/internal/build"
	"github.com/indiegabo/handy-unity-bulder/internal/credentials"
	"github.com/indiegabo/handy-unity-bulder/internal/publish"
	"github.com/indiegabo/handy-unity-bulder/internal/repository"
	"github.com/indiegabo/handy-unity-bulder/internal/trigger"
)

func TestCredentialsManagementEndpoints(t *testing.T) {
	t.Parallel()

	server, cleanup := newRepositoryTestServer(t)
	defer cleanup()

	created := performJSONRequest[credentials.Record](
		t,
		server,
		http.MethodPost,
		"/api/v1/credentials",
		map[string]any{
			"name":        "github-basic",
			"kind":        credentials.KindGitHTTPBasic,
			"config_json": `{"username":"git","password":"token"}`,
		},
		http.StatusCreated,
	)
	if created.ID == 0 {
		t.Fatalf("expected created credentials id to be set")
	}

	loaded := performJSONRequest[credentials.Record](
		t,
		server,
		http.MethodGet,
		"/api/v1/credentials/1",
		nil,
		http.StatusOK,
	)
	if loaded.Name != created.Name {
		t.Fatalf("expected loaded credentials name %q, got %q", created.Name, loaded.Name)
	}

	updated := performJSONRequest[credentials.Record](
		t,
		server,
		http.MethodPut,
		"/api/v1/credentials/1",
		map[string]any{
			"name":        "github-basic-stable",
			"kind":        credentials.KindGitHTTPBasic,
			"config_json": `{"username":"git","password":"token-v2"}`,
		},
		http.StatusOK,
	)
	if updated.Name != "github-basic-stable" {
		t.Fatalf("expected updated credentials name, got %q", updated.Name)
	}

	list := performJSONRequest[[]credentials.Record](
		t,
		server,
		http.MethodGet,
		"/api/v1/credentials",
		nil,
		http.StatusOK,
	)
	if len(list) != 1 {
		t.Fatalf("expected one credentials record, got %d", len(list))
	}
}

func TestBuildTargetManagementEndpoints(t *testing.T) {
	t.Parallel()

	server, cleanup := newRepositoryTestServer(t)
	defer cleanup()

	repo := performJSONRequest[repository.Record](
		t,
		server,
		http.MethodPost,
		"/api/v1/repositories",
		map[string]any{
			"name":                     "build-http-repo",
			"repo_url":                 "https://example.com/org/build-http.git",
			"polling_interval_seconds": 180,
		},
		http.StatusCreated,
	)

	created := performJSONRequest[build.Target](
		t,
		server,
		http.MethodPost,
		"/api/v1/build-targets",
		map[string]any{
			"repository_id": repo.ID,
			"name":          "linux-player",
			"platform":      "linux",
		},
		http.StatusCreated,
	)

	if created.ID == 0 {
		t.Fatalf("expected created build target id to be set")
	}

	loaded := performJSONRequest[build.Target](
		t,
		server,
		http.MethodGet,
		"/api/v1/build-targets/1",
		nil,
		http.StatusOK,
	)
	if loaded.Name != "linux-player" {
		t.Fatalf("expected build target name %q, got %q", "linux-player", loaded.Name)
	}

	updated := performJSONRequest[build.Target](
		t,
		server,
		http.MethodPut,
		"/api/v1/build-targets/1",
		map[string]any{
			"name":            "linux-player-stable",
			"platform":        "linux",
			"enabled":         false,
			"runner_type":     "gameci",
			"timeout_seconds": 3600,
		},
		http.StatusOK,
	)
	if updated.Enabled {
		t.Fatalf("expected updated build target to be disabled")
	}

	list := performJSONRequest[[]build.Target](
		t,
		server,
		http.MethodGet,
		"/api/v1/build-targets?repository_id=1",
		nil,
		http.StatusOK,
	)
	if len(list) != 1 {
		t.Fatalf("expected one build target, got %d", len(list))
	}

	performJSONRequest[map[string]any](
		t,
		server,
		http.MethodDelete,
		"/api/v1/build-targets/1",
		nil,
		http.StatusOK,
	)
}

func TestTriggerRuleManagementEndpoints(t *testing.T) {
	t.Parallel()

	server, cleanup := newRepositoryTestServer(t)
	defer cleanup()

	repo := performJSONRequest[repository.Record](
		t,
		server,
		http.MethodPost,
		"/api/v1/repositories",
		map[string]any{
			"name":                     "trigger-http-repo",
			"repo_url":                 "https://example.com/org/trigger-http.git",
			"polling_interval_seconds": 180,
		},
		http.StatusCreated,
	)

	created := performJSONRequest[trigger.Rule](
		t,
		server,
		http.MethodPost,
		"/api/v1/trigger-rules",
		map[string]any{
			"repository_id": repo.ID,
			"name":          "poll-main",
			"source":        "poll",
		},
		http.StatusCreated,
	)
	if created.ID == 0 {
		t.Fatalf("expected created trigger rule id to be set")
	}

	updated := performJSONRequest[trigger.Rule](
		t,
		server,
		http.MethodPut,
		"/api/v1/trigger-rules/1",
		map[string]any{
			"name":    "poll-stable",
			"source":  "poll",
			"enabled": false,
		},
		http.StatusOK,
	)
	if updated.Enabled {
		t.Fatalf("expected updated trigger rule to be disabled")
	}

	list := performJSONRequest[[]trigger.Rule](
		t,
		server,
		http.MethodGet,
		"/api/v1/trigger-rules?repository_id=1",
		nil,
		http.StatusOK,
	)
	if len(list) != 1 {
		t.Fatalf("expected one trigger rule, got %d", len(list))
	}
}

func TestPublishManagementEndpoints(t *testing.T) {
	t.Parallel()

	server, cleanup := newRepositoryTestServer(t)
	defer cleanup()

	repo := performJSONRequest[repository.Record](
		t,
		server,
		http.MethodPost,
		"/api/v1/repositories",
		map[string]any{
			"name":                     "publish-http-repo",
			"repo_url":                 "https://example.com/org/publish-http.git",
			"polling_interval_seconds": 180,
		},
		http.StatusCreated,
	)

	buildTarget := performJSONRequest[build.Target](
		t,
		server,
		http.MethodPost,
		"/api/v1/build-targets",
		map[string]any{
			"repository_id": repo.ID,
			"name":          "linux-player",
			"platform":      "linux",
		},
		http.StatusCreated,
	)

	publishTarget := performJSONRequest[publish.Target](
		t,
		server,
		http.MethodPost,
		"/api/v1/publish-targets",
		map[string]any{
			"repository_id": repo.ID,
			"name":          "filesystem-default",
			"kind":          publish.KindFilesystem,
			"config_json":   `{"root_path":"/exports"}`,
		},
		http.StatusCreated,
	)
	if publishTarget.ID == 0 {
		t.Fatalf("expected created publish target id to be set")
	}

	updatedTarget := performJSONRequest[publish.Target](
		t,
		server,
		http.MethodPut,
		"/api/v1/publish-targets/1",
		map[string]any{
			"name":        "filesystem-stable",
			"kind":        publish.KindFilesystem,
			"enabled":     false,
			"config_json": `{"root_path":"/exports/stable"}`,
		},
		http.StatusOK,
	)
	if updatedTarget.Enabled {
		t.Fatalf("expected updated publish target to be disabled")
	}

	targetList := performJSONRequest[[]publish.Target](
		t,
		server,
		http.MethodGet,
		"/api/v1/publish-targets?repository_id=1",
		nil,
		http.StatusOK,
	)
	if len(targetList) != 1 {
		t.Fatalf("expected one publish target, got %d", len(targetList))
	}

	binding := performJSONRequest[publish.Binding](
		t,
		server,
		http.MethodPost,
		"/api/v1/build-publish-bindings",
		map[string]any{
			"build_target_id":   buildTarget.ID,
			"publish_target_id": publishTarget.ID,
			"options_json":      `{"channel":"stable"}`,
		},
		http.StatusCreated,
	)
	if binding.ID == 0 {
		t.Fatalf("expected created binding id to be set")
	}

	updatedBinding := performJSONRequest[publish.Binding](
		t,
		server,
		http.MethodPut,
		"/api/v1/build-publish-bindings/1",
		map[string]any{
			"enabled":      false,
			"options_json": `{"channel":"stable","compression":"zip"}`,
		},
		http.StatusOK,
	)
	if updatedBinding.Enabled {
		t.Fatalf("expected updated build publish binding to be disabled")
	}

	bindingList := performJSONRequest[[]publish.Binding](
		t,
		server,
		http.MethodGet,
		"/api/v1/build-publish-bindings?build_target_id=1",
		nil,
		http.StatusOK,
	)
	if len(bindingList) != 1 {
		t.Fatalf("expected one publish binding, got %d", len(bindingList))
	}
}

func TestDatabaseAdminEndpoints(t *testing.T) {
	t.Parallel()

	server, cleanup := newRepositoryTestServer(t)
	defer cleanup()

	performJSONRequest[repository.Record](
		t,
		server,
		http.MethodPost,
		"/api/v1/repositories",
		map[string]any{
			"name":                     "database-admin-repo",
			"repo_url":                 "https://example.com/org/database-admin.git",
			"polling_interval_seconds": 300,
		},
		http.StatusCreated,
	)

	exportRequest := httptest.NewRequest(
		http.MethodGet,
		"/api/v1/runtime/database/export",
		nil,
	)
	exportRecorder := httptest.NewRecorder()
	server.httpServer.Handler.ServeHTTP(exportRecorder, exportRequest)

	if exportRecorder.Code != http.StatusOK {
		t.Fatalf("expected database export status %d, got %d", http.StatusOK, exportRecorder.Code)
	}

	snapshotBytes := exportRecorder.Body.Bytes()
	if len(snapshotBytes) == 0 {
		t.Fatal("expected database export snapshot to contain bytes")
	}

	request := httptest.NewRequest(
		http.MethodPost,
		"/api/v1/runtime/database/import",
		bytes.NewReader(snapshotBytes),
	)
	request.Header.Set("Content-Type", "application/octet-stream")

	importRecorder := httptest.NewRecorder()
	server.httpServer.Handler.ServeHTTP(importRecorder, request)

	if importRecorder.Code != http.StatusAccepted {
		t.Fatalf(
			"expected database import status %d, got %d: %s",
			http.StatusAccepted,
			importRecorder.Code,
			importRecorder.Body.String(),
		)
	}
}
