package httpapi

import (
	"context"
	"errors"
	"net/http"
	"sort"
	"strconv"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

const (
	defaultAuditLimit = 100
	maxAuditLimit     = 1000
)

// objectStatter is implemented by object stores that can report their size.
type objectStatter interface {
	Stats(ctx context.Context) (count, bytes int64, err error)
}

// handleAudit answers GET /api/v1/audit?tenant=&action=&limit=&since=.
// With a tenant the caller needs tenant:admin on it; without one the caller
// must be a server admin.
func (a *api) handleAudit(w http.ResponseWriter, r *http.Request) error {
	p, err := requireLogin(r)
	if err != nil {
		return err
	}
	ctx := r.Context()
	q := r.URL.Query()
	var f domain.AuditFilter
	if ref := q.Get("tenant"); ref != "" {
		t, err := a.tenantByRef(ctx, ref)
		if err != nil {
			return err
		}
		if err := a.d.Authz.Authorize(ctx, p, domain.ActTenantAdmin, t.ID, nil); err != nil {
			return err
		}
		f.TenantID = t.ID
	} else if err := a.d.Authz.Authorize(ctx, p, domain.ActServerAdmin, "", nil); err != nil {
		return err
	}
	f.Action = q.Get("action")
	if f.Limit, err = queryInt(r, "limit", defaultAuditLimit, maxAuditLimit); err != nil {
		return err
	}
	if s := q.Get("since"); s != "" {
		if f.Since, err = time.Parse(time.RFC3339, s); err != nil {
			return badRequestf("since must be an RFC 3339 timestamp")
		}
	}
	if a.d.Audit == nil {
		writeJSON(w, http.StatusOK, []domain.AuditEvent{})
		return nil
	}
	events, err := a.d.Audit.Query(ctx, f)
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusOK, nonNil(events))
	return nil
}

// computeStats gathers ServerStats from the store and object store.
func (a *api) computeStats(ctx context.Context) (domain.ServerStats, error) {
	var st domain.ServerStats
	tenants, err := a.d.Store.ListTenants(ctx)
	if err != nil {
		return st, err
	}
	accounts, err := a.d.Store.ListAccounts(ctx)
	if err != nil {
		return st, err
	}
	trees, err := a.d.Store.ListTrees(ctx, "")
	if err != nil {
		return st, err
	}
	st.Tenants, st.Accounts, st.Trees = len(tenants), len(accounts), len(trees)
	for _, t := range trees {
		bs, err := a.d.Store.ListBranches(ctx, t.ID)
		if err != nil {
			return st, err
		}
		st.Branches += len(bs)
	}
	if st.Snapshots, err = a.d.Store.CountSnapshots(ctx, ""); err != nil {
		return st, err
	}
	if st.Staged, err = a.d.Store.CountStaged(ctx); err != nil {
		return st, err
	}
	if s, ok := a.d.Objects.(objectStatter); ok {
		if st.Objects, st.StorageBytes, err = s.Stats(ctx); err != nil {
			return st, err
		}
	}
	return st, nil
}

// handleAdminStats answers GET /api/v1/admin/stats.
func (a *api) handleAdminStats(w http.ResponseWriter, r *http.Request) error {
	if err := a.d.Authz.Authorize(r.Context(), principal(r), domain.ActServerAdmin, "", nil); err != nil {
		return err
	}
	st, err := a.computeStats(r.Context())
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusOK, st)
	return nil
}

// handleAdminListAccounts answers GET /api/v1/admin/accounts.
func (a *api) handleAdminListAccounts(w http.ResponseWriter, r *http.Request) error {
	if err := a.d.Authz.Authorize(r.Context(), principal(r), domain.ActServerAdmin, "", nil); err != nil {
		return err
	}
	accts, err := a.d.Store.ListAccounts(r.Context())
	if err != nil {
		return err
	}
	accts = nonNil(accts)
	sort.Slice(accts, func(i, j int) bool { return accts[i].Username < accts[j].Username })
	writeJSON(w, http.StatusOK, accts)
	return nil
}

type patchAccountRequest struct {
	IsAdmin  *bool `json:"is_admin"`
	Disabled *bool `json:"disabled"`
}

// handleAdminPatchAccount answers PATCH /api/v1/admin/accounts/{username}.
// Admins cannot demote or disable themselves, so the server can never be
// left without a reachable admin by accident.
func (a *api) handleAdminPatchAccount(w http.ResponseWriter, r *http.Request) error {
	p := principal(r)
	ctx := r.Context()
	if err := a.d.Authz.Authorize(ctx, p, domain.ActServerAdmin, "", nil); err != nil {
		return err
	}
	var req patchAccountRequest
	if err := decodeJSON(w, r, &req); err != nil {
		return err
	}
	if req.IsAdmin == nil && req.Disabled == nil {
		return badRequestf("at least one of is_admin, disabled is required")
	}
	username := r.PathValue("username")
	acct, err := a.d.Store.GetAccountByUsername(ctx, username)
	if errors.Is(err, domain.ErrNotFound) {
		return notFoundf("account %q not found", username)
	}
	if err != nil {
		return err
	}
	details := map[string]string{"username": acct.Username}
	if req.IsAdmin != nil {
		if p != nil && acct.ID == p.AccountID && !*req.IsAdmin {
			return conflictf("you cannot remove your own admin rights")
		}
		acct.IsAdmin = *req.IsAdmin
		details["is_admin"] = strconv.FormatBool(*req.IsAdmin)
	}
	if req.Disabled != nil {
		if p != nil && acct.ID == p.AccountID && *req.Disabled {
			return conflictf("you cannot disable your own account")
		}
		acct.Disabled = *req.Disabled
		details["disabled"] = strconv.FormatBool(*req.Disabled)
	}
	if err := a.d.Store.UpdateAccount(ctx, acct); err != nil {
		return err
	}
	a.audit(r, "account.update", acct.TenantID, "account", acct.ID, details)
	writeJSON(w, http.StatusOK, acct)
	return nil
}
