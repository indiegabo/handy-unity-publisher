package app

import (
	"net/http"
	"time"

	"github.com/indiegabo/handy-unity-bulder/internal/automation"
)

// handleRuntimePipelines returns the last declarative pipeline synchronization report.
func (s *Server) handleRuntimePipelines(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodGet {
		http.Error(w, http.StatusText(http.StatusMethodNotAllowed), http.StatusMethodNotAllowed)
		return
	}

	if err := writeJSON(w, http.StatusOK, s.pipelineReport); err != nil {
		s.logger.Error("write runtime pipelines response", "error", err)
	}
}

// handleRuntimeAutomation returns the current polling and release-backlog
// report maintained by the automation coordinator.
func (s *Server) handleRuntimeAutomation(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodGet {
		http.Error(w, http.StatusText(http.StatusMethodNotAllowed), http.StatusMethodNotAllowed)
		return
	}

	if s.automation == nil {
		report := automation.RuntimeReport{
			GeneratedAt:  time.Now().UTC().Format(time.RFC3339),
			Repositories: []automation.RepositoryRuntimeStatus{},
		}
		if err := writeJSON(w, http.StatusOK, report); err != nil {
			s.logger.Error("write empty runtime automation response", "error", err)
		}
		return
	}

	report, err := s.automation.Snapshot(r.Context())
	if err != nil {
		s.writeAPIError(
			w,
			http.StatusInternalServerError,
			"runtime automation report failed",
			err,
		)
		return
	}

	if err := writeJSON(w, http.StatusOK, report); err != nil {
		s.logger.Error("write runtime automation response", "error", err)
	}
}
