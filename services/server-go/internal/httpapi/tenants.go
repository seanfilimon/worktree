package httpapi

import (
	"errors"
	"net/http"
	"sort"
	"strings"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

type createTenantRequest struct {
	Slug string `json:"slug"`
	Name string `json:"name"`
}

type memberView struct {
	Account domain.Account `json:"account"`
	Role    string         `json:"role"`
}

type putMemberRequest struct {
	Role string `json:"role"`
}

// handleListTenants answers GET /api/v1/tenants with the tenants visible to
// the caller: every tenant for server admins, otherwise the caller's
// memberships.
func (a *api) handleListTenants(w http.ResponseWriter, r *http.Request) error {
	p, err := requireLogin(r)
	if err != nil {
		return err
	}
	ctx := r.Context()
	var out []domain.Tenant
	if p.IsAdmin {
		if out, err = a.d.Store.ListTenants(ctx); err != nil {
			return err
		}
	} else {
		ms, err := a.d.Store.ListAccountMemberships(ctx, p.AccountID)
		if err != nil {
			return err
		}
		out = make([]domain.Tenant, 0, len(ms))
		for _, m := range ms {
			t, err := a.d.Store.GetTenant(ctx, m.TenantID)
			if errors.Is(err, domain.ErrNotFound) {
				continue
			}
			if err != nil {
				return err
			}
			out = append(out, *t)
		}
	}
	out = nonNil(out)
	sort.Slice(out, func(i, j int) bool { return out[i].Slug < out[j].Slug })
	writeJSON(w, http.StatusOK, out)
	return nil
}

// handleCreateTenant answers POST /api/v1/tenants: any authenticated
// account may create an organization tenant and becomes its owner.
func (a *api) handleCreateTenant(w http.ResponseWriter, r *http.Request) error {
	p, err := requireLogin(r)
	if err != nil {
		return err
	}
	var req createTenantRequest
	if err := decodeJSON(w, r, &req); err != nil {
		return err
	}
	req.Slug = strings.TrimSpace(req.Slug)
	req.Name = strings.TrimSpace(req.Name)
	if err := validateSlug(req.Slug); err != nil {
		return err
	}
	if req.Name == "" {
		req.Name = req.Slug
	}
	if err := validateText("name", req.Name, 200, true); err != nil {
		return err
	}
	ctx := r.Context()
	now := time.Now().UTC()
	t := &domain.Tenant{ID: newID(), Slug: req.Slug, Name: req.Name, Kind: domain.TenantOrg, Plan: "free", CreatedAt: now}
	if err := a.d.Store.CreateTenant(ctx, t); err != nil {
		if errors.Is(err, domain.ErrAlreadyExists) {
			return conflictf("tenant %q already exists", req.Slug)
		}
		return err
	}
	m := &domain.Membership{TenantID: t.ID, AccountID: p.AccountID, Role: domain.RoleOwner, CreatedAt: now}
	if err := a.d.Store.PutMembership(ctx, m); err != nil {
		return err
	}
	a.audit(r, "tenant.create", t.ID, "tenant", t.ID, map[string]string{"slug": t.Slug})
	writeJSON(w, http.StatusCreated, t)
	return nil
}

// handleGetTenant answers GET /api/v1/tenants/{tenant}.
func (a *api) handleGetTenant(w http.ResponseWriter, r *http.Request) error {
	t, err := a.tenantFor(r, domain.ActTenantRead)
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusOK, t)
	return nil
}

// handleListMembers answers GET /api/v1/tenants/{tenant}/members.
func (a *api) handleListMembers(w http.ResponseWriter, r *http.Request) error {
	t, err := a.tenantFor(r, domain.ActTenantRead)
	if err != nil {
		return err
	}
	ctx := r.Context()
	ms, err := a.d.Store.ListTenantMembers(ctx, t.ID)
	if err != nil {
		return err
	}
	out := make([]memberView, 0, len(ms))
	for _, m := range ms {
		acct, err := a.d.Store.GetAccount(ctx, m.AccountID)
		if errors.Is(err, domain.ErrNotFound) {
			continue
		}
		if err != nil {
			return err
		}
		out = append(out, memberView{Account: *acct, Role: m.Role})
	}
	sort.Slice(out, func(i, j int) bool { return out[i].Account.Username < out[j].Account.Username })
	writeJSON(w, http.StatusOK, out)
	return nil
}

// memberTarget resolves the {username} of a member request.
func (a *api) memberTarget(r *http.Request) (*domain.Account, error) {
	username := r.PathValue("username")
	acct, err := a.d.Store.GetAccountByUsername(r.Context(), username)
	if errors.Is(err, domain.ErrNotFound) {
		return nil, notFoundf("account %q not found", username)
	}
	return acct, err
}

// checkOwnerChange enforces that only owners (or server admins) grant or
// revoke the owner role, and that a tenant always keeps one owner.
func (a *api) checkOwnerChange(r *http.Request, tenantID string, cur *domain.Membership, newRole string) error {
	p := principal(r)
	touchesOwner := newRole == domain.RoleOwner || (cur != nil && cur.Role == domain.RoleOwner)
	if !touchesOwner {
		return nil
	}
	if !p.IsAdmin {
		role, err := a.d.Authz.RoleOf(r.Context(), p.AccountID, tenantID)
		if err != nil {
			return err
		}
		if role != domain.RoleOwner {
			return forbiddenf("only tenant owners can grant or revoke the owner role")
		}
	}
	if cur != nil && cur.Role == domain.RoleOwner && newRole != domain.RoleOwner {
		return a.checkLastOwner(r, tenantID)
	}
	return nil
}

// currentMembership returns the membership or nil when there is none.
func (a *api) currentMembership(r *http.Request, tenantID, accountID string) (*domain.Membership, error) {
	m, err := a.d.Store.GetMembership(r.Context(), tenantID, accountID)
	if errors.Is(err, domain.ErrNotFound) {
		return nil, nil
	}
	return m, err
}

// handlePutMember answers PUT /api/v1/tenants/{tenant}/members/{username}.
func (a *api) handlePutMember(w http.ResponseWriter, r *http.Request) error {
	if _, err := requireLogin(r); err != nil {
		return err
	}
	t, err := a.tenantFor(r, domain.ActTenantAdmin)
	if err != nil {
		return err
	}
	var req putMemberRequest
	if err := decodeJSON(w, r, &req); err != nil {
		return err
	}
	if err := a.checkTenantRole(r.Context(), t.ID, req.Role); err != nil {
		return err
	}
	acct, err := a.memberTarget(r)
	if err != nil {
		return err
	}
	cur, err := a.currentMembership(r, t.ID, acct.ID)
	if err != nil {
		return err
	}
	if err := a.checkOwnerChange(r, t.ID, cur, req.Role); err != nil {
		return err
	}
	// ---- ops (§13.3): tenant MaxMembers quota for new members (409
	// quota_exceeded); role changes of existing members are not limited ----
	if cur == nil {
		if err := a.checkMemberQuota(r.Context(), t); err != nil {
			return err
		}
	}
	// ---- end ops ----
	m := &domain.Membership{TenantID: t.ID, AccountID: acct.ID, Role: req.Role, CreatedAt: time.Now().UTC()}
	prev := ""
	if cur != nil {
		m.CreatedAt = cur.CreatedAt
		prev = cur.Role
	}
	if err := a.d.Store.PutMembership(r.Context(), m); err != nil {
		return err
	}
	a.audit(r, "member.update", t.ID, "account", acct.ID,
		map[string]string{"username": acct.Username, "role": req.Role, "previous_role": prev})
	writeJSON(w, http.StatusOK, memberView{Account: *acct, Role: m.Role})
	return nil
}

// handleDeleteMember answers DELETE /api/v1/tenants/{tenant}/members/{username}.
// Members may always remove themselves; removing others needs tenant:admin.
func (a *api) handleDeleteMember(w http.ResponseWriter, r *http.Request) error {
	p, err := requireLogin(r)
	if err != nil {
		return err
	}
	t, err := a.tenantByRef(r.Context(), r.PathValue("tenant"))
	if err != nil {
		return err
	}
	acct, err := a.memberTarget(r)
	if err != nil {
		return err
	}
	self := acct.ID == p.AccountID
	if !self {
		if err := a.d.Authz.Authorize(r.Context(), p, domain.ActTenantAdmin, t.ID, nil); err != nil {
			return err
		}
	}
	cur, err := a.currentMembership(r, t.ID, acct.ID)
	if err != nil {
		return err
	}
	if cur == nil {
		return notFoundf("%s is not a member of %s", acct.Username, t.Slug)
	}
	if self && cur.Role == domain.RoleOwner {
		// Leaving as owner only requires that another owner remains.
		if err := a.checkLastOwner(r, t.ID); err != nil {
			return err
		}
	} else if err := a.checkOwnerChange(r, t.ID, cur, ""); err != nil {
		return err
	}
	if err := a.d.Store.DeleteMembership(r.Context(), t.ID, acct.ID); err != nil {
		return err
	}
	a.audit(r, "member.remove", t.ID, "account", acct.ID,
		map[string]string{"username": acct.Username, "previous_role": cur.Role})
	w.WriteHeader(http.StatusNoContent)
	return nil
}

// checkLastOwner fails with 409 when tenantID has at most one owner, i.e.
// removing or demoting an owner would leave it ownerless.
func (a *api) checkLastOwner(r *http.Request, tenantID string) error {
	ms, err := a.d.Store.ListTenantMembers(r.Context(), tenantID)
	if err != nil {
		return err
	}
	owners := 0
	for _, m := range ms {
		if m.Role == domain.RoleOwner {
			owners++
		}
	}
	if owners <= 1 {
		return conflictf("a tenant must keep at least one owner")
	}
	return nil
}
