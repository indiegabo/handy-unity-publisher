package app

import (
	"errors"
	"fmt"
	"io"
	"net/http"
	"os"
	"path/filepath"

	internaldb "github.com/indiegabo/handy-unity-bulder/internal/db"
)

// handleDatabaseExport streams one consistent SQLite snapshot to the operator.
func (s *Server) handleDatabaseExport(w http.ResponseWriter, r *http.Request) {
	snapshotPath, err := internaldb.CreateSnapshot(r.Context(), s.cfg.DBPath())
	if err != nil {
		s.writeDatabaseAdminError(w, err)
		return
	}
	defer os.Remove(snapshotPath)

	snapshot, err := os.Open(snapshotPath)
	if err != nil {
		s.writeAPIError(w, http.StatusInternalServerError, "open database snapshot failed", err)
		return
	}
	defer snapshot.Close()

	info, err := snapshot.Stat()
	if err != nil {
		s.writeAPIError(w, http.StatusInternalServerError, "stat database snapshot failed", err)
		return
	}

	w.Header().Set("Content-Type", "application/octet-stream")
	w.Header().Set(
		"Content-Disposition",
		fmt.Sprintf("attachment; filename=%q", filepath.Base(s.cfg.DBPath())),
	)
	w.Header().Set("Content-Length", fmt.Sprintf("%d", info.Size()))
	w.WriteHeader(http.StatusOK)

	if _, err := io.Copy(w, snapshot); err != nil {
		s.logger.Error("write database export response", "error", err)
	}
}

// handleDatabaseImport replaces the runtime SQLite file with one validated snapshot.
func (s *Server) handleDatabaseImport(w http.ResponseWriter, r *http.Request) {
	defer r.Body.Close()

	if err := internaldb.ReplaceWithSnapshot(r.Context(), s.cfg.DBPath(), r.Body); err != nil {
		s.writeDatabaseAdminError(w, err)
		return
	}

	if err := writeJSON(w, http.StatusAccepted, map[string]any{
		"imported":         true,
		"path":             s.cfg.DBPath(),
		"restart_required": true,
	}); err != nil {
		s.logger.Error("write database import response", "error", err)
	}
}

// writeDatabaseAdminError maps snapshot validation failures to operator-facing status codes.
func (s *Server) writeDatabaseAdminError(w http.ResponseWriter, err error) {
	switch {
	case errors.Is(err, internaldb.ErrInvalidSnapshot):
		s.writeAPIError(w, http.StatusBadRequest, err.Error(), err)
	default:
		s.writeAPIError(w, http.StatusInternalServerError, "database admin request failed", err)
	}
}
