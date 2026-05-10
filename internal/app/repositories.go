package app

import (
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"strconv"

	"github.com/indiegabo/handy-unity-bulder/internal/repository"
)

// repositoryCreateRequest describes the JSON body accepted when creating one
// repository through the HTTP API.
type repositoryCreateRequest struct {
	Name                   string `json:"name"`
	RepoURL                string `json:"repo_url"`
	CredentialsID          *int64 `json:"credentials_id"`
	DefaultBranch          string `json:"default_branch"`
	PollingIntervalSeconds int    `json:"polling_interval_seconds"`
	Enabled                *bool  `json:"enabled,omitempty"`
}

// repositoryUpdateRequest describes the JSON body accepted when replacing one
// repository through the HTTP API.
type repositoryUpdateRequest struct {
	Name                   string `json:"name"`
	RepoURL                string `json:"repo_url"`
	CredentialsID          *int64 `json:"credentials_id"`
	DefaultBranch          string `json:"default_branch"`
	PollingIntervalSeconds int    `json:"polling_interval_seconds"`
	Enabled                *bool  `json:"enabled"`
}

// handleRepositoryList returns every persisted repository.
func (s *Server) handleRepositoryList(w http.ResponseWriter, r *http.Request) {
	records, err := s.repos.List(r.Context())
	if err != nil {
		s.writeAPIError(
			w,
			http.StatusInternalServerError,
			"list repositories failed",
			err,
		)
		return
	}

	if err := writeJSON(w, http.StatusOK, records); err != nil {
		s.logger.Error("write repository list response", "error", err)
	}
}

// handleRepositoryCreate decodes one repository create request and persists it.
func (s *Server) handleRepositoryCreate(w http.ResponseWriter, r *http.Request) {
	var request repositoryCreateRequest
	if err := decodeJSONRequest(r, &request); err != nil {
		s.writeAPIError(w, http.StatusBadRequest, "invalid JSON body", err)
		return
	}

	record, err := s.repos.Create(r.Context(), repository.CreateInput{
		Name:                   request.Name,
		RepoURL:                request.RepoURL,
		CredentialsID:          request.CredentialsID,
		DefaultBranch:          request.DefaultBranch,
		PollingIntervalSeconds: request.PollingIntervalSeconds,
		Enabled:                request.Enabled,
	})
	if err != nil {
		s.writeRepositoryError(w, err)
		return
	}

	if err := writeJSON(w, http.StatusCreated, record); err != nil {
		s.logger.Error("write repository create response", "error", err)
	}
}

// handleRepositoryGet returns one repository selected by path id.
func (s *Server) handleRepositoryGet(w http.ResponseWriter, r *http.Request) {
	id, err := repositoryIDFromRequest(r)
	if err != nil {
		s.writeAPIError(w, http.StatusBadRequest, err.Error(), err)
		return
	}

	record, err := s.repos.Get(r.Context(), id)
	if err != nil {
		s.writeRepositoryError(w, err)
		return
	}

	if err := writeJSON(w, http.StatusOK, record); err != nil {
		s.logger.Error("write repository get response", "error", err)
	}
}

// handleRepositoryUpdate decodes one repository replacement request and
// persists the new values.
func (s *Server) handleRepositoryUpdate(w http.ResponseWriter, r *http.Request) {
	id, err := repositoryIDFromRequest(r)
	if err != nil {
		s.writeAPIError(w, http.StatusBadRequest, err.Error(), err)
		return
	}

	var request repositoryUpdateRequest
	if err := decodeJSONRequest(r, &request); err != nil {
		s.writeAPIError(w, http.StatusBadRequest, "invalid JSON body", err)
		return
	}

	if request.Enabled == nil {
		s.writeAPIError(
			w,
			http.StatusBadRequest,
			"enabled must be provided for repository updates",
			repository.ErrInvalid,
		)
		return
	}

	record, err := s.repos.Update(r.Context(), id, repository.UpdateInput{
		Name:                   request.Name,
		RepoURL:                request.RepoURL,
		CredentialsID:          request.CredentialsID,
		DefaultBranch:          request.DefaultBranch,
		PollingIntervalSeconds: request.PollingIntervalSeconds,
		Enabled:                *request.Enabled,
	})
	if err != nil {
		s.writeRepositoryError(w, err)
		return
	}

	if err := writeJSON(w, http.StatusOK, record); err != nil {
		s.logger.Error("write repository update response", "error", err)
	}
}

// handleRepositoryDelete removes one repository selected by path id.
func (s *Server) handleRepositoryDelete(w http.ResponseWriter, r *http.Request) {
	id, err := repositoryIDFromRequest(r)
	if err != nil {
		s.writeAPIError(w, http.StatusBadRequest, err.Error(), err)
		return
	}

	if err := s.repos.Delete(r.Context(), id); err != nil {
		s.writeRepositoryError(w, err)
		return
	}

	if err := writeJSON(w, http.StatusOK, map[string]any{"deleted": true, "id": id}); err != nil {
		s.logger.Error("write repository delete response", "error", err)
	}
}

// writeRepositoryError maps repository store failures to HTTP status codes.
func (s *Server) writeRepositoryError(w http.ResponseWriter, err error) {
	switch {
	case errors.Is(err, repository.ErrInvalid):
		s.writeAPIError(w, http.StatusBadRequest, err.Error(), err)
	case errors.Is(err, repository.ErrNotFound):
		s.writeAPIError(w, http.StatusNotFound, err.Error(), err)
	case errors.Is(err, repository.ErrConflict):
		s.writeAPIError(w, http.StatusConflict, err.Error(), err)
	default:
		s.writeAPIError(w, http.StatusInternalServerError, "repository request failed", err)
	}
}

// writeAPIError logs one API failure and writes the normalized JSON error
// envelope expected by the operator clients.
func (s *Server) writeAPIError(
	w http.ResponseWriter,
	status int,
	message string,
	err error,
) {
	if err != nil {
		s.logger.Error("api request failed", "status", status, "message", message, "error", err)
	}

	if writeErr := writeJSON(w, status, map[string]string{"error": message}); writeErr != nil {
		s.logger.Error("write api error response", "error", writeErr)
	}
}

// repositoryIDFromRequest parses the repository path parameter from the
// current request.
func repositoryIDFromRequest(r *http.Request) (int64, error) {
	return resourceIDFromRequest(r, "repository")
}

// resourceIDFromRequest parses one positive integer id from the `{id}` path
// parameter and tailors the validation error to the named resource.
func resourceIDFromRequest(r *http.Request, resourceName string) (int64, error) {
	idValue := r.PathValue("id")
	id, err := strconv.ParseInt(idValue, 10, 64)
	if err != nil || id <= 0 {
		return 0, fmt.Errorf("invalid %s id %q", resourceName, idValue)
	}

	return id, nil
}

// int64QueryParam parses one required positive integer query parameter.
func int64QueryParam(r *http.Request, key string) (int64, error) {
	value := r.URL.Query().Get(key)
	id, err := strconv.ParseInt(value, 10, 64)
	if err != nil || id <= 0 {
		return 0, fmt.Errorf("invalid query parameter %q", key)
	}

	return id, nil
}

// decodeJSONRequest decodes exactly one JSON object and rejects trailing input
// or unknown fields.
func decodeJSONRequest(r *http.Request, dst any) error {
	defer r.Body.Close()

	decoder := json.NewDecoder(r.Body)
	decoder.DisallowUnknownFields()

	if err := decoder.Decode(dst); err != nil {
		return err
	}

	if err := decoder.Decode(&struct{}{}); !errors.Is(err, io.EOF) {
		return fmt.Errorf("request body must contain a single JSON object")
	}

	return nil
}