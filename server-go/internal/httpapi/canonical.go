package httpapi

import (
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"strings"
	"time"

	"github.com/ramizik/worktree/server-go/internal/audit"
	"github.com/ramizik/worktree/server-go/internal/auth"
	"github.com/ramizik/worktree/server-go/internal/canonical"
	"github.com/ramizik/worktree/server-go/internal/iam"
	"github.com/ramizik/worktree/server-go/internal/storage"
)

// CanonicalService serves the v1 canonical push/pull surface:
//
//	POST /api/push                — promote snapshot chain with CAS
//	POST /api/pull                — fetch snapshots + needed objects
//	POST /api/objects/check       — which hashes is the server missing
//	PUT  /api/objects/{hash}      — upload a single content-addressed blob
//	GET  /api/objects/{hash}      — download a single content-addressed blob
//	GET  /api/refs                — list canonical branch tips
type CanonicalService struct {
	canonical     *canonical.Service
	audit         audit.Recorder
	authorizer    iam.Authorizer
	maxObjectSize int64
}

// CanonicalLimits constrains a single object upload. 64 MiB matches the
// staged path; clients streaming larger blobs must chunk via FastCDC (out
// of scope for v1).
type CanonicalLimits struct {
	MaxObjectBytes int64
}

func DefaultCanonicalLimits() CanonicalLimits {
	return CanonicalLimits{MaxObjectBytes: 64 * 1024 * 1024}
}

func NewCanonicalService(svc *canonical.Service, recorder audit.Recorder, authorizer iam.Authorizer, limits ...CanonicalLimits) *CanonicalService {
	if recorder == nil {
		recorder = audit.NoopRecorder{}
	}
	if authorizer == nil {
		authorizer = iam.NewDefaultPolicyAuthorizer()
	}
	selected := DefaultCanonicalLimits()
	if len(limits) > 0 {
		selected = limits[0]
	}
	return &CanonicalService{
		canonical:     svc,
		audit:         recorder,
		authorizer:    authorizer,
		maxObjectSize: selected.MaxObjectBytes,
	}
}

// --- request/response shapes (JSON over REST) -----------------------------

type pushRequest struct {
	Tenant        string             `json:"tenant"`
	Worktree      string             `json:"worktree"`
	TreeID        string             `json:"tree_id"`
	Branch        string             `json:"branch"`
	ExpectedTip   string             `json:"expected_tip"`
	NewTip        string             `json:"new_tip"`
	SnapshotChain []wireSnapshot     `json:"snapshot_chain"`
	Objects       []wireObjectRef    `json:"objects"`
}

type wireSnapshot struct {
	SnapshotID   string          `json:"snapshot_id"`
	Tenant       string          `json:"tenant"`
	Worktree     string          `json:"worktree"`
	TreeID       string          `json:"tree_id"`
	Branch       string          `json:"branch"`
	Parents      []string        `json:"parents"`
	ManifestHash string          `json:"manifest_hash"`
	Message      string          `json:"message"`
	Author       string          `json:"author"`
	CommittedAt  time.Time       `json:"committed_at"`
	Objects      []wireObjectRef `json:"objects"`
	Payload      json.RawMessage `json:"payload,omitempty"`
}

type wireObjectRef struct {
	Hash string `json:"hash"`
	Path string `json:"path"`
	Size int64  `json:"size"`
}

type pullRequest struct {
	Tenant       string `json:"tenant"`
	Worktree     string `json:"worktree"`
	TreeID       string `json:"tree_id"`
	Branch       string `json:"branch"`
	LastKnownTip string `json:"last_known_tip"`
}

type objectsCheckRequest struct {
	Tenant string   `json:"tenant"`
	Hashes []string `json:"hashes"`
}

// --- handlers -------------------------------------------------------------

func (s *CanonicalService) HandlePush(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		writeError(w, http.StatusMethodNotAllowed, "MethodNotAllowed", "method not allowed")
		return
	}
	var req pushRequest
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
		writeError(w, http.StatusBadRequest, "InvalidJSON", "request body is not valid JSON")
		return
	}
	if err := req.validate(); err != nil {
		s.auditDecision(r, "branch:push", audit.DecisionDeny, err.Error(), req.Tenant, req.resource())
		writeError(w, http.StatusUnprocessableEntity, "InvalidPush", err.Error())
		return
	}
	if !s.enforceTenant(w, r, req.Tenant, "branch:push", req.resource()) {
		return
	}
	if !s.enforceIAM(w, r, "branch:push", req.Tenant, req.resource()) {
		return
	}

	chain := make([]canonical.Snapshot, 0, len(req.SnapshotChain))
	for _, ws := range req.SnapshotChain {
		payload := []byte(ws.Payload)
		if len(payload) == 0 {
			encoded, err := json.Marshal(ws)
			if err != nil {
				writeError(w, http.StatusInternalServerError, "PayloadEncodingFailed", err.Error())
				return
			}
			payload = encoded
		}
		chain = append(chain, canonical.Snapshot{
			SnapshotID:   ws.SnapshotID,
			Tenant:       req.Tenant,
			Worktree:     req.Worktree,
			TreeID:       req.TreeID,
			Branch:       req.Branch,
			Parents:      ws.Parents,
			ManifestHash: ws.ManifestHash,
			Message:      ws.Message,
			Author:       ws.Author,
			CommittedAt:  ws.CommittedAt,
			Payload:      payload,
			Objects:      toCanonicalObjectRefs(ws.Objects),
		})
	}

	result, err := s.canonical.Push(r.Context(), canonical.PushInput{
		Tenant:        req.Tenant,
		Worktree:      req.Worktree,
		TreeID:        req.TreeID,
		Branch:        req.Branch,
		ExpectedTip:   req.ExpectedTip,
		NewTip:        req.NewTip,
		SnapshotChain: chain,
		Objects:       toCanonicalObjectRefs(req.Objects),
	})
	if err != nil {
		s.auditDecision(r, "branch:push", audit.DecisionDeny, err.Error(), req.Tenant, req.resource())
		writeError(w, http.StatusInternalServerError, "PushFailed", err.Error())
		return
	}
	switch result.Status {
	case "missing_objects":
		s.auditDecision(r, "branch:push", audit.DecisionDeny, "missing objects", req.Tenant, req.resource())
		writeJSON(w, http.StatusPreconditionFailed, map[string]any{
			"status":  "missing_objects",
			"missing": result.MissingObjects,
		})
	case "conflict":
		s.auditDecision(r, "branch:push", audit.DecisionDeny, "tip conflict", req.Tenant, req.resource())
		writeJSON(w, http.StatusConflict, map[string]any{
			"status":     "conflict",
			"actual_tip": result.ActualTip,
			"message":    "branch tip moved since expected_tip; pull and retry",
		})
	case "accepted":
		s.auditDecision(r, "branch:push", audit.DecisionAllow, "", req.Tenant, req.resource())
		writeJSON(w, http.StatusOK, map[string]any{
			"status":              "accepted",
			"new_tip":             result.NewTip,
			"snapshots_committed": result.SnapshotsAccepted,
		})
	default:
		writeError(w, http.StatusInternalServerError, "UnknownPushStatus", result.Status)
	}
}

func (s *CanonicalService) HandlePull(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		writeError(w, http.StatusMethodNotAllowed, "MethodNotAllowed", "method not allowed")
		return
	}
	var req pullRequest
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
		writeError(w, http.StatusBadRequest, "InvalidJSON", "request body is not valid JSON")
		return
	}
	if err := req.validate(); err != nil {
		s.auditDecision(r, "branch:pull", audit.DecisionDeny, err.Error(), req.Tenant, req.resource())
		writeError(w, http.StatusUnprocessableEntity, "InvalidPull", err.Error())
		return
	}
	if !s.enforceTenant(w, r, req.Tenant, "branch:pull", req.resource()) {
		return
	}
	if !s.enforceIAM(w, r, "branch:pull", req.Tenant, req.resource()) {
		return
	}

	result, err := s.canonical.Pull(r.Context(), canonical.PullInput{
		Tenant:       req.Tenant,
		Worktree:     req.Worktree,
		TreeID:       req.TreeID,
		Branch:       req.Branch,
		LastKnownTip: req.LastKnownTip,
	})
	if err != nil {
		s.auditDecision(r, "branch:pull", audit.DecisionDeny, err.Error(), req.Tenant, req.resource())
		writeError(w, http.StatusInternalServerError, "PullFailed", err.Error())
		return
	}
	s.auditDecision(r, "branch:pull", audit.DecisionAllow, "", req.Tenant, req.resource())
	writeJSON(w, http.StatusOK, map[string]any{
		"new_tip":          result.NewTip,
		"snapshots":        canonicalSnapshotsToWire(result.Snapshots),
		"objects_needed":   nonNilStrings(result.ObjectsNeeded),
		"up_to_date":       result.UpToDate,
		"branch_not_found": result.BranchNotFound,
	})
}

func (s *CanonicalService) HandleObjectsCheck(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		writeError(w, http.StatusMethodNotAllowed, "MethodNotAllowed", "method not allowed")
		return
	}
	var req objectsCheckRequest
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
		writeError(w, http.StatusBadRequest, "InvalidJSON", "request body is not valid JSON")
		return
	}
	resource := req.Tenant
	if !s.enforceTenant(w, r, req.Tenant, "object:check", resource) {
		return
	}
	if !s.enforceIAM(w, r, "object:check", req.Tenant, resource) {
		return
	}
	missing, err := s.canonical.MissingObjects(r.Context(), req.Hashes)
	if err != nil {
		s.auditDecision(r, "object:check", audit.DecisionDeny, err.Error(), req.Tenant, resource)
		writeError(w, http.StatusUnprocessableEntity, "InvalidObjectCheck", err.Error())
		return
	}
	s.auditDecision(r, "object:check", audit.DecisionAllow, "", req.Tenant, resource)
	writeJSON(w, http.StatusOK, map[string]any{"missing": nonNilStrings(missing)})
}

func (s *CanonicalService) HandleObject(w http.ResponseWriter, r *http.Request) {
	hash := strings.ToLower(r.PathValue("hash"))
	if !storage.IsValidHash(hash) {
		writeError(w, http.StatusBadRequest, "InvalidHash", "hash must be a 64-character BLAKE3 hex digest")
		return
	}
	switch r.Method {
	case http.MethodGet:
		s.handleObjectGet(w, r, hash)
	case http.MethodPut:
		s.handleObjectPut(w, r, hash)
	default:
		writeError(w, http.StatusMethodNotAllowed, "MethodNotAllowed", "method not allowed")
	}
}

func (s *CanonicalService) handleObjectGet(w http.ResponseWriter, r *http.Request, hash string) {
	principal, _ := auth.PrincipalFromContext(r.Context())
	resource := principal.Tenant + "/objects/" + hash
	if !s.enforceIAM(w, r, "object:read", principal.Tenant, resource) {
		return
	}
	data, err := s.canonical.GetObject(r.Context(), hash)
	if err != nil {
		if errors.Is(err, storage.ErrObjectMissing) {
			s.auditDecision(r, "object:read", audit.DecisionDeny, "not found", principal.Tenant, resource)
			writeError(w, http.StatusNotFound, "ObjectNotFound", "object not found")
			return
		}
		s.auditDecision(r, "object:read", audit.DecisionDeny, err.Error(), principal.Tenant, resource)
		writeError(w, http.StatusInternalServerError, "ObjectReadFailed", err.Error())
		return
	}
	s.auditDecision(r, "object:read", audit.DecisionAllow, "", principal.Tenant, resource)
	w.Header().Set("Content-Type", "application/octet-stream")
	w.Header().Set("X-Content-Hash", hash)
	w.WriteHeader(http.StatusOK)
	_, _ = w.Write(data)
}

func (s *CanonicalService) handleObjectPut(w http.ResponseWriter, r *http.Request, hash string) {
	principal, _ := auth.PrincipalFromContext(r.Context())
	resource := principal.Tenant + "/objects/" + hash
	if !s.enforceIAM(w, r, "object:write", principal.Tenant, resource) {
		return
	}
	limit := s.maxObjectSize
	if limit <= 0 {
		limit = 64 * 1024 * 1024
	}
	body := http.MaxBytesReader(w, r.Body, limit)
	data, err := io.ReadAll(body)
	if err != nil {
		s.auditDecision(r, "object:write", audit.DecisionDeny, "read body", principal.Tenant, resource)
		writeError(w, http.StatusRequestEntityTooLarge, "ObjectTooLarge", err.Error())
		return
	}
	if err := s.canonical.PutObject(r.Context(), hash, data); err != nil {
		s.auditDecision(r, "object:write", audit.DecisionDeny, err.Error(), principal.Tenant, resource)
		if errors.Is(err, storage.ErrInvalidHash) {
			writeError(w, http.StatusBadRequest, "InvalidObject", err.Error())
			return
		}
		writeError(w, http.StatusInternalServerError, "ObjectWriteFailed", err.Error())
		return
	}
	s.auditDecision(r, "object:write", audit.DecisionAllow, "", principal.Tenant, resource)
	writeJSON(w, http.StatusCreated, map[string]any{"status": "stored", "hash": hash, "size": len(data)})
}

func (s *CanonicalService) HandleListRefs(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodGet {
		writeError(w, http.StatusMethodNotAllowed, "MethodNotAllowed", "method not allowed")
		return
	}
	tenant := r.URL.Query().Get("tenant")
	worktree := r.URL.Query().Get("worktree")
	treeID := r.URL.Query().Get("tree_id")
	if tenant == "" {
		writeError(w, http.StatusUnprocessableEntity, "InvalidRefList", "tenant query parameter is required")
		return
	}
	resource := tenant + "/" + worktree
	if !s.enforceTenant(w, r, tenant, "ref:list", resource) {
		return
	}
	if !s.enforceIAM(w, r, "ref:list", tenant, resource) {
		return
	}
	branches, err := s.canonical.ListRefs(r.Context(), tenant, worktree, treeID)
	if err != nil {
		s.auditDecision(r, "ref:list", audit.DecisionDeny, err.Error(), tenant, resource)
		writeError(w, http.StatusInternalServerError, "RefListFailed", err.Error())
		return
	}
	s.auditDecision(r, "ref:list", audit.DecisionAllow, "", tenant, resource)
	writeJSON(w, http.StatusOK, map[string]any{"branches": branches, "count": len(branches)})
}

// --- helpers --------------------------------------------------------------

func (s *CanonicalService) enforceTenant(w http.ResponseWriter, r *http.Request, tenant, action, resource string) bool {
	principal, ok := auth.PrincipalFromContext(r.Context())
	if !ok {
		writeError(w, http.StatusUnauthorized, "AuthenticationRequired", "authenticated principal missing")
		return false
	}
	if principal.Tenant != "" && tenant != "" && principal.Tenant != tenant {
		s.auditDecision(r, action, audit.DecisionDeny, "tenant mismatch", tenant, resource)
		writeError(w, http.StatusForbidden, "TenantMismatch", "authenticated tenant does not match request tenant")
		return false
	}
	return true
}

func (s *CanonicalService) enforceIAM(w http.ResponseWriter, r *http.Request, action, tenant, resource string) bool {
	principal, ok := auth.PrincipalFromContext(r.Context())
	if !ok {
		writeError(w, http.StatusUnauthorized, "AuthenticationRequired", "authenticated principal missing")
		return false
	}
	decision, err := s.authorizer.Authorize(r.Context(), principal, action, resource)
	if err != nil || decision == iam.Deny {
		s.auditDecision(r, action, audit.DecisionDeny, "iam denied", tenant, resource)
		writeError(w, http.StatusForbidden, "Forbidden", "access denied")
		return false
	}
	return true
}

func (s *CanonicalService) auditDecision(r *http.Request, action string, decision audit.Decision, reason, tenant, resource string) {
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

func toCanonicalObjectRefs(in []wireObjectRef) []canonical.ObjectRef {
	out := make([]canonical.ObjectRef, 0, len(in))
	for _, o := range in {
		out = append(out, canonical.ObjectRef{Hash: o.Hash, Path: o.Path, Size: o.Size})
	}
	return out
}

func canonicalSnapshotsToWire(in []canonical.Snapshot) []map[string]any {
	out := make([]map[string]any, 0, len(in))
	for _, snap := range in {
		entry := map[string]any{
			"snapshot_id":   snap.SnapshotID,
			"tenant":        snap.Tenant,
			"worktree":      snap.Worktree,
			"tree_id":       snap.TreeID,
			"branch":        snap.Branch,
			"parents":       snap.Parents,
			"manifest_hash": snap.ManifestHash,
			"message":       snap.Message,
			"author":        snap.Author,
			"committed_at":  snap.CommittedAt,
			"objects":       snap.Objects,
		}
		if len(snap.Payload) > 0 {
			entry["payload"] = json.RawMessage(snap.Payload)
		}
		out = append(out, entry)
	}
	return out
}

func nonNilStrings(in []string) []string {
	if in == nil {
		return []string{}
	}
	return in
}

// --- request validation ---------------------------------------------------

func (r pushRequest) validate() error {
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
	if r.NewTip == "" {
		return errors.New("new_tip is required")
	}
	if len(r.SnapshotChain) == 0 {
		return errors.New("snapshot_chain must contain at least one snapshot")
	}
	for _, snap := range r.SnapshotChain {
		if snap.SnapshotID == "" {
			return errors.New("each snapshot must have snapshot_id")
		}
		for _, obj := range snap.Objects {
			if !storage.IsValidHash(obj.Hash) {
				return errors.New("object hash must be a 64-character BLAKE3 hex digest")
			}
		}
	}
	for _, obj := range r.Objects {
		if !storage.IsValidHash(obj.Hash) {
			return errors.New("object hash must be a 64-character BLAKE3 hex digest")
		}
	}
	return nil
}

func (r pushRequest) resource() string {
	return r.Tenant + "/" + r.Worktree + "/" + r.TreeID + "/branches/" + r.Branch
}

func (r pullRequest) validate() error {
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
	return nil
}

func (r pullRequest) resource() string {
	return r.Tenant + "/" + r.Worktree + "/" + r.TreeID + "/branches/" + r.Branch
}
