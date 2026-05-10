package app

import (
	"errors"
	"net/http"

	"github.com/indiegabo/handy-unity-bulder/internal/credentials"
)

// credentialsCreateRequest describes the JSON body accepted when creating one
// credential through the HTTP API.
type credentialsCreateRequest struct {
	Name       string `json:"name"`
	Kind       string `json:"kind"`
	ConfigJSON string `json:"config_json"`
}

// credentialsUpdateRequest describes the JSON body accepted when replacing one
// credential through the HTTP API.
type credentialsUpdateRequest struct {
	Name       string `json:"name"`
	Kind       string `json:"kind"`
	ConfigJSON string `json:"config_json"`
}

// handleCredentialsList returns every persisted credential.
func (s *Server) handleCredentialsList(w http.ResponseWriter, r *http.Request) {
	records, err := s.creds.List(r.Context())
	if err != nil {
		s.writeCredentialsError(w, err)
		return
	}

	if err := writeJSON(w, http.StatusOK, records); err != nil {
		s.logger.Error("write credentials list response", "error", err)
	}
}

// handleCredentialsCreate decodes one credential create request and persists
// it.
func (s *Server) handleCredentialsCreate(w http.ResponseWriter, r *http.Request) {
	var request credentialsCreateRequest
	if err := decodeJSONRequest(r, &request); err != nil {
		s.writeAPIError(w, http.StatusBadRequest, "invalid JSON body", err)
		return
	}

	record, err := s.creds.Create(r.Context(), credentials.CreateInput{
		Name:       request.Name,
		Kind:       request.Kind,
		ConfigJSON: request.ConfigJSON,
	})
	if err != nil {
		s.writeCredentialsError(w, err)
		return
	}

	if err := writeJSON(w, http.StatusCreated, record); err != nil {
		s.logger.Error("write credentials create response", "error", err)
	}
}

// handleCredentialsGet returns one credential selected by path id.
func (s *Server) handleCredentialsGet(w http.ResponseWriter, r *http.Request) {
	id, err := resourceIDFromRequest(r, "credentials")
	if err != nil {
		s.writeAPIError(w, http.StatusBadRequest, err.Error(), err)
		return
	}

	record, err := s.creds.Get(r.Context(), id)
	if err != nil {
		s.writeCredentialsError(w, err)
		return
	}

	if err := writeJSON(w, http.StatusOK, record); err != nil {
		s.logger.Error("write credentials get response", "error", err)
	}
}

// handleCredentialsUpdate decodes one credential replacement request and
// persists the new values.
func (s *Server) handleCredentialsUpdate(w http.ResponseWriter, r *http.Request) {
	id, err := resourceIDFromRequest(r, "credentials")
	if err != nil {
		s.writeAPIError(w, http.StatusBadRequest, err.Error(), err)
		return
	}

	var request credentialsUpdateRequest
	if err := decodeJSONRequest(r, &request); err != nil {
		s.writeAPIError(w, http.StatusBadRequest, "invalid JSON body", err)
		return
	}

	record, err := s.creds.Update(r.Context(), id, credentials.UpdateInput{
		Name:       request.Name,
		Kind:       request.Kind,
		ConfigJSON: request.ConfigJSON,
	})
	if err != nil {
		s.writeCredentialsError(w, err)
		return
	}

	if err := writeJSON(w, http.StatusOK, record); err != nil {
		s.logger.Error("write credentials update response", "error", err)
	}
}

// handleCredentialsDelete removes one credential selected by path id.
func (s *Server) handleCredentialsDelete(w http.ResponseWriter, r *http.Request) {
	id, err := resourceIDFromRequest(r, "credentials")
	if err != nil {
		s.writeAPIError(w, http.StatusBadRequest, err.Error(), err)
		return
	}

	if err := s.creds.Delete(r.Context(), id); err != nil {
		s.writeCredentialsError(w, err)
		return
	}

	if err := writeJSON(w, http.StatusOK, map[string]any{"deleted": true, "id": id}); err != nil {
		s.logger.Error("write credentials delete response", "error", err)
	}
}

// writeCredentialsError maps credential store failures to HTTP status codes.
func (s *Server) writeCredentialsError(w http.ResponseWriter, err error) {
	switch {
	case errors.Is(err, credentials.ErrInvalid):
		s.writeAPIError(w, http.StatusBadRequest, err.Error(), err)
	case errors.Is(err, credentials.ErrNotFound):
		s.writeAPIError(w, http.StatusNotFound, err.Error(), err)
	case errors.Is(err, credentials.ErrConflict):
		s.writeAPIError(w, http.StatusConflict, err.Error(), err)
	default:
		s.writeAPIError(w, http.StatusInternalServerError, "credentials request failed", err)
	}
}