package app

import (
	"context"
	"errors"
	"net/http"
	"net/http/httptest"
	"testing"

	"github.com/indiegabo/handy-unity-bulder/internal/automation"
)

func TestRuntimeAutomationEndpointReturnsCoordinatorReport(t *testing.T) {
	t.Parallel()

	server, cleanup := newRepositoryTestServer(t)
	defer cleanup()

	server.WithAutomationReporter(runtimeAutomationReporterStub{
		report: automation.RuntimeReport{
			GeneratedAt: "2026-05-09T00:00:00Z",
			Repositories: []automation.RepositoryRuntimeStatus{
				{
					RepositoryID:        1,
					RepositoryName:      "revolutions",
					PollState:           automation.PollStatePaused,
					PendingReleaseCount: 2,
				},
			},
		},
	})

	report := performJSONRequest[automation.RuntimeReport](
		t,
		server,
		http.MethodGet,
		"/api/v1/runtime/automation",
		nil,
		http.StatusOK,
	)

	if len(report.Repositories) != 1 || report.Repositories[0].PollState != automation.PollStatePaused {
		t.Fatalf("runtime automation report = %#v", report)
	}
}

func TestRuntimeAutomationEndpointReturnsInternalErrorOnSnapshotFailure(t *testing.T) {
	t.Parallel()

	server, cleanup := newRepositoryTestServer(t)
	defer cleanup()

	server.WithAutomationReporter(runtimeAutomationReporterStub{
		err: errors.New("snapshot failed"),
	})

	recorder := httptest.NewRecorder()
	request := httptest.NewRequest(http.MethodGet, "/api/v1/runtime/automation", nil)
	server.httpServer.Handler.ServeHTTP(recorder, request)

	if recorder.Code != http.StatusInternalServerError {
		t.Fatalf("expected 500 runtime automation response, got %d with body %s", recorder.Code, recorder.Body.String())
	}
	if recorder.Body.String() == "" {
		t.Fatal("expected JSON error body for runtime automation failure")
	}
}

type runtimeAutomationReporterStub struct {
	report automation.RuntimeReport
	err    error
}

func (s runtimeAutomationReporterStub) Snapshot(context.Context) (automation.RuntimeReport, error) {
	if s.err != nil {
		return automation.RuntimeReport{}, s.err
	}

	return s.report, nil
}
