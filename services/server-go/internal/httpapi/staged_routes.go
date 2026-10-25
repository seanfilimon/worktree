package httpapi

import (
	"errors"
	"net/http"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/staged"
)

// registerStagedRoutes registers the v0.5 staged-snapshot routes
// (DELETE {tree}/staged, docs/ENGINEERING_PLAN.md §13.11).
func (a *api) registerStagedRoutes(tree string) {
	a.handle("DELETE "+tree+"/staged", a.handleUnstage)
}

// unstageResponse is the body of DELETE {tree}/staged.
type unstageResponse struct {
	Removed []domain.StagedSnapshot `json:"removed"`
}

// handleUnstage answers DELETE /api/v1/trees/{tenant}/{tree}/staged?branch=&account=.
//
// It removes the caller's staged snapshot on branch (every branch when
// branch is omitted). account= (a username or account id) removes another
// account's entries and requires tree:admin. The removed entries are
// returned; removing nothing is not an error.
func (a *api) handleUnstage(w http.ResponseWriter, r *http.Request) error {
	u, ok := a.d.Staged.(staged.Unstager)
	if a.d.Staged == nil || !ok {
		return &apiError{status: http.StatusNotImplemented, code: "not_implemented", msg: "unstaging is not enabled on this server"}
	}
	p, err := requireLogin(r)
	if err != nil {
		return err
	}
	t, tree, err := a.treeFor(r, domain.ActTreeRead)
	if err != nil {
		return err
	}
	q := r.URL.Query()
	branch := q.Get("branch")
	if branch != "" {
		if err := validateBranchName(branch); err != nil {
			return err
		}
	}
	accountID := ""
	if ref := q.Get("account"); ref != "" {
		accountID, err = a.resolveAccountRef(r, ref)
		if err != nil {
			return err
		}
	}
	removed, err := u.Unstage(r.Context(), p, tree, accountID, branch)
	if err != nil {
		return err
	}
	for _, s := range removed {
		a.audit(r, "staged.unstage", t.ID, "staged", tree.ID+"/"+s.BranchName, map[string]string{
			"tree":     tree.Name,
			"branch":   s.BranchName,
			"author":   s.Author,
			"snapshot": s.SnapshotID,
		})
	}
	for i := range removed {
		removed[i].FilesChanged = nonNil(removed[i].FilesChanged)
	}
	writeJSON(w, http.StatusOK, unstageResponse{Removed: nonNil(removed)})
	return nil
}

// resolveAccountRef maps a username (or, failing that, an account id) to
// an account id.
func (a *api) resolveAccountRef(r *http.Request, ref string) (string, error) {
	ctx := r.Context()
	acct, err := a.d.Store.GetAccountByUsername(ctx, ref)
	if err == nil {
		return acct.ID, nil
	}
	if !errors.Is(err, domain.ErrNotFound) {
		return "", err
	}
	if acct, err := a.d.Store.GetAccount(ctx, ref); err == nil {
		return acct.ID, nil
	} else if !errors.Is(err, domain.ErrNotFound) {
		return "", err
	}
	return "", notFoundf("account %q not found", ref)
}
