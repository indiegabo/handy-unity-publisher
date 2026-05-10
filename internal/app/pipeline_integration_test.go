package app

import (
	"fmt"
	"net/http"
	"testing"

	"github.com/indiegabo/handy-unity-bulder/internal/build"
	"github.com/indiegabo/handy-unity-bulder/internal/credentials"
	"github.com/indiegabo/handy-unity-bulder/internal/publish"
	"github.com/indiegabo/handy-unity-bulder/internal/repository"
	"github.com/indiegabo/handy-unity-bulder/internal/trigger"
)

func TestPipelineCRUDIntegrationFlow(t *testing.T) {
	t.Parallel()

	server, cleanup := newRepositoryTestServer(t)
	defer cleanup()

	createdCredentials := performJSONRequest[credentials.Record](
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

	updatedCredentials := performJSONRequest[credentials.Record](
		t,
		server,
		http.MethodPut,
		fmt.Sprintf("/api/v1/credentials/%d", createdCredentials.ID),
		map[string]any{
			"name":        "github-basic-stable",
			"kind":        credentials.KindGitHTTPBasic,
			"config_json": `{"username":"git","password":"token-v2"}`,
		},
		http.StatusOK,
	)

	createdRepository := performJSONRequest[repository.Record](
		t,
		server,
		http.MethodPost,
		"/api/v1/repositories",
		map[string]any{
			"name":                     "pipeline-alpha",
			"repo_url":                 "https://example.com/org/pipeline-alpha.git",
			"credentials_id":           updatedCredentials.ID,
			"default_branch":           "main",
			"polling_interval_seconds": 300,
			"enabled":                  true,
		},
		http.StatusCreated,
	)
	if createdRepository.CredentialsID == nil || *createdRepository.CredentialsID != updatedCredentials.ID {
		t.Fatalf("expected repository credentials id %d, got %#v", updatedCredentials.ID, createdRepository.CredentialsID)
	}

	updatedRepository := performJSONRequest[repository.Record](
		t,
		server,
		http.MethodPut,
		fmt.Sprintf("/api/v1/repositories/%d", createdRepository.ID),
		map[string]any{
			"name":                     "pipeline-alpha-stable",
			"repo_url":                 createdRepository.RepoURL,
			"credentials_id":           updatedCredentials.ID,
			"default_branch":           "release",
			"polling_interval_seconds": 600,
			"enabled":                  true,
		},
		http.StatusOK,
	)
	if updatedRepository.DefaultBranch == nil || *updatedRepository.DefaultBranch != "release" {
		t.Fatalf("expected updated default branch %q, got %#v", "release", updatedRepository.DefaultBranch)
	}

	createdBuildTarget := performJSONRequest[build.Target](
		t,
		server,
		http.MethodPost,
		"/api/v1/build-targets",
		map[string]any{
			"repository_id":        updatedRepository.ID,
			"name":                 "linux-player",
			"platform":             "linux",
			"build_method":         "Builder.PerformLinux",
			"output_kind":          "archive",
			"output_path_template": "Builds/linux-player",
			"timeout_seconds":      3600,
			"enabled":              true,
		},
		http.StatusCreated,
	)

	updatedBuildTarget := performJSONRequest[build.Target](
		t,
		server,
		http.MethodPut,
		fmt.Sprintf("/api/v1/build-targets/%d", createdBuildTarget.ID),
		map[string]any{
			"name":                 "linux-player-release",
			"platform":             "linux",
			"runner_type":          build.DefaultRunnerType,
			"build_method":         "Builder.PerformLinuxRelease",
			"output_kind":          "archive",
			"output_path_template": "Builds/linux-player-release",
			"timeout_seconds":      5400,
			"enabled":              true,
			"config_json":          `{"channel":"stable"}`,
		},
		http.StatusOK,
	)
	if updatedBuildTarget.TimeoutSeconds != 5400 {
		t.Fatalf("expected updated build target timeout 5400, got %d", updatedBuildTarget.TimeoutSeconds)
	}

	createdTrigger := performJSONRequest[trigger.Rule](
		t,
		server,
		http.MethodPost,
		"/api/v1/trigger-rules",
		map[string]any{
			"repository_id": updatedRepository.ID,
			"name":          "default-poll",
			"source":        trigger.SourcePoll,
			"enabled":       true,
			"config_json":   `{"interval_seconds":600}`,
		},
		http.StatusCreated,
	)

	updatedTrigger := performJSONRequest[trigger.Rule](
		t,
		server,
		http.MethodPut,
		fmt.Sprintf("/api/v1/trigger-rules/%d", createdTrigger.ID),
		map[string]any{
			"name":        "default-poll-stable",
			"source":      trigger.SourcePoll,
			"enabled":     false,
			"config_json": `{"interval_seconds":900}`,
		},
		http.StatusOK,
	)
	if updatedTrigger.Enabled {
		t.Fatal("expected updated trigger rule to be disabled")
	}

	createdPublishTarget := performJSONRequest[publish.Target](
		t,
		server,
		http.MethodPost,
		"/api/v1/publish-targets",
		map[string]any{
			"repository_id": updatedRepository.ID,
			"name":          "filesystem-release",
			"kind":          publish.KindFilesystem,
			"enabled":       true,
			"config_json":   `{"root_path":"/exports"}`,
		},
		http.StatusCreated,
	)

	updatedPublishTarget := performJSONRequest[publish.Target](
		t,
		server,
		http.MethodPut,
		fmt.Sprintf("/api/v1/publish-targets/%d", createdPublishTarget.ID),
		map[string]any{
			"name":        "filesystem-release-stable",
			"kind":        publish.KindFilesystem,
			"enabled":     true,
			"config_json": `{"root_path":"/exports/stable","channel":"stable"}`,
		},
		http.StatusOK,
	)
	if updatedPublishTarget.Name != "filesystem-release-stable" {
		t.Fatalf("expected updated publish target name %q, got %q", "filesystem-release-stable", updatedPublishTarget.Name)
	}

	createdBinding := performJSONRequest[publish.Binding](
		t,
		server,
		http.MethodPost,
		"/api/v1/build-publish-bindings",
		map[string]any{
			"build_target_id":   updatedBuildTarget.ID,
			"publish_target_id": updatedPublishTarget.ID,
			"enabled":           true,
			"options_json":      `{"subdir":"stable"}`,
		},
		http.StatusCreated,
	)

	updatedBinding := performJSONRequest[publish.Binding](
		t,
		server,
		http.MethodPut,
		fmt.Sprintf("/api/v1/build-publish-bindings/%d", createdBinding.ID),
		map[string]any{
			"enabled":      false,
			"options_json": `{"subdir":"stable","mode":"atomic"}`,
		},
		http.StatusOK,
	)
	if updatedBinding.Enabled {
		t.Fatal("expected updated binding to be disabled")
	}

	credentialsList := performJSONRequest[[]credentials.Record](
		t,
		server,
		http.MethodGet,
		"/api/v1/credentials",
		nil,
		http.StatusOK,
	)
	repositoriesList := performJSONRequest[[]repository.Record](
		t,
		server,
		http.MethodGet,
		"/api/v1/repositories",
		nil,
		http.StatusOK,
	)
	buildTargetList := performJSONRequest[[]build.Target](
		t,
		server,
		http.MethodGet,
		fmt.Sprintf("/api/v1/build-targets?repository_id=%d", updatedRepository.ID),
		nil,
		http.StatusOK,
	)
	triggerList := performJSONRequest[[]trigger.Rule](
		t,
		server,
		http.MethodGet,
		fmt.Sprintf("/api/v1/trigger-rules?repository_id=%d", updatedRepository.ID),
		nil,
		http.StatusOK,
	)
	publishTargetList := performJSONRequest[[]publish.Target](
		t,
		server,
		http.MethodGet,
		fmt.Sprintf("/api/v1/publish-targets?repository_id=%d", updatedRepository.ID),
		nil,
		http.StatusOK,
	)
	bindingList := performJSONRequest[[]publish.Binding](
		t,
		server,
		http.MethodGet,
		fmt.Sprintf("/api/v1/build-publish-bindings?build_target_id=%d", updatedBuildTarget.ID),
		nil,
		http.StatusOK,
	)

	if len(credentialsList) != 1 || len(repositoriesList) != 1 || len(buildTargetList) != 1 || len(triggerList) != 1 || len(publishTargetList) != 1 || len(bindingList) != 1 {
		t.Fatalf(
			"expected one record per pipeline list, got credentials=%d repositories=%d build_targets=%d trigger_rules=%d publish_targets=%d bindings=%d",
			len(credentialsList),
			len(repositoriesList),
			len(buildTargetList),
			len(triggerList),
			len(publishTargetList),
			len(bindingList),
		)
	}

	performJSONRequest[map[string]any](
		t,
		server,
		http.MethodDelete,
		fmt.Sprintf("/api/v1/build-publish-bindings/%d", updatedBinding.ID),
		nil,
		http.StatusOK,
	)
	performJSONRequest[map[string]any](
		t,
		server,
		http.MethodDelete,
		fmt.Sprintf("/api/v1/trigger-rules/%d", updatedTrigger.ID),
		nil,
		http.StatusOK,
	)
	performJSONRequest[map[string]any](
		t,
		server,
		http.MethodDelete,
		fmt.Sprintf("/api/v1/publish-targets/%d", updatedPublishTarget.ID),
		nil,
		http.StatusOK,
	)
	performJSONRequest[map[string]any](
		t,
		server,
		http.MethodDelete,
		fmt.Sprintf("/api/v1/build-targets/%d", updatedBuildTarget.ID),
		nil,
		http.StatusOK,
	)
	performJSONRequest[map[string]any](
		t,
		server,
		http.MethodDelete,
		fmt.Sprintf("/api/v1/repositories/%d", updatedRepository.ID),
		nil,
		http.StatusOK,
	)
	performJSONRequest[map[string]any](
		t,
		server,
		http.MethodDelete,
		fmt.Sprintf("/api/v1/credentials/%d", updatedCredentials.ID),
		nil,
		http.StatusOK,
	)

	if remainingCredentials := performJSONRequest[[]credentials.Record](
		t,
		server,
		http.MethodGet,
		"/api/v1/credentials",
		nil,
		http.StatusOK,
	); len(remainingCredentials) != 0 {
		t.Fatalf("expected no credentials after delete, got %d", len(remainingCredentials))
	}

	if remainingRepositories := performJSONRequest[[]repository.Record](
		t,
		server,
		http.MethodGet,
		"/api/v1/repositories",
		nil,
		http.StatusOK,
	); len(remainingRepositories) != 0 {
		t.Fatalf("expected no repositories after delete, got %d", len(remainingRepositories))
	}
}