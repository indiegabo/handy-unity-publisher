package app

import (
	"errors"
	"net/http"

	"github.com/indiegabo/handy-unity-bulder/internal/release"
)

// manualReleaseDispatchRequest describes the JSON body accepted when creating
// one manually requested release run through the HTTP API.
type manualReleaseDispatchRequest struct {
	RepositoryID int64  `json:"repository_id"`
	GitTag       string `json:"git_tag"`
	GitCommit    string `json:"git_commit"`
	Rebuild      bool   `json:"rebuild,omitempty"`
}

// handleManualReleaseDispatch decodes one manual release dispatch request,
// persists the release run, and enqueues the downstream release job.
func (s *Server) handleManualReleaseDispatch(w http.ResponseWriter, r *http.Request) {
	var request manualReleaseDispatchRequest
	if err := decodeJSONRequest(r, &request); err != nil {
		s.writeAPIError(w, http.StatusBadRequest, "invalid JSON body", err)
		return
	}

	gitCommit := ""
	if request.GitCommit != "" {
		gitCommit = request.GitCommit
	}

	input := release.ManualDispatchInput{
		RepositoryID: request.RepositoryID,
		GitTag:       request.GitTag,
		GitCommit:    gitCommit,
		RequestedVia: "hub",
	}

	var record release.Record
	var err error
	if request.Rebuild {
		record, err = s.releases.DispatchManualRebuild(r.Context(), input)
	} else {
		record, err = s.releases.DispatchManual(r.Context(), input)
	}
	if err != nil {
		s.writeReleaseError(w, err)
		return
	}

	if err := writeJSON(w, http.StatusCreated, record); err != nil {
		s.logger.Error("write manual release dispatch response", "error", err)
	}
}

// writeReleaseError maps release dispatcher and store failures to HTTP status
// codes expected by operator-facing clients.
func (s *Server) writeReleaseError(w http.ResponseWriter, err error) {
	switch {
	case errors.Is(err, release.ErrInvalid):
		s.writeAPIError(w, http.StatusBadRequest, err.Error(), err)
	case errors.Is(err, release.ErrBuildInProgress):
		s.writeAPIError(w, http.StatusConflict, err.Error(), err)
	case errors.Is(err, release.ErrRepositoryNotFound):
		s.writeAPIError(w, http.StatusNotFound, err.Error(), err)
	case errors.Is(err, release.ErrNotFound):
		s.writeAPIError(w, http.StatusNotFound, err.Error(), err)
	case errors.Is(err, release.ErrConflict):
		s.writeAPIError(w, http.StatusConflict, err.Error(), err)
	case errors.Is(err, release.ErrDispatchInProgress):
		s.writeAPIError(w, http.StatusConflict, err.Error(), err)
	case errors.Is(err, release.ErrDispatchAlreadyClaimed):
		s.writeAPIError(w, http.StatusConflict, err.Error(), err)
	default:
		s.writeAPIError(w, http.StatusInternalServerError, "release request failed", err)
	}
}
