package httpapi

import (
	"bytes"
	"context"
	"errors"
	"io"
	"net/http"
	"strconv"

	"github.com/seanfilimon/worktree/services/server-go/internal/backup"
)

// Server backups (docs/ENGINEERING_PLAN.md §15.2). Server admins only.

// registerBackupRoutes registers the v0.7 routes for server backups (§15.2).
func (a *api) registerBackupRoutes() {
	a.handle("POST /api/v1/admin/backups", a.handleCreateBackup)
	a.handle("GET /api/v1/admin/backups", a.handleListBackups)
	a.handle("GET /api/v1/admin/backups/{id}/download", a.handleDownloadBackup)
	a.handle("DELETE /api/v1/admin/backups/{id}", a.handleDeleteBackup)
}

// CreateBackupRequest is the (optional) body of POST /api/v1/admin/backups.
type CreateBackupRequest struct {
	// Note is a free-form description stored with the backup.
	Note string `json:"note,omitempty"`
}

// maxBackupNote bounds CreateBackupRequest.Note.
const maxBackupNote = 500

func backupErr(err error) error {
	switch {
	case errors.Is(err, backup.ErrRunning):
		return &apiError{status: http.StatusConflict, code: "backup_running", msg: "a backup is already running"}
	case errors.Is(err, backup.ErrUnsupported):
		return errNotImplemented("backups")
	}
	return err
}

// handleCreateBackup answers POST /api/v1/admin/backups: writes a backup
// synchronously and returns its BackupInfo (201).
func (a *api) handleCreateBackup(w http.ResponseWriter, r *http.Request) error {
	if err := a.requireServerAdmin(r); err != nil {
		return err
	}
	if a.d.Backup == nil {
		return errNotImplemented("backups")
	}
	// The body is optional: empty is the same as {}.
	var req CreateBackupRequest
	body, err := io.ReadAll(http.MaxBytesReader(w, r.Body, 16<<10))
	if err != nil {
		return badRequestf("request body is too large")
	}
	if len(bytes.TrimSpace(body)) > 0 {
		r.Body = io.NopCloser(bytes.NewReader(body))
		if err := decodeJSON(w, r, &req); err != nil {
			return err
		}
	}
	if len(req.Note) > maxBackupNote {
		return badRequestf("note must be at most %d bytes", maxBackupNote)
	}
	by := ""
	if p := principal(r); p != nil {
		by = p.Username
	}
	// The backup outlives a client that disconnects.
	info, err := a.d.Backup.Create(context.WithoutCancel(r.Context()), by, req.Note)
	if err != nil {
		return backupErr(err)
	}
	a.audit(r, backup.ActionCreate, "", "backup", info.ID, map[string]string{
		"objects": strconv.FormatInt(info.Objects, 10), "size_bytes": strconv.FormatInt(info.SizeBytes, 10), "note": info.Note})
	writeJSON(w, http.StatusCreated, info)
	return nil
}

// handleListBackups answers GET /api/v1/admin/backups.
func (a *api) handleListBackups(w http.ResponseWriter, r *http.Request) error {
	if err := a.requireServerAdmin(r); err != nil {
		return err
	}
	if a.d.Backup == nil {
		return errNotImplemented("backups")
	}
	st, err := a.d.Backup.Status(r.Context())
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusOK, st)
	return nil
}

// handleDownloadBackup answers GET /api/v1/admin/backups/{id}/download
// with the .wtbak archive (Range requests supported).
func (a *api) handleDownloadBackup(w http.ResponseWriter, r *http.Request) error {
	if err := a.requireServerAdmin(r); err != nil {
		return err
	}
	if a.d.Backup == nil {
		return errNotImplemented("backups")
	}
	f, info, err := a.d.Backup.Open(r.Context(), r.PathValue("id"))
	if err != nil {
		return err
	}
	defer f.Close()
	fi, err := f.Stat()
	if err != nil {
		return err
	}
	a.audit(r, "backup.download", "", "backup", info.ID, nil)
	w.Header().Set("Content-Type", "application/octet-stream")
	w.Header().Set("Content-Disposition", `attachment; filename="`+info.ID+backup.FileExt+`"`)
	w.Header().Set("X-Content-Type-Options", "nosniff")
	http.ServeContent(w, r, info.ID+backup.FileExt, fi.ModTime(), f)
	return nil
}

// handleDeleteBackup answers DELETE /api/v1/admin/backups/{id} (204).
func (a *api) handleDeleteBackup(w http.ResponseWriter, r *http.Request) error {
	if err := a.requireServerAdmin(r); err != nil {
		return err
	}
	if a.d.Backup == nil {
		return errNotImplemented("backups")
	}
	id := r.PathValue("id")
	if err := a.d.Backup.Delete(r.Context(), id); err != nil {
		return err
	}
	a.audit(r, backup.ActionDelete, "", "backup", id, nil)
	w.WriteHeader(http.StatusNoContent)
	return nil
}
