package httpapi

import (
	"encoding/json"
	"errors"
	"net/http"
	"time"

	"github.com/ramizik/worktree/server-go/internal/audit"
	"github.com/ramizik/worktree/server-go/internal/auth"
	"github.com/ramizik/worktree/server-go/internal/iam"
	"github.com/ramizik/worktree/server-go/internal/staged"
	"github.com/ramizik/worktree/server-go/internal/storage"
)

type StagedService struct {
	objects     storage.ObjectStore
	staged      staged.Store
	audit       audit.Recorder
	limits      StagedLimits
	authorizer  iam.Authorizer
	broadcaster StagedBroadcaster
}

type StagedLimits struct {
	MaxObjectBytes int
	MaxObjects     int
}

func DefaultStagedLimits() StagedLimits {
	return StagedLimits{
		MaxObjectBytes: 64 * 1024 * 1024,
		MaxObjects:     1024,
	}
}

func NewStagedService(objects storage.ObjectStore, stagedStore staged.Store, recorder audit.Recorder, authorizer iam.Authorizer, limits ...StagedLimits) *StagedService {
	if recorder == nil {
		recorder = audit.NoopRecorder{}
	}
	if authorizer == nil {
		authorizer = iam.NewDefaultPolicyAuthorizer()
	}
	selectedLimits := DefaultStagedLimits()
	if len(limits) > 0 {
		selectedLimits = limits[0]
	}
	return &StagedService{objects: objects, staged: stagedStore, audit: recorder, authorizer: authorizer, broadcaster: NewInMemoryHub(), limits: selectedLimits}
}

func (s *StagedService) Broadcaster() StagedBroadcaster {
	return s.broadcaster
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
		s.auditDecision(r, "staged:create", audit.DecisionDeny, "method not allowed", "", "")
		writeError(w, http.StatusMethodNotAllowed, "MethodNotAllowed", "method not allowed")
		return
	}
	var req stagedUploadRequest
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
		s.auditDecision(r, "staged:create", audit.DecisionDeny, "invalid json", "", "")
		writeError(w, http.StatusBadRequest, "InvalidJSON", "request body is not valid JSON")
		return
	}
	if err := req.validate(); err != nil {
		s.auditDecision(r, "staged:create", audit.DecisionDeny, err.Error(), req.Tenant, req.resource())
		writeError(w, http.StatusUnprocessableEntity, "InvalidStagedSnapshot", err.Error())
		return
	}
	if principal, ok := auth.PrincipalFromContext(r.Context()); ok && principal.Tenant != "" && principal.Tenant != req.Tenant {
		if principal.AuthMethod != "static_dev" && !principal.HasScope("*") {
			s.auditDecision(r, "staged:create", audit.DecisionDeny, "tenant mismatch", req.Tenant, req.resource())
			writeError(w, http.StatusForbidden, "TenantMismatch", "authenticated tenant does not match staged snapshot tenant")
			return
		}
	}
	if principal, ok := auth.PrincipalFromContext(r.Context()); ok {
		decision, err := s.authorizer.Authorize(r.Context(), principal, "staged:create", req.resource())
		if err != nil || decision == iam.Deny {
			s.auditDecision(r, "staged:create", audit.DecisionDeny, "iam denied", req.Tenant, req.resource())
			writeError(w, http.StatusForbidden, "Forbidden", "access denied")
			return
		}
	}
	if err := s.enforceUploadLimits(req); err != nil {
		s.auditDecision(r, "staged:create", audit.DecisionDeny, err.Error(), req.Tenant, req.resource())
		writeError(w, http.StatusRequestEntityTooLarge, "StagedUploadTooLarge", err.Error())
		return
	}
	objectIDs := make([]string, 0, len(req.Objects))
	objectRefs := make([]staged.ObjectRef, 0, len(req.Objects))
	for _, obj := range req.Objects {
		if len(obj.Content) != obj.Size {
			s.auditDecision(r, "staged:create", audit.DecisionDeny, "object size mismatch", req.Tenant, req.resource())
			writeError(w, http.StatusUnprocessableEntity, "InvalidObject", "object size does not match content length")
			return
		}
		if err := s.objects.Put(r.Context(), obj.Hash, obj.Content); err != nil {
			s.auditDecision(r, "staged:create", audit.DecisionDeny, "object verification failed", req.Tenant, req.resource())
			writeError(w, http.StatusUnprocessableEntity, "InvalidObject", err.Error())
			return
		}
		objectIDs = append(objectIDs, obj.Hash)
		objectRefs = append(objectRefs, staged.ObjectRef{Path: obj.Path, Hash: obj.Hash, Size: obj.Size})
	}
	record := staged.Snapshot{
		SnapshotID: req.SnapshotID,
		Tenant:     req.Tenant,
		Worktree:   req.Worktree,
		TreeID:     req.TreeID,
		Branch:     req.Branch,
		ObjectIDs:  objectIDs,
		Objects:    objectRefs,
		CreatedAt:  time.Now().UTC(),
	}
	result, err := s.staged.Add(r.Context(), record)
	if err != nil {
		if errors.Is(err, staged.ErrConflict) {
			s.auditDecision(r, "staged:create", audit.DecisionDeny, "staged idempotency conflict", req.Tenant, req.resource())
			writeError(w, http.StatusConflict, "StagedConflict", "staged snapshot conflicts with an existing snapshot for the same identity")
			return
		}
		s.auditDecision(r, "staged:create", audit.DecisionDeny, "staged persistence failed: "+err.Error(), req.Tenant, req.resource())
		writeError(w, http.StatusInternalServerError, "StagedStoreFailed", "failed to persist staged snapshot: "+err.Error())
		return
	}
	s.auditDecision(r, "staged:create", audit.DecisionAllow, "", req.Tenant, req.resource())

	if s.broadcaster != nil {
		s.broadcaster.Publish(result.Snapshot)
	}

	status := http.StatusAccepted
	if result.IdempotentReplay() {
		status = http.StatusOK
	}
	writeJSON(w, status, map[string]any{
		"status":      "staged",
		"snapshot_id": req.SnapshotID,
		"objects":     len(objectIDs),
		"idempotent":  result.IdempotentReplay(),
	})
}

func (s *StagedService) HandleList(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodGet {
		s.auditDecision(r, "staged:list", audit.DecisionDeny, "method not allowed", "", "")
		writeError(w, http.StatusMethodNotAllowed, "MethodNotAllowed", "method not allowed")
		return
	}
	filter := staged.ListFilter{
		Tenant:   r.URL.Query().Get("tenant"),
		Worktree: r.URL.Query().Get("worktree"),
		Branch:   r.URL.Query().Get("branch"),
	}
	if principal, ok := auth.PrincipalFromContext(r.Context()); ok && principal.Tenant != "" {
		if filter.Tenant != "" && filter.Tenant != principal.Tenant {
			s.auditDecision(r, "staged:list", audit.DecisionDeny, "tenant mismatch", filter.Tenant, "staged")
			writeError(w, http.StatusForbidden, "TenantMismatch", "authenticated tenant does not match requested tenant")
			return
		}
		filter.Tenant = principal.Tenant
	}
	resource := stagedListResource(filter)
	if principal, ok := auth.PrincipalFromContext(r.Context()); ok {
		decision, err := s.authorizer.Authorize(r.Context(), principal, "staged:list", resource)
		if err != nil || decision == iam.Deny {
			s.auditDecision(r, "staged:list", audit.DecisionDeny, "iam denied", filter.Tenant, resource)
			writeError(w, http.StatusForbidden, "Forbidden", "access denied")
			return
		}
	}
	snapshots, err := s.staged.List(r.Context(), filter)
	if err != nil {
		s.auditDecision(r, "staged:list", audit.DecisionDeny, "staged list failed", filter.Tenant, resource)
		writeError(w, http.StatusInternalServerError, "StagedListFailed", "failed to list staged snapshots")
		return
	}
	s.auditDecision(r, "staged:list", audit.DecisionAllow, "", filter.Tenant, resource)
	writeJSON(w, http.StatusOK, map[string]any{
		"snapshots": snapshots,
		"count":     len(snapshots),
	})
}

func (r stagedUploadRequest) resource() string {
	return r.Tenant + "/" + r.Worktree + "/" + r.Branch + "/" + r.SnapshotID
}

func stagedListResource(filter staged.ListFilter) string {
	if filter.Tenant == "" {
		return "staged"
	}
	return filter.Tenant + "/" + filter.Worktree + "/" + filter.Branch + "/*"
}

func (s *StagedService) enforceUploadLimits(req stagedUploadRequest) error {
	if s.limits.MaxObjects > 0 && len(req.Objects) > s.limits.MaxObjects {
		return errors.New("staged upload object count exceeds configured limit")
	}
	if s.limits.MaxObjectBytes > 0 {
		for _, obj := range req.Objects {
			if obj.Size > s.limits.MaxObjectBytes || len(obj.Content) > s.limits.MaxObjectBytes {
				return errors.New("staged upload object size exceeds configured limit")
			}
		}
	}
	return nil
}

func (s *StagedService) auditDecision(r *http.Request, action string, decision audit.Decision, reason string, tenant string, resource string) {
	principal, _ := auth.PrincipalFromContext(r.Context())
	if tenant == "" {
		tenant = principal.Tenant
	}
	_ = s.audit.Record(r.Context(), audit.Event{
		Event:      "access_decision",
		Action:     action,
		Decision:   decision,
		Reason:     reason,
		Tenant:     tenant,
		Account:    principal.Account,
		TokenID:    principal.TokenID,
		AuthMethod: principal.AuthMethod,
		Resource:   resource,
		RequestID:  RequestIDFromContext(r.Context()),
		HTTPMethod: r.Method,
		HTTPPath:   r.URL.Path,
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
	for _, obj := range r.Objects {
		if !staged.IsValidRelativePath(obj.Path) {
			return errors.New("object path must be a relative path without parent traversal")
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
