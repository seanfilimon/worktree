package httpapi

import (
	"bufio"
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"sort"
	"strconv"
	"strings"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/audit"
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
// With a tenant the caller needs tenant:admin on it. Without one, server
// admins see every tenant's events and other accounts see the merged events
// of the tenants they administer (empty if none).
func (a *api) handleAudit(w http.ResponseWriter, r *http.Request) error {
	p, err := requireLogin(r)
	if err != nil {
		return err
	}
	ctx := r.Context()
	q := r.URL.Query()
	var f domain.AuditFilter
	// For callers who are not server admins, scoped lists the tenants they
	// administer, whose events are merged.
	var (
		scoped   []string
		isScoped bool
	)
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
		if !errors.Is(err, domain.ErrForbidden) {
			return err
		}
		if scoped, err = a.adminTenantIDs(ctx, p); err != nil {
			return err
		}
		isScoped = true
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
	// v0.5 filters (§13.3): resource, exclusive upper bound, "<prefix>.*"
	// actions (handled by the store) and the NDJSON export.
	f.ResourceType = q.Get("resource_type")
	f.ResourceID = q.Get("resource_id")
	if s := q.Get("until"); s != "" {
		if f.Until, err = time.Parse(time.RFC3339, s); err != nil {
			return badRequestf("until must be an RFC 3339 timestamp")
		}
	}
	if pre, ok := strings.CutSuffix(f.Action, ".*"); ok && pre == "" {
		return badRequestf("action prefix must not be empty")
	}
	switch format := q.Get("format"); format {
	case "", "json":
	case "ndjson":
		tenants := []string{f.TenantID}
		if isScoped {
			tenants = scoped
		}
		return a.exportAudit(w, r, f, tenants)
	default:
		return badRequestf("format must be json or ndjson")
	}
	if a.d.Audit == nil {
		writeJSON(w, http.StatusOK, []domain.AuditEvent{})
		return nil
	}
	if !isScoped {
		events, err := a.d.Audit.Query(ctx, f)
		if err != nil {
			return err
		}
		writeJSON(w, http.StatusOK, nonNil(events))
		return nil
	}
	merged := []domain.AuditEvent{}
	for _, id := range scoped {
		tf := f
		tf.TenantID = id
		events, err := a.d.Audit.Query(ctx, tf)
		if err != nil {
			return err
		}
		merged = append(merged, events...)
	}
	sort.SliceStable(merged, func(i, j int) bool { return merged[i].At.After(merged[j].At) })
	if len(merged) > f.Limit {
		merged = merged[:f.Limit]
	}
	writeJSON(w, http.StatusOK, merged)
	return nil
}

// exportAudit streams every event matching f as NDJSON (one JSON object
// per line, no limit). With several tenants (a caller administering more
// than one) their events follow each other, each tenant newest first.
func (a *api) exportAudit(w http.ResponseWriter, r *http.Request, f domain.AuditFilter, tenants []string) error {
	w.Header().Set("Content-Type", "application/x-ndjson")
	w.Header().Set("X-Content-Type-Options", "nosniff")
	w.Header().Set("Cache-Control", "no-store")
	w.WriteHeader(http.StatusOK)
	if a.d.Audit == nil {
		return nil
	}
	bw := bufio.NewWriterSize(w, 64<<10)
	enc := json.NewEncoder(bw)
	rc := http.NewResponseController(w)
	var lister audit.Lister = a.d.Store
	if l, ok := a.d.Audit.(audit.Lister); ok {
		lister = l
	}
	n := 0
	for _, id := range tenants {
		tf := f
		tf.TenantID = id
		err := audit.Each(r.Context(), lister, tf, func(e domain.AuditEvent) error {
			if err := enc.Encode(e); err != nil {
				return err
			}
			if n++; n%1000 == 0 {
				if err := bw.Flush(); err != nil {
					return err
				}
				_ = rc.Flush()
			}
			return nil
		})
		if err != nil {
			// Headers are sent: all we can do is stop and log.
			a.log.WarnContext(r.Context(), "audit export aborted", "error", err, "events", n)
			_ = bw.Flush()
			return nil
		}
	}
	_ = bw.Flush()
	return nil
}

// adminTenantIDs lists the tenants on which p holds tenant:admin.
func (a *api) adminTenantIDs(ctx context.Context, p *domain.Principal) ([]string, error) {
	ms, err := a.d.Store.ListAccountMemberships(ctx, p.AccountID)
	if err != nil {
		return nil, err
	}
	var ids []string
	for _, m := range ms {
		switch err := a.d.Authz.Authorize(ctx, p, domain.ActTenantAdmin, m.TenantID, nil); {
		case err == nil:
			ids = append(ids, m.TenantID)
		case !errors.Is(err, domain.ErrForbidden):
			return nil, err
		}
	}
	return ids, nil
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
