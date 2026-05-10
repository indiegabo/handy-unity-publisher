package app

import (
	"errors"
	"net/http"

	"github.com/indiegabo/handy-unity-bulder/internal/trigger"
)

// triggerRuleCreateRequest describes the JSON body accepted when creating one
// trigger rule through the HTTP API.
type triggerRuleCreateRequest struct {
	RepositoryID int64  `json:"repository_id"`
	Name         string `json:"name"`
	Source       string `json:"source"`
	Enabled      *bool  `json:"enabled,omitempty"`
	ConfigJSON   string `json:"config_json"`
}

// triggerRuleUpdateRequest describes the JSON body accepted when replacing one
// trigger rule through the HTTP API.
type triggerRuleUpdateRequest struct {
	Name       string `json:"name"`
	Source     string `json:"source"`
	Enabled    *bool  `json:"enabled"`
	ConfigJSON string `json:"config_json"`
}

// handleTriggerRuleList returns every trigger rule for one repository.
func (s *Server) handleTriggerRuleList(w http.ResponseWriter, r *http.Request) {
	repositoryID, err := int64QueryParam(r, "repository_id")
	if err != nil {
		s.writeAPIError(w, http.StatusBadRequest, err.Error(), err)
		return
	}

	rules, err := s.triggers.ListByRepository(r.Context(), repositoryID)
	if err != nil {
		s.writeTriggerError(w, err)
		return
	}

	if err := writeJSON(w, http.StatusOK, rules); err != nil {
		s.logger.Error("write trigger rule list response", "error", err)
	}
}

// handleTriggerRuleCreate decodes one trigger rule create request and persists
// it.
func (s *Server) handleTriggerRuleCreate(w http.ResponseWriter, r *http.Request) {
	var request triggerRuleCreateRequest
	if err := decodeJSONRequest(r, &request); err != nil {
		s.writeAPIError(w, http.StatusBadRequest, "invalid JSON body", err)
		return
	}

	rule, err := s.triggers.Create(r.Context(), trigger.CreateInput{
		RepositoryID: request.RepositoryID,
		Name:         request.Name,
		Source:       request.Source,
		Enabled:      request.Enabled,
		ConfigJSON:   request.ConfigJSON,
	})
	if err != nil {
		s.writeTriggerError(w, err)
		return
	}

	if err := writeJSON(w, http.StatusCreated, rule); err != nil {
		s.logger.Error("write trigger rule create response", "error", err)
	}
}

// handleTriggerRuleGet returns one trigger rule selected by path id.
func (s *Server) handleTriggerRuleGet(w http.ResponseWriter, r *http.Request) {
	id, err := resourceIDFromRequest(r, "trigger rule")
	if err != nil {
		s.writeAPIError(w, http.StatusBadRequest, err.Error(), err)
		return
	}

	rule, err := s.triggers.Get(r.Context(), id)
	if err != nil {
		s.writeTriggerError(w, err)
		return
	}

	if err := writeJSON(w, http.StatusOK, rule); err != nil {
		s.logger.Error("write trigger rule get response", "error", err)
	}
}

// handleTriggerRuleUpdate decodes one trigger rule replacement request and
// persists the new values.
func (s *Server) handleTriggerRuleUpdate(w http.ResponseWriter, r *http.Request) {
	id, err := resourceIDFromRequest(r, "trigger rule")
	if err != nil {
		s.writeAPIError(w, http.StatusBadRequest, err.Error(), err)
		return
	}

	var request triggerRuleUpdateRequest
	if err := decodeJSONRequest(r, &request); err != nil {
		s.writeAPIError(w, http.StatusBadRequest, "invalid JSON body", err)
		return
	}
	if request.Enabled == nil {
		s.writeAPIError(
			w,
			http.StatusBadRequest,
			"enabled must be provided for trigger rule updates",
			trigger.ErrInvalid,
		)
		return
	}

	rule, err := s.triggers.Update(r.Context(), id, trigger.UpdateInput{
		Name:       request.Name,
		Source:     request.Source,
		Enabled:    *request.Enabled,
		ConfigJSON: request.ConfigJSON,
	})
	if err != nil {
		s.writeTriggerError(w, err)
		return
	}

	if err := writeJSON(w, http.StatusOK, rule); err != nil {
		s.logger.Error("write trigger rule update response", "error", err)
	}
}

// handleTriggerRuleDelete removes one trigger rule selected by path id.
func (s *Server) handleTriggerRuleDelete(w http.ResponseWriter, r *http.Request) {
	id, err := resourceIDFromRequest(r, "trigger rule")
	if err != nil {
		s.writeAPIError(w, http.StatusBadRequest, err.Error(), err)
		return
	}

	if err := s.triggers.Delete(r.Context(), id); err != nil {
		s.writeTriggerError(w, err)
		return
	}

	if err := writeJSON(w, http.StatusOK, map[string]any{"deleted": true, "id": id}); err != nil {
		s.logger.Error("write trigger rule delete response", "error", err)
	}
}

// writeTriggerError maps trigger store failures to HTTP status codes.
func (s *Server) writeTriggerError(w http.ResponseWriter, err error) {
	switch {
	case errors.Is(err, trigger.ErrInvalid):
		s.writeAPIError(w, http.StatusBadRequest, err.Error(), err)
	case errors.Is(err, trigger.ErrNotFound),
		errors.Is(err, trigger.ErrRepositoryNotFound):
		s.writeAPIError(w, http.StatusNotFound, err.Error(), err)
	case errors.Is(err, trigger.ErrConflict):
		s.writeAPIError(w, http.StatusConflict, err.Error(), err)
	default:
		s.writeAPIError(w, http.StatusInternalServerError, "trigger rule request failed", err)
	}
}