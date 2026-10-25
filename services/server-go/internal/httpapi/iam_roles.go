package httpapi

import (
	"context"
	"errors"
	"net/http"
	"slices"
	"strings"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

type roleView struct {
	Name        string          `json:"name"`
	Description string          `json:"description"`
	Permissions []domain.Action `json:"permissions"`
	Builtin     bool            `json:"builtin"`
	ManagedBy   string          `json:"managed_by,omitempty"` // v0.5: "config" for roles owned by an access document
	CreatedAt   *time.Time      `json:"created_at,omitempty"`
	UpdatedAt   *time.Time      `json:"updated_at,omitempty"`
}

type createRoleRequest struct {
	Name        string   `json:"name"`
	Description string   `json:"description"`
	Permissions []string `json:"permissions"`
}

type patchRoleRequest struct {
	Description *string   `json:"description"`
	Permissions *[]string `json:"permissions"`
}

func builtinRoleView(name, description string) roleView {
	return roleView{Name: name, Description: description, Permissions: iamCatalog.BuiltinPermissions(name), Builtin: true}
}

func customRoleView(r *domain.CustomRole) roleView {
	v := roleView{Name: r.Name, Description: r.Description, Permissions: nonNil(r.Permissions), ManagedBy: r.ManagedBy}
	if !r.CreatedAt.IsZero() {
		c := r.CreatedAt
		v.CreatedAt = &c
	}
	if !r.UpdatedAt.IsZero() {
		u := r.UpdatedAt
		v.UpdatedAt = &u
	}
	return v
}

// normalizePermissions validates custom-role permissions: concrete
// assignable actions or "<area>:*". server:admin is never assignable.
func normalizePermissions(perms []string) ([]domain.Action, error) {
	if len(perms) > 128 {
		return nil, badRequestf("at most 128 permissions are allowed")
	}
	out := make([]domain.Action, 0, len(perms))
	for _, s := range perms {
		s = strings.TrimSpace(s)
		if s == string(domain.ActServerAdmin) || s == "server:*" {
			return nil, badRequestf("permissions: %q cannot be granted by a role", s)
		}
		if msg := checkActionPattern(s, false); msg != "" {
			return nil, badRequestf("permissions: %s", msg)
		}
		if !slices.Contains(out, domain.Action(s)) {
			out = append(out, domain.Action(s))
		}
	}
	slices.Sort(out)
	return out, nil
}

// handleListRoles answers GET /api/v1/tenants/{tenant}/roles: built-in
// roles first (most privileged first), then custom roles by name.
func (a *api) handleListRoles(w http.ResponseWriter, r *http.Request) error {
	t, err := a.iamTenant(r, domain.ActIAMRead)
	if err != nil {
		return err
	}
	custom, err := a.d.Store.ListRoles(r.Context(), t.ID)
	if err != nil {
		return err
	}
	out := make([]roleView, 0, len(builtinRoles)+len(custom))
	for _, b := range builtinRoles {
		out = append(out, builtinRoleView(b.name, b.description))
	}
	for i := range custom {
		out = append(out, customRoleView(&custom[i]))
	}
	writeJSON(w, http.StatusOK, out)
	return nil
}

// handleCreateRole answers POST /api/v1/tenants/{tenant}/roles.
func (a *api) handleCreateRole(w http.ResponseWriter, r *http.Request) error {
	t, err := a.iamTenant(r, domain.ActIAMAdmin)
	if err != nil {
		return err
	}
	var req createRoleRequest
	if err := decodeJSON(w, r, &req); err != nil {
		return err
	}
	req.Name = strings.TrimSpace(req.Name)
	req.Description = strings.TrimSpace(req.Description)
	if err := validateIAMName("name", req.Name); err != nil {
		return err
	}
	if isBuiltinRole(req.Name) {
		return conflictf("%q is a built-in role", req.Name)
	}
	if err := validateText("description", req.Description, 2000, false); err != nil {
		return err
	}
	perms, err := normalizePermissions(req.Permissions)
	if err != nil {
		return err
	}
	ctx := r.Context()
	if _, err := a.d.Store.GetRole(ctx, t.ID, req.Name); err == nil {
		return conflictf("role %q already exists", req.Name)
	} else if !errors.Is(err, domain.ErrNotFound) {
		return err
	}
	now := time.Now().UTC()
	role := &domain.CustomRole{TenantID: t.ID, Name: req.Name, Description: req.Description, Permissions: perms, CreatedAt: now, UpdatedAt: now}
	if err := a.d.Store.PutRole(ctx, role); err != nil {
		if errors.Is(err, domain.ErrAlreadyExists) {
			return conflictf("role %q already exists", req.Name)
		}
		return err
	}
	a.audit(r, "role.create", t.ID, "role", role.Name, map[string]string{"permissions": joinActions(perms)})
	writeJSON(w, http.StatusCreated, customRoleView(role))
	return nil
}

// customRoleByPath loads the custom role {name} of tenant t (404 when
// there is none).
func (a *api) customRoleByPath(r *http.Request, t *domain.Tenant) (*domain.CustomRole, error) {
	name := r.PathValue("name")
	role, err := a.d.Store.GetRole(r.Context(), t.ID, name)
	if errors.Is(err, domain.ErrNotFound) {
		return nil, notFoundf("role %q not found", name)
	}
	return role, err
}

// handleGetRole answers GET /api/v1/tenants/{tenant}/roles/{name}.
func (a *api) handleGetRole(w http.ResponseWriter, r *http.Request) error {
	t, err := a.iamTenant(r, domain.ActIAMRead)
	if err != nil {
		return err
	}
	name := r.PathValue("name")
	for _, b := range builtinRoles {
		if b.name == name {
			writeJSON(w, http.StatusOK, builtinRoleView(b.name, b.description))
			return nil
		}
	}
	role, err := a.customRoleByPath(r, t)
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusOK, customRoleView(role))
	return nil
}

// handlePatchRole answers PATCH /api/v1/tenants/{tenant}/roles/{name}.
func (a *api) handlePatchRole(w http.ResponseWriter, r *http.Request) error {
	t, err := a.iamTenant(r, domain.ActIAMAdmin)
	if err != nil {
		return err
	}
	if isBuiltinRole(r.PathValue("name")) {
		return badRequestf("built-in roles cannot be modified")
	}
	var req patchRoleRequest
	if err := decodeJSON(w, r, &req); err != nil {
		return err
	}
	role, err := a.customRoleByPath(r, t)
	if err != nil {
		return err
	}
	if err := a.checkManaged(r, t.ID, role.ManagedBy, "role "+role.Name); err != nil {
		return err
	}
	details := map[string]string{}
	if req.Description != nil {
		d := strings.TrimSpace(*req.Description)
		if err := validateText("description", d, 2000, false); err != nil {
			return err
		}
		role.Description = d
		details["description"] = "updated"
	}
	if req.Permissions != nil {
		perms, err := normalizePermissions(*req.Permissions)
		if err != nil {
			return err
		}
		details["previous_permissions"] = joinActions(role.Permissions)
		details["permissions"] = joinActions(perms)
		role.Permissions = perms
	}
	role.UpdatedAt = time.Now().UTC()
	if err := a.d.Store.PutRole(r.Context(), role); err != nil {
		return err
	}
	a.audit(r, "role.update", t.ID, "role", role.Name, details)
	writeJSON(w, http.StatusOK, customRoleView(role))
	return nil
}

// handleDeleteRole answers DELETE /api/v1/tenants/{tenant}/roles/{name};
// 409 while the role is assigned.
func (a *api) handleDeleteRole(w http.ResponseWriter, r *http.Request) error {
	t, err := a.iamTenant(r, domain.ActIAMAdmin)
	if err != nil {
		return err
	}
	if isBuiltinRole(r.PathValue("name")) {
		return badRequestf("built-in roles cannot be deleted")
	}
	role, err := a.customRoleByPath(r, t)
	if err != nil {
		return err
	}
	if err := a.checkManaged(r, t.ID, role.ManagedBy, "role "+role.Name); err != nil {
		return err
	}
	ctx := r.Context()
	if where, err := a.roleUsage(ctx, t.ID, role.Name); err != nil {
		return err
	} else if where != "" {
		return conflictf("role %q is still assigned (%s)", role.Name, where)
	}
	if err := a.d.Store.DeleteRole(ctx, t.ID, role.Name); err != nil {
		return err
	}
	a.audit(r, "role.delete", t.ID, "role", role.Name, nil)
	w.WriteHeader(http.StatusNoContent)
	return nil
}

// roleUsage describes the first place role is assigned within tenantID
// (memberships, tree grants, service accounts, pending invitations), or ""
// when it is unused.
func (a *api) roleUsage(ctx context.Context, tenantID, role string) (string, error) {
	ms, err := a.d.Store.ListTenantMembers(ctx, tenantID)
	if err != nil {
		return "", err
	}
	for _, m := range ms {
		if m.Role == role {
			return "tenant membership", nil
		}
	}
	trees, err := a.d.Store.ListTrees(ctx, tenantID)
	if err != nil {
		return "", err
	}
	for _, tr := range trees {
		gs, err := a.d.Store.ListGrants(ctx, tr.ID)
		if err != nil {
			return "", err
		}
		for _, g := range gs {
			if g.Role == role {
				return "grant on tree " + tr.Name, nil
			}
		}
	}
	sas, err := a.d.Store.ListServiceAccounts(ctx, tenantID)
	if err != nil {
		return "", err
	}
	for _, sa := range sas {
		if sa.Role == role {
			return "service account " + sa.Name, nil
		}
	}
	invs, err := a.d.Store.ListInvitations(ctx, tenantID)
	if err != nil {
		return "", err
	}
	now := time.Now()
	for _, inv := range invs {
		if inv.Role == role && !inv.Revoked && inv.AcceptedAt == nil && now.Before(inv.ExpiresAt) {
			return "pending invitation", nil
		}
	}
	return "", nil
}

func joinActions(acts []domain.Action) string {
	s := make([]string, len(acts))
	for i, a := range acts {
		s[i] = string(a)
	}
	return strings.Join(s, ",")
}
