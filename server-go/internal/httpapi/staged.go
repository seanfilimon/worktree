package httpapi

import (
	"encoding/json"
	"errors"
	"net/http"
	"time"

	"github.com/ramizik/worktree/server-go/internal/staged"
	"github.com/ramizik/worktree/server-go/internal/storage"
)

type StagedService struct {
	objects storage.ObjectStore
	staged  staged.Store
}

func NewStagedService(objects storage.ObjectStore, stagedStore staged.Store) *StagedService {
	return &StagedService{objects: objects, staged: stagedStore}
}

type stagedUploadRequest struct {
	SnapshotID string               `json:"snapshot_id"`
	Tenant     string               `json:"tenant"`
	Worktree   string               `json:"worktree"`
	TreeID     string               `json:"tree_id"`
	Branch     string               `json:"branch"`
	Objects    []stagedObjectUpload `json:"objects"`
}

type stagedObjectUpload struct {
	Path    string `json:"path"`
	Hash    string `json:"hash"`
	Size    int    `json:"size"`
	Content []byte `json:"content"`
}

func (s *StagedService) HandleUpload(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		writeError(w, http.StatusMethodNotAllowed, "MethodNotAllowed", "method not allowed")
		return
	}
	var req stagedUploadRequest
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
		writeError(w, http.StatusBadRequest, "InvalidJSON", "request body is not valid JSON")
		return
	}
	if err := req.validate(); err != nil {
		writeError(w, http.StatusUnprocessableEntity, "InvalidStagedSnapshot", err.Error())
		return
	}
	objectIDs := make([]string, 0, len(req.Objects))
	for _, obj := range req.Objects {
		if len(obj.Content) != obj.Size {
			writeError(w, http.StatusUnprocessableEntity, "InvalidObject", "object size does not match content length")
			return
		}
		if err := s.objects.Put(r.Context(), obj.Hash, obj.Content); err != nil {
			writeError(w, http.StatusUnprocessableEntity, "InvalidObject", err.Error())
			return
		}
		objectIDs = append(objectIDs, obj.Hash)
	}
	record := staged.Snapshot{
		SnapshotID: req.SnapshotID,
		Tenant:     req.Tenant,
		Worktree:   req.Worktree,
		TreeID:     req.TreeID,
		Branch:     req.Branch,
		ObjectIDs:  objectIDs,
		CreatedAt:  time.Now().UTC(),
	}
	if err := s.staged.Add(r.Context(), record); err != nil {
		writeError(w, http.StatusInternalServerError, "StagedStoreFailed", "failed to persist staged snapshot")
		return
	}
	writeJSON(w, http.StatusAccepted, map[string]any{
		"status":      "staged",
		"snapshot_id": req.SnapshotID,
		"objects":     len(objectIDs),
	})
}

func (r stagedUploadRequest) validate() error {
	if r.SnapshotID == "" {
		return errors.New("snapshot_id is required")
	}
	if r.Tenant == "" {
		return errors.New("tenant is required")
	}
	if r.Worktree == "" {
		return errors.New("worktree is required")
	}
	if r.TreeID == "" {
		return errors.New("tree_id is required")
	}
	if r.Branch == "" {
		return errors.New("branch is required")
	}
	if len(r.Objects) == 0 {
		return errors.New("at least one object is required")
	}
	for _, obj := range r.Objects {
		if obj.Path == "" {
			return errors.New("object path is required")
		}
		if !storage.IsValidHash(obj.Hash) {
			return errors.New("object hash must be a 64-character BLAKE3 hex digest")
		}
		if obj.Size < 0 {
			return errors.New("object size must be non-negative")
		}
	}
	return nil
}

func writeError(w http.ResponseWriter, status int, code string, message string) {
	writeJSON(w, status, map[string]any{
		"error": map[string]string{
			"code":    code,
			"message": message,
		},
	})
}
