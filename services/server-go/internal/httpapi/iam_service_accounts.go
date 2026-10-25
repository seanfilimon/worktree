package httpapi

import (
	"errors"
	"net/http"
	"strconv"
	"strings"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

type createServiceAccountRequest struct {
	Name        string `json:"name"`
	Description string `json:"description"`
	Role        string `json:"role"`
}

type patchServiceAccountRequest struct {
	Description *string `json:"description"`
	Role        *string `json:"role"`
	Disabled    *bool   `json:"disabled"`
}

type serviceAccountDetail struct {
	ServiceAccount *domain.ServiceAccount `json:"service_account"`
	Keys           []domain.APIToken      `json:"keys"`
}

type createKeyRequest struct {
	Name     string   `json:"name"`
	TTLHours int      `json:"ttl_hours"`
	Scopes   []string `json:"scopes"`
}

// serviceAccountByPath resolves {name} inside tenant t.
func (a *api) serviceAccountByPath(r *http.Request, t *domain.Tenant) (*domain.ServiceAccount, error) {
	name := r.PathValue("name")
	sa, err := a.d.Store.GetServiceAccountByName(r.Context(), t.ID, name)
	if errors.Is(err, domain.ErrNotFound) {
		return nil, notFoundf("service account %s/%s not found", t.Slug, name)
	}
	return sa, err
}

// handleListServiceAccounts answers GET /api/v1/tenants/{tenant}/service-accounts.
func (a *api) handleListServiceAccounts(w http.ResponseWriter, r *http.Request) error {
	t, err := a.iamTenant(r, domain.ActIAMRead)
	if err != nil {
		return err
	}
	sas, err := a.d.Store.ListServiceAccounts(r.Context(), t.ID)
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusOK, nonNil(sas))
	return nil
}

// handleCreateServiceAccount answers POST /api/v1/tenants/{tenant}/service-accounts.
func (a *api) handleCreateServiceAccount(w http.ResponseWriter, r *http.Request) error {
	t, err := a.iamTenant(r, domain.ActIAMAdmin)
	if err != nil {
		return err
	}
	var req createServiceAccountRequest
	if err := decodeJSON(w, r, &req); err != nil {
		return err
	}
	req.Name = strings.TrimSpace(req.Name)
	req.Description = strings.TrimSpace(req.Description)
	req.Role = strings.TrimSpace(req.Role)
	if err := validateIAMName("name", req.Name); err != nil {
		return err
	}
	if err := validateText("description", req.Description, 2000, false); err != nil {
		return err
	}
	ctx := r.Context()
	if err := a.checkTenantRole(ctx, t.ID, req.Role); err != nil {
		return err
	}
	if req.Role == domain.RoleOwner {
		return badRequestf("service accounts cannot be owners")
	}
	now := time.Now().UTC()
	sa := &domain.ServiceAccount{TenantID: t.ID, Name: req.Name, Description: req.Description, Role: req.Role,
		CreatedBy: principal(r).Username, CreatedAt: now, UpdatedAt: now}
	if err := a.d.Store.CreateServiceAccount(ctx, sa); err != nil {
		if errors.Is(err, domain.ErrAlreadyExists) {
			return conflictf("service account %s/%s already exists", t.Slug, req.Name)
		}
		return err
	}
	a.audit(r, "sa.create", t.ID, "service_account", sa.ID, map[string]string{"name": sa.Name, "role": sa.Role})
	writeJSON(w, http.StatusCreated, sa)
	return nil
}

// handleGetServiceAccount answers GET /api/v1/tenants/{tenant}/service-accounts/{name}.
func (a *api) handleGetServiceAccount(w http.ResponseWriter, r *http.Request) error {
	t, err := a.iamTenant(r, domain.ActIAMRead)
	if err != nil {
		return err
	}
	sa, err := a.serviceAccountByPath(r, t)
	if err != nil {
		return err
	}
	keys, err := a.d.Store.ListServiceAccountTokens(r.Context(), sa.ID)
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusOK, serviceAccountDetail{ServiceAccount: sa, Keys: nonNil(keys)})
	return nil
}

// handlePatchServiceAccount answers PATCH /api/v1/tenants/{tenant}/service-accounts/{name}.
func (a *api) handlePatchServiceAccount(w http.ResponseWriter, r *http.Request) error {
	t, err := a.iamTenant(r, domain.ActIAMAdmin)
	if err != nil {
		return err
	}
	var req patchServiceAccountRequest
	if err := decodeJSON(w, r, &req); err != nil {
		return err
	}
	sa, err := a.serviceAccountByPath(r, t)
	if err != nil {
		return err
	}
	ctx := r.Context()
	details := map[string]string{"name": sa.Name}
	if req.Description != nil {
		d := strings.TrimSpace(*req.Description)
		if err := validateText("description", d, 2000, false); err != nil {
			return err
		}
		sa.Description = d
		details["description"] = "updated"
	}
	if req.Role != nil {
		role := strings.TrimSpace(*req.Role)
		if err := a.checkTenantRole(ctx, t.ID, role); err != nil {
			return err
		}
		if role == domain.RoleOwner {
			return badRequestf("service accounts cannot be owners")
		}
		details["previous_role"], details["role"] = sa.Role, role
		sa.Role = role
	}
	if req.Disabled != nil {
		sa.Disabled = *req.Disabled
		details["disabled"] = strconv.FormatBool(sa.Disabled)
	}
	sa.UpdatedAt = time.Now().UTC()
	if err := a.d.Store.UpdateServiceAccount(ctx, sa); err != nil {
		return err
	}
	a.audit(r, "sa.update", t.ID, "service_account", sa.ID, details)
	writeJSON(w, http.StatusOK, sa)
	return nil
}

// handleDeleteServiceAccount answers DELETE /api/v1/tenants/{tenant}/service-accounts/{name}.
func (a *api) handleDeleteServiceAccount(w http.ResponseWriter, r *http.Request) error {
	t, err := a.iamTenant(r, domain.ActIAMAdmin)
	if err != nil {
		return err
	}
	sa, err := a.serviceAccountByPath(r, t)
	if err != nil {
		return err
	}
	if err := a.d.Store.DeleteServiceAccount(r.Context(), sa.ID); err != nil {
		return err
	}
	a.audit(r, "sa.delete", t.ID, "service_account", sa.ID, map[string]string{"name": sa.Name})
	w.WriteHeader(http.StatusNoContent)
	return nil
}

// handleCreateServiceAccountKey answers POST
// /api/v1/tenants/{tenant}/service-accounts/{name}/keys. The raw key is
// returned once.
func (a *api) handleCreateServiceAccountKey(w http.ResponseWriter, r *http.Request) error {
	t, err := a.iamTenant(r, domain.ActIAMAdmin)
	if err != nil {
		return err
	}
	sec, err := a.security()
	if err != nil {
		return err
	}
	var req createKeyRequest
	if err := decodeJSON(w, r, &req); err != nil {
		return err
	}
	req.Name = strings.TrimSpace(req.Name)
	if req.Name == "" {
		req.Name = "key"
	}
	if err := validateText("name", req.Name, 100, true); err != nil {
		return err
	}
	if req.TTLHours < 0 || req.TTLHours > maxTokenTTLHours {
		return badRequestf("ttl_hours must be between 0 (no expiry) and %d", maxTokenTTLHours)
	}
	// Keys act as the service account, so the caller's own token scopes
	// do not bound them; the account's role does.
	scopes, err := validateScopes(req.Scopes, nil)
	if err != nil {
		return err
	}
	sa, err := a.serviceAccountByPath(r, t)
	if err != nil {
		return err
	}
	if sa.Disabled {
		return conflictf("service account %s is disabled", sa.Name)
	}
	raw, tok, err := sec.CreateServiceAccountKey(r.Context(), sa.ID, req.Name, time.Duration(req.TTLHours)*time.Hour, scopes)
	if err != nil {
		return err
	}
	a.audit(r, "sa.key.create", t.ID, "service_account", sa.ID, map[string]string{
		"name": sa.Name, "key_id": tok.ID, "key_name": tok.Name, "scopes": strings.Join(tok.Scopes, ",")})
	writeJSON(w, http.StatusCreated, createTokenResponse{Token: raw, APIToken: tok})
	return nil
}

// handleDeleteServiceAccountKey answers DELETE
// /api/v1/tenants/{tenant}/service-accounts/{name}/keys/{id}.
func (a *api) handleDeleteServiceAccountKey(w http.ResponseWriter, r *http.Request) error {
	t, err := a.iamTenant(r, domain.ActIAMAdmin)
	if err != nil {
		return err
	}
	sec, err := a.security()
	if err != nil {
		return err
	}
	sa, err := a.serviceAccountByPath(r, t)
	if err != nil {
		return err
	}
	id := r.PathValue("id")
	if err := sec.RevokeServiceAccountKey(r.Context(), sa.ID, id); err != nil {
		if errors.Is(err, domain.ErrNotFound) {
			return notFoundf("key %q of service account %s not found", id, sa.Name)
		}
		return err
	}
	a.audit(r, "sa.key.delete", t.ID, "service_account", sa.ID, map[string]string{"name": sa.Name, "key_id": id})
	w.WriteHeader(http.StatusNoContent)
	return nil
}
