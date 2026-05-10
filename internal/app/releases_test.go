package app

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/indiegabo/handy-unity-bulder/internal/release"
	"github.com/indiegabo/handy-unity-bulder/internal/repository"
)

func TestHandleManualReleaseDispatch(t *testing.T) {
	server, cleanup := newRepositoryTestServer(t)
	defer cleanup()

	repoStore := server.repos
	enabled := true
	repoRecord, err := repoStore.Create(context.Background(), repository.CreateInput{
		Name:                   "revolutions",
		RepoURL:                "https://example.com/revolutions.git",
		PollingIntervalSeconds: 300,
		Enabled:                &enabled,
	})
	if err != nil {
		t.Fatalf("create repository: %v", err)
	}

	body, err := json.Marshal(manualReleaseDispatchRequest{
		RepositoryID: repoRecord.ID,
		GitTag:       "v1.2.3",
	})
	if err != nil {
		t.Fatalf("marshal request: %v", err)
	}

	request := httptest.NewRequest(
		http.MethodPost,
		"/api/v1/releases/dispatch/manual",
		bytes.NewReader(body),
	)
	response := httptest.NewRecorder()

	server.handleManualReleaseDispatch(response, request)

	if response.Code != http.StatusCreated {
		t.Fatalf("expected status 201, got %d: %s", response.Code, response.Body.String())
	}

	var record release.Record
	if err := json.Unmarshal(response.Body.Bytes(), &record); err != nil {
		t.Fatalf("decode response: %v", err)
	}
	if record.RepositoryID != repoRecord.ID {
		t.Fatalf("expected repository id %d, got %d", repoRecord.ID, record.RepositoryID)
	}
	if record.GitTag != "v1.2.3" {
		t.Fatalf("expected git tag v1.2.3, got %q", record.GitTag)
	}
	if record.TriggerSource != release.TriggerSourceManual {
		t.Fatalf("expected manual trigger source, got %q", record.TriggerSource)
	}
}

func TestHandleManualReleaseDispatchRebuildReusesExistingRelease(t *testing.T) {
	server, cleanup := newRepositoryTestServer(t)
	defer cleanup()

	repoRecord, err := server.repos.Create(context.Background(), repository.CreateInput{
		Name:                   "revolutions",
		RepoURL:                "https://example.com/revolutions.git",
		PollingIntervalSeconds: 300,
	})
	if err != nil {
		t.Fatalf("create repository: %v", err)
	}

	first := performJSONRequest[release.Record](
		t,
		server,
		http.MethodPost,
		"/api/v1/releases/dispatch/manual",
		map[string]any{
			"repository_id": repoRecord.ID,
			"git_tag":       "v1.2.3",
		},
		http.StatusCreated,
	)

	rebuilt := performJSONRequest[release.Record](
		t,
		server,
		http.MethodPost,
		"/api/v1/releases/dispatch/manual",
		map[string]any{
			"repository_id": repoRecord.ID,
			"git_tag":       "v1.2.3",
			"git_commit":    "feedface",
			"rebuild":       true,
		},
		http.StatusCreated,
	)

	if rebuilt.ID != first.ID {
		t.Fatalf("expected rebuild to reuse release id %d, got %d", first.ID, rebuilt.ID)
	}
	if rebuilt.GitCommit == nil || *rebuilt.GitCommit != "feedface" {
		t.Fatalf("expected updated git commit, got %#v", rebuilt.GitCommit)
	}
	if rebuilt.Status != release.StatusQueued {
		t.Fatalf("expected queued rebuilt release, got %q", rebuilt.Status)
	}
}

func TestHandleManualReleaseDispatchRejectsInvalidBody(t *testing.T) {
	server, cleanup := newRepositoryTestServer(t)
	defer cleanup()

	request := httptest.NewRequest(
		http.MethodPost,
		"/api/v1/releases/dispatch/manual",
		bytes.NewBufferString("{not-json}"),
	)
	response := httptest.NewRecorder()

	server.handleManualReleaseDispatch(response, request)

	if response.Code != http.StatusBadRequest {
		t.Fatalf("expected status 400, got %d", response.Code)
	}
}

func TestWriteReleaseErrorReturnsConflictForActiveBuildWork(t *testing.T) {
	server, cleanup := newRepositoryTestServer(t)
	defer cleanup()

	response := httptest.NewRecorder()
	server.writeReleaseError(
		response,
		fmt.Errorf(
			"%w for repository %q",
			release.ErrBuildInProgress,
			"revolutions",
		),
	)

	if response.Code != http.StatusConflict {
		t.Fatalf("expected status 409, got %d", response.Code)
	}
	if !strings.Contains(response.Body.String(), "revolutions") {
		t.Fatalf("expected repository name in body, got %q", response.Body.String())
	}
}
