package app

import (
	"errors"
	"net/http"

	"github.com/indiegabo/handy-unity-bulder/internal/publish"
)

// publishTargetCreateRequest describes the JSON body accepted when creating
// one publish target through the HTTP API.
type publishTargetCreateRequest struct {
	RepositoryID  int64  `json:"repository_id"`
	Name          string `json:"name"`
	Kind          string `json:"kind"`
	CredentialsID *int64 `json:"credentials_id"`
	Enabled       *bool  `json:"enabled,omitempty"`
	ConfigJSON    string `json:"config_json"`
}

// publishTargetUpdateRequest describes the JSON body accepted when replacing
// one publish target through the HTTP API.
type publishTargetUpdateRequest struct {
	Name          string `json:"name"`
	Kind          string `json:"kind"`
	CredentialsID *int64 `json:"credentials_id"`
	Enabled       *bool  `json:"enabled"`
	ConfigJSON    string `json:"config_json"`
}

// buildPublishBindingCreateRequest describes the JSON body accepted when
// creating one build-to-publish binding through the HTTP API.
type buildPublishBindingCreateRequest struct {
	BuildTargetID   int64  `json:"build_target_id"`
	PublishTargetID int64  `json:"publish_target_id"`
	Enabled         *bool  `json:"enabled,omitempty"`
	OptionsJSON     string `json:"options_json"`
}

// buildPublishBindingUpdateRequest describes the JSON body accepted when
// replacing one build-to-publish binding through the HTTP API.
type buildPublishBindingUpdateRequest struct {
	Enabled     *bool  `json:"enabled"`
	OptionsJSON string `json:"options_json"`
}

// handlePublishTargetList returns every publish target for one repository.
func (s *Server) handlePublishTargetList(w http.ResponseWriter, r *http.Request) {
	repositoryID, err := int64QueryParam(r, "repository_id")
	if err != nil {
		s.writeAPIError(w, http.StatusBadRequest, err.Error(), err)
		return
	}

	targets, err := s.publishes.ListTargetsByRepository(r.Context(), repositoryID)
	if err != nil {
		s.writePublishError(w, err)
		return
	}

	if err := writeJSON(w, http.StatusOK, targets); err != nil {
		s.logger.Error("write publish target list response", "error", err)
	}
}

// handlePublishTargetCreate decodes one publish target create request and
// persists it.
func (s *Server) handlePublishTargetCreate(w http.ResponseWriter, r *http.Request) {
	var request publishTargetCreateRequest
	if err := decodeJSONRequest(r, &request); err != nil {
		s.writeAPIError(w, http.StatusBadRequest, "invalid JSON body", err)
		return
	}

	target, err := s.publishes.CreateTarget(r.Context(), publish.CreateTargetInput{
		RepositoryID:  request.RepositoryID,
		Name:          request.Name,
		Kind:          request.Kind,
		CredentialsID: request.CredentialsID,
		Enabled:       request.Enabled,
		ConfigJSON:    request.ConfigJSON,
	})
	if err != nil {
		s.writePublishError(w, err)
		return
	}

	if err := writeJSON(w, http.StatusCreated, target); err != nil {
		s.logger.Error("write publish target create response", "error", err)
	}
}

// handlePublishTargetGet returns one publish target selected by path id.
func (s *Server) handlePublishTargetGet(w http.ResponseWriter, r *http.Request) {
	id, err := resourceIDFromRequest(r, "publish target")
	if err != nil {
		s.writeAPIError(w, http.StatusBadRequest, err.Error(), err)
		return
	}

	target, err := s.publishes.GetTarget(r.Context(), id)
	if err != nil {
		s.writePublishError(w, err)
		return
	}

	if err := writeJSON(w, http.StatusOK, target); err != nil {
		s.logger.Error("write publish target get response", "error", err)
	}
}

// handlePublishTargetUpdate decodes one publish target replacement request and
// persists the new values.
func (s *Server) handlePublishTargetUpdate(w http.ResponseWriter, r *http.Request) {
	id, err := resourceIDFromRequest(r, "publish target")
	if err != nil {
		s.writeAPIError(w, http.StatusBadRequest, err.Error(), err)
		return
	}

	var request publishTargetUpdateRequest
	if err := decodeJSONRequest(r, &request); err != nil {
		s.writeAPIError(w, http.StatusBadRequest, "invalid JSON body", err)
		return
	}
	if request.Enabled == nil {
		s.writeAPIError(
			w,
			http.StatusBadRequest,
			"enabled must be provided for publish target updates",
			publish.ErrInvalid,
		)
		return
	}

	target, err := s.publishes.UpdateTarget(r.Context(), id, publish.UpdateTargetInput{
		Name:          request.Name,
		Kind:          request.Kind,
		CredentialsID: request.CredentialsID,
		Enabled:       *request.Enabled,
		ConfigJSON:    request.ConfigJSON,
	})
	if err != nil {
		s.writePublishError(w, err)
		return
	}

	if err := writeJSON(w, http.StatusOK, target); err != nil {
		s.logger.Error("write publish target update response", "error", err)
	}
}

// handlePublishTargetDelete removes one publish target selected by path id.
func (s *Server) handlePublishTargetDelete(w http.ResponseWriter, r *http.Request) {
	id, err := resourceIDFromRequest(r, "publish target")
	if err != nil {
		s.writeAPIError(w, http.StatusBadRequest, err.Error(), err)
		return
	}

	if err := s.publishes.DeleteTarget(r.Context(), id); err != nil {
		s.writePublishError(w, err)
		return
	}

	if err := writeJSON(w, http.StatusOK, map[string]any{"deleted": true, "id": id}); err != nil {
		s.logger.Error("write publish target delete response", "error", err)
	}
}

// handleBuildPublishBindingList returns every binding for one build target.
func (s *Server) handleBuildPublishBindingList(w http.ResponseWriter, r *http.Request) {
	buildTargetID, err := int64QueryParam(r, "build_target_id")
	if err != nil {
		s.writeAPIError(w, http.StatusBadRequest, err.Error(), err)
		return
	}

	bindings, err := s.publishes.ListBindingsByBuildTarget(r.Context(), buildTargetID)
	if err != nil {
		s.writePublishError(w, err)
		return
	}

	if err := writeJSON(w, http.StatusOK, bindings); err != nil {
		s.logger.Error("write build publish binding list response", "error", err)
	}
}

// handleBuildPublishBindingCreate decodes one binding create request and
// persists it.
func (s *Server) handleBuildPublishBindingCreate(w http.ResponseWriter, r *http.Request) {
	var request buildPublishBindingCreateRequest
	if err := decodeJSONRequest(r, &request); err != nil {
		s.writeAPIError(w, http.StatusBadRequest, "invalid JSON body", err)
		return
	}

	binding, err := s.publishes.CreateBinding(r.Context(), publish.CreateBindingInput{
		BuildTargetID:   request.BuildTargetID,
		PublishTargetID: request.PublishTargetID,
		Enabled:         request.Enabled,
		OptionsJSON:     request.OptionsJSON,
	})
	if err != nil {
		s.writePublishError(w, err)
		return
	}

	if err := writeJSON(w, http.StatusCreated, binding); err != nil {
		s.logger.Error("write build publish binding create response", "error", err)
	}
}

// handleBuildPublishBindingGet returns one build-to-publish binding selected
// by path id.
func (s *Server) handleBuildPublishBindingGet(w http.ResponseWriter, r *http.Request) {
	id, err := resourceIDFromRequest(r, "build publish binding")
	if err != nil {
		s.writeAPIError(w, http.StatusBadRequest, err.Error(), err)
		return
	}

	binding, err := s.publishes.GetBinding(r.Context(), id)
	if err != nil {
		s.writePublishError(w, err)
		return
	}

	if err := writeJSON(w, http.StatusOK, binding); err != nil {
		s.logger.Error("write build publish binding get response", "error", err)
	}
}

// handleBuildPublishBindingUpdate decodes one binding replacement request and
// persists the new values.
func (s *Server) handleBuildPublishBindingUpdate(w http.ResponseWriter, r *http.Request) {
	id, err := resourceIDFromRequest(r, "build publish binding")
	if err != nil {
		s.writeAPIError(w, http.StatusBadRequest, err.Error(), err)
		return
	}

	var request buildPublishBindingUpdateRequest
	if err := decodeJSONRequest(r, &request); err != nil {
		s.writeAPIError(w, http.StatusBadRequest, "invalid JSON body", err)
		return
	}
	if request.Enabled == nil {
		s.writeAPIError(
			w,
			http.StatusBadRequest,
			"enabled must be provided for build publish binding updates",
			publish.ErrInvalid,
		)
		return
	}

	binding, err := s.publishes.UpdateBinding(r.Context(), id, publish.UpdateBindingInput{
		Enabled:     *request.Enabled,
		OptionsJSON: request.OptionsJSON,
	})
	if err != nil {
		s.writePublishError(w, err)
		return
	}

	if err := writeJSON(w, http.StatusOK, binding); err != nil {
		s.logger.Error("write build publish binding update response", "error", err)
	}
}

// handleBuildPublishBindingDelete removes one build-to-publish binding
// selected by path id.
func (s *Server) handleBuildPublishBindingDelete(w http.ResponseWriter, r *http.Request) {
	id, err := resourceIDFromRequest(r, "build publish binding")
	if err != nil {
		s.writeAPIError(w, http.StatusBadRequest, err.Error(), err)
		return
	}

	if err := s.publishes.DeleteBinding(r.Context(), id); err != nil {
		s.writePublishError(w, err)
		return
	}

	if err := writeJSON(w, http.StatusOK, map[string]any{"deleted": true, "id": id}); err != nil {
		s.logger.Error("write build publish binding delete response", "error", err)
	}
}

// writePublishError maps publish target, binding, and execution planning
// failures to HTTP status codes.
func (s *Server) writePublishError(w http.ResponseWriter, err error) {
	switch {
	case errors.Is(err, publish.ErrInvalid):
		s.writeAPIError(w, http.StatusBadRequest, err.Error(), err)
	case errors.Is(err, publish.ErrNotFound),
		errors.Is(err, publish.ErrBindingNotFound),
		errors.Is(err, publish.ErrRepositoryNotFound),
		errors.Is(err, publish.ErrCredentialsNotFound),
		errors.Is(err, publish.ErrBuildTargetNotFound):
		s.writeAPIError(w, http.StatusNotFound, err.Error(), err)
	case errors.Is(err, publish.ErrConflict),
		errors.Is(err, publish.ErrBindingConflict),
		errors.Is(err, publish.ErrRepositoryMismatch):
		s.writeAPIError(w, http.StatusConflict, err.Error(), err)
	default:
		s.writeAPIError(w, http.StatusInternalServerError, "publish request failed", err)
	}
}