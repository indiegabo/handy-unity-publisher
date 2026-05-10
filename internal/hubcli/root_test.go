package hubcli

import (
	"bytes"
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync"
	"testing"

	"github.com/indiegabo/handy-unity-bulder/internal/automation"
	"github.com/indiegabo/handy-unity-bulder/internal/pipelines"
	"github.com/indiegabo/handy-unity-bulder/internal/release"
	"github.com/indiegabo/handy-unity-bulder/internal/repository"
)

func TestRunRuntimePipelinesViaHTTP(t *testing.T) {
	server := newHubTestServer(t)
	server.runtimePipelines = pipelines.ApplyReport{
		Pipelines: []pipelines.ApplyStatus{
			{
				Path:         "/workspace/pipelines/revolutions.yml",
				PipelineName: "revolutions",
				Applied:      true,
			},
		},
	}
	t.Setenv("HUB_BASE_URL", server.URL)

	var stdout bytes.Buffer
	var stderr bytes.Buffer
	if exitCode := Run([]string{"runtime", "pipelines"}, &stdout, &stderr); exitCode != 0 {
		t.Fatalf("expected runtime pipelines command to succeed, got exit code %d: %s", exitCode, stderr.String())
	}

	var report pipelines.ApplyReport
	if err := json.Unmarshal(stdout.Bytes(), &report); err != nil {
		t.Fatalf("decode runtime pipelines output: %v", err)
	}
	if len(report.Pipelines) != 1 || report.Pipelines[0].PipelineName != "revolutions" {
		t.Fatalf("runtime report = %#v", report)
	}
}

func TestRunRuntimePipelinesShowsStartHintWhenServiceIsDown(t *testing.T) {
	t.Setenv("HUB_BASE_URL", "http://127.0.0.1:1")
	var stdout bytes.Buffer
	var stderr bytes.Buffer
	if exitCode := Run([]string{"runtime", "pipelines"}, &stdout, &stderr); exitCode == 0 {
		t.Fatal("expected hub runtime pipelines to fail when service is unavailable")
	}

	if !strings.Contains(stderr.String(), "docker compose up -d") {
		t.Fatalf("expected start hint in stderr, got %q", stderr.String())
	}
}

func TestRunRuntimeAutomationViaHTTP(t *testing.T) {
	server := newHubTestServer(t)
	server.runtimeAutomation = automation.RuntimeReport{
		GeneratedAt: "2026-05-09T00:00:00Z",
		Repositories: []automation.RepositoryRuntimeStatus{
			{
				RepositoryID:        1,
				RepositoryName:      "revolutions",
				PollState:           automation.PollStatePaused,
				PendingReleaseCount: 2,
			},
		},
	}
	t.Setenv("HUB_BASE_URL", server.URL)

	var stdout bytes.Buffer
	var stderr bytes.Buffer
	if exitCode := Run([]string{"runtime", "automation"}, &stdout, &stderr); exitCode != 0 {
		t.Fatalf("expected runtime automation command to succeed, got exit code %d: %s", exitCode, stderr.String())
	}

	var report automation.RuntimeReport
	if err := json.Unmarshal(stdout.Bytes(), &report); err != nil {
		t.Fatalf("decode runtime automation output: %v", err)
	}
	if len(report.Repositories) != 1 || report.Repositories[0].PollState != automation.PollStatePaused {
		t.Fatalf("runtime automation report = %#v", report)
	}
}

func TestRunDispatchViaHTTP(t *testing.T) {
	server := newHubTestServer(t)
	server.repositories = []repository.Record{{ID: 7, Name: "revolutions"}}
	server.dispatchedRelease = release.Record{
		ID:           11,
		RepositoryID: 7,
		GitTag:       "v1.2.3",
		Status:       release.StatusQueued,
	}
	t.Setenv("HUB_BASE_URL", server.URL)

	var stdout bytes.Buffer
	var stderr bytes.Buffer
	if exitCode := Run([]string{"dispatch", "revolutions", "v1.2.3"}, &stdout, &stderr); exitCode != 0 {
		t.Fatalf("expected dispatch command to succeed, got exit code %d: %s", exitCode, stderr.String())
	}

	if !strings.Contains(stdout.String(), "\"git_tag\": \"v1.2.3\"") {
		t.Fatalf("expected dispatched release JSON in stdout, got %q", stdout.String())
	}

	server.mu.Lock()
	defer server.mu.Unlock()
	if server.lastDispatchRequest.RepositoryID != 7 {
		t.Fatalf("expected repository id 7, got %d", server.lastDispatchRequest.RepositoryID)
	}
	if server.lastDispatchRequest.GitTag != "v1.2.3" {
		t.Fatalf("expected git tag v1.2.3, got %q", server.lastDispatchRequest.GitTag)
	}
}

func TestRunDispatchViaHTTPWithRebuildFlagAfterPositionals(t *testing.T) {
	server := newHubTestServer(t)
	server.repositories = []repository.Record{{ID: 7, Name: "revolutions"}}
	server.dispatchedRelease = release.Record{
		ID:           11,
		RepositoryID: 7,
		GitTag:       "v1.2.3",
		Status:       release.StatusQueued,
	}
	t.Setenv("HUB_BASE_URL", server.URL)

	var stdout bytes.Buffer
	var stderr bytes.Buffer
	if exitCode := Run(
		[]string{"dispatch", "revolutions", "v1.2.3", "--rebuild", "--git-commit", "feedface"},
		&stdout,
		&stderr,
	); exitCode != 0 {
		t.Fatalf("expected dispatch command to succeed, got exit code %d: %s", exitCode, stderr.String())
	}

	server.mu.Lock()
	defer server.mu.Unlock()
	if !server.lastDispatchRequest.Rebuild {
		t.Fatal("expected rebuild flag in dispatch request")
	}
	if server.lastDispatchRequest.GitCommit != "feedface" {
		t.Fatalf("expected git commit feedface, got %q", server.lastDispatchRequest.GitCommit)
	}
}

func TestRunDispatchFailsWhenRepositoryDoesNotExist(t *testing.T) {
	server := newHubTestServer(t)
	t.Setenv("HUB_BASE_URL", server.URL)

	var stdout bytes.Buffer
	var stderr bytes.Buffer
	if exitCode := Run([]string{"dispatch", "revolutions", "v1.2.3"}, &stdout, &stderr); exitCode == 0 {
		t.Fatal("expected dispatch command to fail when repository is missing")
	}

	if !strings.Contains(stderr.String(), "repository \"revolutions\" was not found") {
		t.Fatalf("expected repository not found error, got %q", stderr.String())
	}
}

func TestRunDispatchShowsActiveBuildConflict(t *testing.T) {
	server := newHubTestServer(t)
	server.repositories = []repository.Record{{ID: 7, Name: "revolutions"}}
	server.dispatchStatus = http.StatusConflict
	server.dispatchError = "repository already has queued or running build work for repository \"revolutions\""
	t.Setenv("HUB_BASE_URL", server.URL)

	var stdout bytes.Buffer
	var stderr bytes.Buffer
	if exitCode := Run([]string{"dispatch", "revolutions", "v1.2.3"}, &stdout, &stderr); exitCode == 0 {
		t.Fatal("expected dispatch command to fail when build work is active")
	}

	if !strings.Contains(stderr.String(), "already has queued or running build work") {
		t.Fatalf("expected active build error in stderr, got %q", stderr.String())
	}
}

type hubTestServer struct {
	*httptest.Server
	mu                  sync.Mutex
	databaseSnapshot    []byte
	importedDatabase    []byte
	runtimePipelines    pipelines.ApplyReport
	runtimeAutomation   automation.RuntimeReport
	repositories        []repository.Record
	lastDispatchRequest manualReleaseDispatchClientRequest
	dispatchedRelease   release.Record
	dispatchStatus      int
	dispatchError       string
}

func newHubTestServer(t *testing.T) *hubTestServer {
	t.Helper()

	state := &hubTestServer{}
	mux := http.NewServeMux()
	mux.HandleFunc("/healthz", func(w http.ResponseWriter, r *http.Request) {
		writeHubJSON(t, w, http.StatusOK, map[string]string{"status": "ok"})
	})
	mux.HandleFunc("/api/v1/runtime/pipelines", func(w http.ResponseWriter, r *http.Request) {
		state.mu.Lock()
		defer state.mu.Unlock()
		writeHubJSON(t, w, http.StatusOK, state.runtimePipelines)
	})
	mux.HandleFunc("/api/v1/runtime/automation", func(w http.ResponseWriter, r *http.Request) {
		state.mu.Lock()
		defer state.mu.Unlock()
		writeHubJSON(t, w, http.StatusOK, state.runtimeAutomation)
	})
	mux.HandleFunc("/api/v1/repositories", func(w http.ResponseWriter, r *http.Request) {
		state.mu.Lock()
		defer state.mu.Unlock()
		writeHubJSON(t, w, http.StatusOK, state.repositories)
	})
	mux.HandleFunc("/api/v1/releases/dispatch/manual", func(w http.ResponseWriter, r *http.Request) {
		state.mu.Lock()
		defer state.mu.Unlock()
		defer r.Body.Close()

		if err := json.NewDecoder(r.Body).Decode(&state.lastDispatchRequest); err != nil {
			t.Fatalf("decode hub test dispatch request: %v", err)
		}
		if state.dispatchError != "" {
			status := state.dispatchStatus
			if status == 0 {
				status = http.StatusConflict
			}
			writeHubJSON(t, w, status, map[string]string{"error": state.dispatchError})
			return
		}

		writeHubJSON(t, w, http.StatusCreated, state.dispatchedRelease)
	})
	mux.HandleFunc("/api/v1/runtime/database/export", func(w http.ResponseWriter, r *http.Request) {
		state.mu.Lock()
		defer state.mu.Unlock()

		payload := state.databaseSnapshot
		if payload == nil {
			payload = []byte("sqlite-test-snapshot")
		}

		w.Header().Set("Content-Type", "application/octet-stream")
		w.WriteHeader(http.StatusOK)
		_, _ = w.Write(payload)
	})
	mux.HandleFunc("/api/v1/runtime/database/import", func(w http.ResponseWriter, r *http.Request) {
		state.mu.Lock()
		defer state.mu.Unlock()
		defer r.Body.Close()

		payload, err := io.ReadAll(r.Body)
		if err != nil {
			t.Fatalf("read hub test database import body: %v", err)
		}
		state.importedDatabase = payload

		writeHubJSON(t, w, http.StatusAccepted, map[string]any{
			"imported":         true,
			"restart_required": true,
		})
	})

	state.Server = httptest.NewServer(mux)
	t.Cleanup(state.Close)
	return state
}

func writeHubJSON(t *testing.T, w http.ResponseWriter, status int, value any) {
	t.Helper()

	payload, err := json.Marshal(value)
	if err != nil {
		t.Fatalf("marshal hub test response: %v", err)
	}

	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	_, _ = w.Write(payload)
}
