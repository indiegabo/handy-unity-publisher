// Package app contains the main application runtime entrypoints.
package app

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"log/slog"
	"net/http"
	"time"

	"github.com/indiegabo/handy-unity-bulder/internal/automation"
	"github.com/indiegabo/handy-unity-bulder/internal/build"
	"github.com/indiegabo/handy-unity-bulder/internal/config"
	"github.com/indiegabo/handy-unity-bulder/internal/credentials"
	"github.com/indiegabo/handy-unity-bulder/internal/pipelines"
	"github.com/indiegabo/handy-unity-bulder/internal/publish"
	"github.com/indiegabo/handy-unity-bulder/internal/release"
	"github.com/indiegabo/handy-unity-bulder/internal/repository"
	"github.com/indiegabo/handy-unity-bulder/internal/trigger"
	"github.com/indiegabo/handy-unity-bulder/internal/version"
)

// Server wraps the HTTP runtime for the main application process.
type Server struct {
	cfg            config.Config
	logger         *slog.Logger
	creds          credentials.Store
	repos          repository.Store
	builds         build.Store
	releases       *release.Dispatcher
	triggers       trigger.Store
	publishes      publish.Store
	pipelineReport pipelines.ApplyReport
	automation     automation.Reporter
	httpServer     *http.Server
}

// NewServer creates an HTTP server configured for the current runtime.
func NewServer(
	cfg config.Config,
	logger *slog.Logger,
	creds credentials.Store,
	repos repository.Store,
	builds build.Store,
	releases *release.Dispatcher,
	triggers trigger.Store,
	publishes publish.Store,
) *Server {
	server := &Server{
		cfg:       cfg,
		logger:    logger,
		creds:     creds,
		repos:     repos,
		builds:    builds,
		releases:  releases,
		triggers:  triggers,
		publishes: publishes,
	}

	mux := http.NewServeMux()
	mux.HandleFunc("/", server.handleRoot)
	mux.HandleFunc("/healthz", server.handleHealth)
	mux.HandleFunc("GET /api/v1/runtime/pipelines", server.handleRuntimePipelines)
	mux.HandleFunc("GET /api/v1/runtime/automation", server.handleRuntimeAutomation)
	mux.HandleFunc("GET /api/v1/credentials", server.handleCredentialsList)
	mux.HandleFunc("POST /api/v1/credentials", server.handleCredentialsCreate)
	mux.HandleFunc("GET /api/v1/credentials/{id}", server.handleCredentialsGet)
	mux.HandleFunc("PUT /api/v1/credentials/{id}", server.handleCredentialsUpdate)
	mux.HandleFunc("DELETE /api/v1/credentials/{id}", server.handleCredentialsDelete)
	mux.HandleFunc("GET /api/v1/repositories", server.handleRepositoryList)
	mux.HandleFunc("POST /api/v1/repositories", server.handleRepositoryCreate)
	mux.HandleFunc("GET /api/v1/repositories/{id}", server.handleRepositoryGet)
	mux.HandleFunc("PUT /api/v1/repositories/{id}", server.handleRepositoryUpdate)
	mux.HandleFunc("POST /api/v1/releases/dispatch/manual", server.handleManualReleaseDispatch)
	mux.HandleFunc(
		"DELETE /api/v1/repositories/{id}",
		server.handleRepositoryDelete,
	)
	mux.HandleFunc("GET /api/v1/build-targets", server.handleBuildTargetList)
	mux.HandleFunc("POST /api/v1/build-targets", server.handleBuildTargetCreate)
	mux.HandleFunc("GET /api/v1/build-targets/{id}", server.handleBuildTargetGet)
	mux.HandleFunc("PUT /api/v1/build-targets/{id}", server.handleBuildTargetUpdate)
	mux.HandleFunc("DELETE /api/v1/build-targets/{id}", server.handleBuildTargetDelete)
	mux.HandleFunc("GET /api/v1/trigger-rules", server.handleTriggerRuleList)
	mux.HandleFunc("POST /api/v1/trigger-rules", server.handleTriggerRuleCreate)
	mux.HandleFunc("GET /api/v1/trigger-rules/{id}", server.handleTriggerRuleGet)
	mux.HandleFunc("PUT /api/v1/trigger-rules/{id}", server.handleTriggerRuleUpdate)
	mux.HandleFunc("DELETE /api/v1/trigger-rules/{id}", server.handleTriggerRuleDelete)
	mux.HandleFunc("GET /api/v1/publish-targets", server.handlePublishTargetList)
	mux.HandleFunc("POST /api/v1/publish-targets", server.handlePublishTargetCreate)
	mux.HandleFunc("GET /api/v1/publish-targets/{id}", server.handlePublishTargetGet)
	mux.HandleFunc("PUT /api/v1/publish-targets/{id}", server.handlePublishTargetUpdate)
	mux.HandleFunc("DELETE /api/v1/publish-targets/{id}", server.handlePublishTargetDelete)
	mux.HandleFunc("GET /api/v1/build-publish-bindings", server.handleBuildPublishBindingList)
	mux.HandleFunc("POST /api/v1/build-publish-bindings", server.handleBuildPublishBindingCreate)
	mux.HandleFunc("GET /api/v1/build-publish-bindings/{id}", server.handleBuildPublishBindingGet)
	mux.HandleFunc("PUT /api/v1/build-publish-bindings/{id}", server.handleBuildPublishBindingUpdate)
	mux.HandleFunc("DELETE /api/v1/build-publish-bindings/{id}", server.handleBuildPublishBindingDelete)
	mux.HandleFunc("GET /api/v1/runtime/database/export", server.handleDatabaseExport)
	mux.HandleFunc("POST /api/v1/runtime/database/import", server.handleDatabaseImport)

	server.httpServer = &http.Server{
		Addr:              cfg.HTTPAddr,
		Handler:           mux,
		ReadHeaderTimeout: 5 * time.Second,
	}

	return server
}

// WithPipelineReport attaches the latest declarative synchronization report.
func (s *Server) WithPipelineReport(report pipelines.ApplyReport) *Server {
	s.pipelineReport = report
	return s
}

// WithAutomationReporter attaches the runtime automation reporter used by the
// operator-facing runtime inspection endpoints.
func (s *Server) WithAutomationReporter(reporter automation.Reporter) *Server {
	s.automation = reporter
	return s
}

// Run starts the HTTP server and shuts it down when the context is canceled.
func (s *Server) Run(ctx context.Context) error {
	go func() {
		<-ctx.Done()

		shutdownCtx, cancel := context.WithTimeout(
			context.Background(),
			10*time.Second,
		)
		defer cancel()

		if err := s.httpServer.Shutdown(shutdownCtx); err != nil {
			s.logger.Error("http server shutdown failed", "error", err)
		}
	}()

	if err := s.httpServer.ListenAndServe(); err != nil &&
		!errors.Is(err, http.ErrServerClosed) {
		return fmt.Errorf("listen and serve: %w", err)
	}

	return nil
}

// handleRoot serves the operator-facing runtime summary document.
func (s *Server) handleRoot(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodGet {
		http.Error(w, http.StatusText(http.StatusMethodNotAllowed), http.StatusMethodNotAllowed)
		return
	}

	response := map[string]string{
		"data_dir": s.cfg.DataDir,
		"env":      s.cfg.Env,
		"name":     "handy-unity-bulder",
		"status":   "ok",
		"version":  version.String(),
	}

	if err := writeJSON(w, http.StatusOK, response); err != nil {
		s.logger.Error("write root response", "error", err)
	}
}

// handleHealth serves the lightweight liveness response used by operators and
// automation.
func (s *Server) handleHealth(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodGet {
		http.Error(w, http.StatusText(http.StatusMethodNotAllowed), http.StatusMethodNotAllowed)
		return
	}

	response := map[string]string{
		"status":  "ok",
		"version": version.String(),
	}

	if err := writeJSON(w, http.StatusOK, response); err != nil {
		s.logger.Error("write health response", "error", err)
	}
}

// writeJSON buffers one JSON payload so encoding failures happen before
// headers are committed to the response.
func writeJSON(w http.ResponseWriter, status int, value any) error {
	var buffer bytes.Buffer

	encoder := json.NewEncoder(&buffer)
	encoder.SetIndent("", "  ")
	if err := encoder.Encode(value); err != nil {
		return fmt.Errorf("encode json: %w", err)
	}

	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)

	if _, err := w.Write(buffer.Bytes()); err != nil {
		return fmt.Errorf("write response body: %w", err)
	}

	return nil
}
