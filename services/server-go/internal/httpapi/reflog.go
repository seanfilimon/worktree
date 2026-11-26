package httpapi

// v0.8 history-v2 (docs/ENGINEERING_PLAN.md §16.10): the server reflog of
// branch moves (pushes, force pushes and restores), recorded by
// internal/syncsvc, and the restore of a branch to a tip of its reflog
// (specs/tree/Undo.md §4.1).

import (
	"context"
	"errors"
	"net/http"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/syncsvc"
)

// BranchRestorer moves a branch to a tip of its server reflog
// (implemented by *syncsvc.Service, which enforces permissions, branch
// protection and the lease, records the reflog, events and audit).
type BranchRestorer interface {
	RestoreBranch(ctx context.Context, p *domain.Principal, t *domain.Tree, req syncsvc.RestoreRequest) (*syncsvc.RestoreResult, error)
}

const (
	defaultReflogLimit = 50
	maxReflogLimit     = 500
)

// reflogResponse is the body of GET {tree}/reflog.
type reflogResponse struct {
	Entries []syncsvc.ReflogEntry `json:"entries"`
}

// registerReflogRoutes registers GET {tree}/reflog and POST
// {tree}/reflog/restore.
func (a *api) registerReflogRoutes(tree string) {
	a.handle("GET "+tree+"/reflog", a.handleReflog)
	a.handle("POST "+tree+"/reflog/restore", a.handleReflogRestore)
}

// handleReflog answers GET {tree}/reflog?branch=&limit=50: the branch moves
// of the tree (one branch when given), newest first. Force pushes have
// kind "force_push" and keep the rewound tip in old_tip.
func (a *api) handleReflog(w http.ResponseWriter, r *http.Request) error {
	_, tree, err := a.treeFor(r, domain.ActTreeRead)
	if err != nil {
		return err
	}
	branch := r.URL.Query().Get("branch")
	if len(branch) > maxRefNameLen {
		return badRequestf("branch must be at most %d bytes", maxRefNameLen)
	}
	limit, err := queryInt(r, "limit", defaultReflogLimit, maxReflogLimit)
	if err != nil {
		return err
	}
	entries, err := syncsvc.ListReflog(r.Context(), a.d.Store, tree.ID, branch, limit)
	if err != nil {
		return err
	}
	if entries == nil {
		entries = []syncsvc.ReflogEntry{}
	}
	writeJSON(w, http.StatusOK, reflogResponse{Entries: entries})
	return nil
}

// restoreRequest is the body of POST {tree}/reflog/restore.
type restoreRequest struct {
	Branch string `json:"branch"`
	// Tip is the snapshot to restore (old_tip or new_tip of a reflog entry
	// of the branch).
	Tip string `json:"tip"`
	// ExpectedTip is the branch tip the caller saw (lease).
	ExpectedTip string `json:"expected_tip"`
	// Bypass asks to bypass branch protection (branch:bypass_protection).
	Bypass bool `json:"bypass,omitempty"`
}

// handleReflogRestore answers POST {tree}/reflog/restore: move a branch to
// a tip recorded in its reflog. It needs branch:force_push (rewinds follow
// the force-push protections, forward moves the direct-push ones) and
// answers 409 with error.details {current_tip, expected_tip} when the
// branch is no longer at expected_tip.
func (a *api) handleReflogRestore(w http.ResponseWriter, r *http.Request) error {
	p, err := requireLogin(r)
	if err != nil {
		return err
	}
	_, tree, err := a.treeFor(r, domain.ActTreeRead)
	if err != nil {
		return err
	}
	if a.d.Reflog == nil {
		return &apiError{status: http.StatusNotImplemented, code: "not_implemented",
			msg: "restoring branches from the reflog is not available on this server"}
	}
	var req restoreRequest
	if err := decodeJSON(w, r, &req); err != nil {
		return err
	}
	if req.Branch == "" || req.Tip == "" || req.ExpectedTip == "" {
		return badRequestf("branch, tip and expected_tip are required")
	}
	if len(req.Branch) > maxRefNameLen {
		return badRequestf("branch must be at most %d bytes", maxRefNameLen)
	}
	res, err := a.d.Reflog.RestoreBranch(r.Context(), p, tree, syncsvc.RestoreRequest{
		Branch:      req.Branch,
		Tip:         req.Tip,
		ExpectedTip: req.ExpectedTip,
		Bypass:      req.Bypass,
		RemoteAddr:  remoteHost(r),
	})
	if err != nil {
		var conflict *syncsvc.RestoreConflict
		if errors.As(err, &conflict) {
			return &apiError{status: http.StatusConflict, code: codeConflict, msg: conflict.Error(),
				details: map[string]string{"current_tip": conflict.CurrentTip, "expected_tip": conflict.ExpectedTip}}
		}
		return err
	}
	if res.Bypassed == nil {
		res.Bypassed = []string{}
	}
	writeJSON(w, http.StatusOK, res)
	return nil
}
