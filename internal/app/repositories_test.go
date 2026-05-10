package app

import (
	"bytes"
	"context"
	"encoding/json"
	"io"
	"log/slog"
	"net/http"
	"net/http/httptest"
	"path/filepath"
	"testing"
	"time"

	"github.com/indiegabo/handy-unity-bulder/internal/build"
	"github.com/indiegabo/handy-unity-bulder/internal/config"
	"github.com/indiegabo/handy-unity-bulder/internal/credentials"
	"github.com/indiegabo/handy-unity-bulder/internal/db"
	"github.com/indiegabo/handy-unity-bulder/internal/publish"
	"github.com/indiegabo/handy-unity-bulder/internal/release"
	"github.com/indiegabo/handy-unity-bulder/internal/repository"
	"github.com/indiegabo/handy-unity-bulder/internal/trigger"
	"github.com/indiegabo/handy-unity-bulder/internal/worker"
)

func TestRepositoryManagementEndpoints(t *testing.T) {
	t.Parallel()

	server, cleanup := newRepositoryTestServer(t)
	defer cleanup()

	created := performJSONRequest[repository.Record](
		t,
		server,
		http.MethodPost,
		"/api/v1/repositories",
		map[string]any{
			"name":                     "api-repo",
			"repo_url":                 "https://example.com/org/api.git",
			"default_branch":           "main",
			"polling_interval_seconds": 180,
		},
		http.StatusCreated,
	)

	if created.ID == 0 {
		t.Fatalf("expected created repository id to be set")
	}

	loaded := performJSONRequest[repository.Record](
		t,
		server,
		http.MethodGet,
		"/api/v1/repositories/1",
		nil,
		http.StatusOK,
	)

	if loaded.Name != "api-repo" {
		t.Fatalf("expected repository name %q, got %q", "api-repo", loaded.Name)
	}

	updated := performJSONRequest[repository.Record](
		t,
		server,
		http.MethodPut,
		"/api/v1/repositories/1",
		map[string]any{
			"name":                     "api-repo-stable",
			"repo_url":                 "https://example.com/org/api.git",
			"default_branch":           "stable",
			"polling_interval_seconds": 900,
			"enabled":                  false,
		},
		http.StatusOK,
	)

	if updated.Enabled {
		t.Fatalf("expected updated repository to be disabled")
	}

	list := performJSONRequest[[]repository.Record](
		t,
		server,
		http.MethodGet,
		"/api/v1/repositories",
		nil,
		http.StatusOK,
	)

	if len(list) != 1 {
		t.Fatalf("expected one repository in list, got %d", len(list))
	}

	performJSONRequest[map[string]any](
		t,
		server,
		http.MethodDelete,
		"/api/v1/repositories/1",
		nil,
		http.StatusOK,
	)

	recorder := httptest.NewRecorder()
	request := httptest.NewRequest(http.MethodGet, "/api/v1/repositories/1", nil)
	server.httpServer.Handler.ServeHTTP(recorder, request)

	if recorder.Code != http.StatusNotFound {
		t.Fatalf("expected 404 after delete, got %d", recorder.Code)
	}
}

func newRepositoryTestServer(t *testing.T) (*Server, func()) {
	t.Helper()

	dataDir := t.TempDir()
	cfg := config.Config{
		HTTPAddr:     ":0",
		DataDir:      dataDir,
		DatabasePath: filepath.Join(dataDir, "app.db"),
	}

	database, err := db.Open(context.Background(), cfg)
	if err != nil {
		t.Fatalf("open test database: %v", err)
	}

	logger := slog.New(slog.NewTextHandler(io.Discard, nil))
	server := NewServer(
		cfg,
		logger,
		credentials.NewStore(database),
		repository.NewStore(database),
		build.NewStore(database),
		release.NewDispatcher(release.NewStore(database), &serverTestQueueStub{}),
		trigger.NewStore(database),
		publish.NewStore(database),
	)

	cleanup := func() {
		if err := database.Close(); err != nil {
			t.Fatalf("close test database: %v", err)
		}
	}

	return server, cleanup
}

type serverTestQueueStub struct{}

func (s *serverTestQueueStub) Enqueue(
	context.Context,
	string,
	[]byte,
) error {
	return nil
}

func (s *serverTestQueueStub) Dequeue(
	context.Context,
	string,
	time.Duration,
) ([]byte, error) {
	return nil, nil
}

var _ worker.Queue = (*serverTestQueueStub)(nil)

func performJSONRequest[T any](
	t *testing.T,
	server *Server,
	method string,
	path string,
	body any,
	wantStatus int,
) T {
	t.Helper()

	var reader *bytes.Reader
	if body == nil {
		reader = bytes.NewReader(nil)
	} else {
		payload, err := json.Marshal(body)
		if err != nil {
			t.Fatalf("marshal request body: %v", err)
		}

		reader = bytes.NewReader(payload)
	}

	request := httptest.NewRequest(method, path, reader)
	if body != nil {
		request.Header.Set("Content-Type", "application/json")
	}

	recorder := httptest.NewRecorder()
	server.httpServer.Handler.ServeHTTP(recorder, request)

	if recorder.Code != wantStatus {
		t.Fatalf(
			"expected status %d, got %d with body %s",
			wantStatus,
			recorder.Code,
			recorder.Body.String(),
		)
	}

	var response T
	if err := json.Unmarshal(recorder.Body.Bytes(), &response); err != nil {
		t.Fatalf("decode response body: %v", err)
	}

	return response
}
