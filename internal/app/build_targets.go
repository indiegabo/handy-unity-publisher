package app

import (
	"errors"
	"net/http"

	"github.com/indiegabo/handy-unity-bulder/internal/build"
)

// buildTargetCreateRequest describes the JSON body accepted when creating one
// build target through the HTTP API.
type buildTargetCreateRequest struct {
	RepositoryID         int64  `json:"repository_id"`
	Name                 string `json:"name"`
	Platform             string `json:"platform"`
	RunnerType           string `json:"runner_type"`
	BuildMethod          string `json:"build_method"`
	OutputKind           string `json:"output_kind"`
	OutputPathTemplate   string `json:"output_path_template"`
	UnityVersionOverride string `json:"unity_version_override"`
	ImageOverride        string `json:"image_override"`
	TimeoutSeconds       int    `json:"timeout_seconds"`
	Enabled              *bool  `json:"enabled,omitempty"`
	ConfigJSON           string `json:"config_json"`
}

// buildTargetUpdateRequest describes the JSON body accepted when replacing
// one build target through the HTTP API.
type buildTargetUpdateRequest struct {
	Name                 string `json:"name"`
	Platform             string `json:"platform"`
	RunnerType           string `json:"runner_type"`
	BuildMethod          string `json:"build_method"`
	OutputKind           string `json:"output_kind"`
	OutputPathTemplate   string `json:"output_path_template"`
	UnityVersionOverride string `json:"unity_version_override"`
	ImageOverride        string `json:"image_override"`
	TimeoutSeconds       int    `json:"timeout_seconds"`
	Enabled              *bool  `json:"enabled"`
	ConfigJSON           string `json:"config_json"`
}

// handleBuildTargetList returns every build target for one repository.
func (s *Server) handleBuildTargetList(w http.ResponseWriter, r *http.Request) {
	repositoryID, err := int64QueryParam(r, "repository_id")
	if err != nil {
		s.writeAPIError(w, http.StatusBadRequest, err.Error(), err)
		return
	}

	targets, err := s.builds.ListTargetsByRepository(r.Context(), repositoryID)
	if err != nil {
		s.writeBuildError(w, err)
		return
	}

	if err := writeJSON(w, http.StatusOK, targets); err != nil {
		s.logger.Error("write build target list response", "error", err)
	}
}

// handleBuildTargetCreate decodes one build target create request and
// persists it.
func (s *Server) handleBuildTargetCreate(w http.ResponseWriter, r *http.Request) {
	var request buildTargetCreateRequest
	if err := decodeJSONRequest(r, &request); err != nil {
		s.writeAPIError(w, http.StatusBadRequest, "invalid JSON body", err)
		return
	}

	target, err := s.builds.CreateTarget(r.Context(), build.CreateTargetInput{
		RepositoryID:         request.RepositoryID,
		Name:                 request.Name,
		Platform:             request.Platform,
		RunnerType:           request.RunnerType,
		BuildMethod:          request.BuildMethod,
		OutputKind:           request.OutputKind,
		OutputPathTemplate:   request.OutputPathTemplate,
		UnityVersionOverride: request.UnityVersionOverride,
		ImageOverride:        request.ImageOverride,
		TimeoutSeconds:       request.TimeoutSeconds,
		Enabled:              request.Enabled,
		ConfigJSON:           request.ConfigJSON,
	})
	if err != nil {
		s.writeBuildError(w, err)
		return
	}

	if err := writeJSON(w, http.StatusCreated, target); err != nil {
		s.logger.Error("write build target create response", "error", err)
	}
}

// handleBuildTargetGet returns one build target selected by path id.
func (s *Server) handleBuildTargetGet(w http.ResponseWriter, r *http.Request) {
	id, err := resourceIDFromRequest(r, "build target")
	if err != nil {
		s.writeAPIError(w, http.StatusBadRequest, err.Error(), err)
		return
	}

	target, err := s.builds.GetTarget(r.Context(), id)
	if err != nil {
		s.writeBuildError(w, err)
		return
	}

	if err := writeJSON(w, http.StatusOK, target); err != nil {
		s.logger.Error("write build target get response", "error", err)
	}
}

// handleBuildTargetUpdate decodes one build target replacement request and
// persists the new values.
func (s *Server) handleBuildTargetUpdate(w http.ResponseWriter, r *http.Request) {
	id, err := resourceIDFromRequest(r, "build target")
	if err != nil {
		s.writeAPIError(w, http.StatusBadRequest, err.Error(), err)
		return
	}

	var request buildTargetUpdateRequest
	if err := decodeJSONRequest(r, &request); err != nil {
		s.writeAPIError(w, http.StatusBadRequest, "invalid JSON body", err)
		return
	}
	if request.Enabled == nil {
		s.writeAPIError(
			w,
			http.StatusBadRequest,
			"enabled must be provided for build target updates",
			build.ErrInvalid,
		)
		return
	}

	target, err := s.builds.UpdateTarget(r.Context(), id, build.UpdateTargetInput{
		Name:                 request.Name,
		Platform:             request.Platform,
		RunnerType:           request.RunnerType,
		BuildMethod:          request.BuildMethod,
		OutputKind:           request.OutputKind,
		OutputPathTemplate:   request.OutputPathTemplate,
		UnityVersionOverride: request.UnityVersionOverride,
		ImageOverride:        request.ImageOverride,
		TimeoutSeconds:       request.TimeoutSeconds,
		Enabled:              *request.Enabled,
		ConfigJSON:           request.ConfigJSON,
	})
	if err != nil {
		s.writeBuildError(w, err)
		return
	}

	if err := writeJSON(w, http.StatusOK, target); err != nil {
		s.logger.Error("write build target update response", "error", err)
	}
}

// handleBuildTargetDelete removes one build target selected by path id.
func (s *Server) handleBuildTargetDelete(w http.ResponseWriter, r *http.Request) {
	id, err := resourceIDFromRequest(r, "build target")
	if err != nil {
		s.writeAPIError(w, http.StatusBadRequest, err.Error(), err)
		return
	}

	if err := s.builds.DeleteTarget(r.Context(), id); err != nil {
		s.writeBuildError(w, err)
		return
	}

	if err := writeJSON(w, http.StatusOK, map[string]any{"deleted": true, "id": id}); err != nil {
		s.logger.Error("write build target delete response", "error", err)
	}
}

// writeBuildError maps build target store failures to HTTP status codes.
func (s *Server) writeBuildError(w http.ResponseWriter, err error) {
	switch {
	case errors.Is(err, build.ErrInvalid):
		s.writeAPIError(w, http.StatusBadRequest, err.Error(), err)
	case errors.Is(err, build.ErrNotFound),
		errors.Is(err, build.ErrRepositoryNotFound):
		s.writeAPIError(w, http.StatusNotFound, err.Error(), err)
	case errors.Is(err, build.ErrConflict):
		s.writeAPIError(w, http.StatusConflict, err.Error(), err)
	default:
		s.writeAPIError(w, http.StatusInternalServerError, "build target request failed", err)
	}
}