package httpapi

import (
	"context"
	"errors"
	"fmt"
	"net/http"
	"strconv"
	"strings"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/authz"
	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// maxPolicyStatements bounds the size of one policy.
const maxPolicyStatements = 100

// ValidationErr is one problem found in a list of policy statements
// (POST /tenants/{t}/policies/validate).
type ValidationErr struct {
	Statement int    `json:"statement"` // index into the statements array
	Field     string `json:"field"`     // e.g. "actions[1]", "conditions.ip_ranges[0]"
	Message   string `json:"message"`
}

type policyRequest struct {
	Name        string                   `json:"name"`
	Description string                   `json:"description"`
	Enabled     *bool                    `json:"enabled"` // default true
	Statements  []domain.PolicyStatement `json:"statements"`
	// Version, when non-zero on PUT, must equal the stored version
	// (optimistic concurrency).
	Version int `json:"version"`

	// Server-managed fields: accepted so that a fetched policy can be sent
	// back unchanged, and ignored.
	ID        string     `json:"id"`
	TenantID  string     `json:"tenant_id"`
	CreatedBy string     `json:"created_by"`
	CreatedAt *time.Time `json:"created_at"`
	UpdatedAt *time.Time `json:"updated_at"`
	ManagedBy string     `json:"managed_by"` // v0.5; changed only by access documents
	TreeID    string     `json:"tree_id"`    // v0.5; changed only by access documents
}

// codeManagedByConfig answers REST changes to config-managed policies and
// roles (§13.7).
const codeManagedByConfig = "managed_by_config"

// checkManaged refuses changing a policy or role owned by a declarative
// access document, unless a tenant owner (or server admin) passes
// ?force=true. The next `wt access apply` reconciles forced edits.
func (a *api) checkManaged(r *http.Request, tenantID, managedBy, what string) error {
	if managedBy != domain.ManagedByConfig {
		return nil
	}
	if r.URL.Query().Get("force") != "true" {
		return &apiError{status: http.StatusConflict, code: codeManagedByConfig,
			msg: what + " is managed by a declarative access document; change the document and run `wt access apply`, or pass ?force=true as a tenant owner"}
	}
	p := principal(r)
	if p != nil && p.IsAdmin {
		return nil
	}
	role := ""
	if p != nil && p.AccountID != "" {
		var err error
		if role, err = a.d.Authz.RoleOf(r.Context(), p.AccountID, tenantID); err != nil {
			return err
		}
	}
	if role != domain.RoleOwner {
		return forbiddenf("only tenant owners can force changes to %s", what)
	}
	return nil
}

type validatePolicyRequest struct {
	Statements []domain.PolicyStatement `json:"statements"`
}

type validatePolicyResponse struct {
	Valid  bool            `json:"valid"`
	Errors []ValidationErr `json:"errors"`
}

// validateStatements checks statements with the authorization engine's own
// validator, so a policy that validates is one the engine can evaluate. The
// result is never nil.
func (a *api) validateStatements(stmts []domain.PolicyStatement) []ValidationErr {
	var out []ValidationErr
	if len(stmts) > maxPolicyStatements {
		out = append(out, ValidationErr{Statement: maxPolicyStatements, Field: "statements",
			Message: fmt.Sprintf("a policy may have at most %d statements", maxPolicyStatements)})
	}
	for _, e := range authz.ValidateStatements(stmts) {
		out = append(out, ValidationErr{Statement: e.Statement, Field: e.Field, Message: e.Message})
	}
	return nonNil(out)
}

// statementsError turns validation problems into a 400.
func statementsError(errs []ValidationErr) error {
	if len(errs) == 0 {
		return nil
	}
	msgs := make([]string, 0, 3)
	for i, e := range errs {
		if i == 3 {
			msgs = append(msgs, fmt.Sprintf("and %d more", len(errs)-3))
			break
		}
		msgs = append(msgs, fmt.Sprintf("statements[%d].%s: %s", e.Statement, e.Field, e.Message))
	}
	return badRequestf("invalid policy: %s", strings.Join(msgs, "; "))
}

// checkPolicyRequest normalizes and validates a create / replace body.
func (a *api) checkPolicyRequest(req *policyRequest) error {
	req.Name = strings.TrimSpace(req.Name)
	req.Description = strings.TrimSpace(req.Description)
	if err := validateText("name", req.Name, 100, true); err != nil {
		return err
	}
	if err := validateText("description", req.Description, 2000, false); err != nil {
		return err
	}
	if req.Statements == nil {
		req.Statements = []domain.PolicyStatement{}
	}
	return statementsError(a.validateStatements(req.Statements))
}

// checkPolicyName fails with 409 when another policy of the tenant is
// already called name.
func (a *api) checkPolicyName(ctx context.Context, tenantID, name, selfID string) error {
	ps, err := a.d.Store.ListPolicies(ctx, tenantID)
	if err != nil {
		return err
	}
	for _, p := range ps {
		if p.ID != selfID && strings.EqualFold(p.Name, name) {
			return conflictf("policy %q already exists", name)
		}
	}
	return nil
}

// handleListPolicies answers GET /api/v1/tenants/{tenant}/policies.
func (a *api) handleListPolicies(w http.ResponseWriter, r *http.Request) error {
	t, err := a.iamTenant(r, domain.ActIAMRead)
	if err != nil {
		return err
	}
	ps, err := a.d.Store.ListPolicies(r.Context(), t.ID)
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusOK, nonNil(ps))
	return nil
}

// handleCreatePolicy answers POST /api/v1/tenants/{tenant}/policies.
func (a *api) handleCreatePolicy(w http.ResponseWriter, r *http.Request) error {
	t, err := a.iamTenant(r, domain.ActIAMAdmin)
	if err != nil {
		return err
	}
	var req policyRequest
	if err := decodeJSON(w, r, &req); err != nil {
		return err
	}
	if err := a.checkPolicyRequest(&req); err != nil {
		return err
	}
	ctx := r.Context()
	if err := a.checkPolicyName(ctx, t.ID, req.Name, ""); err != nil {
		return err
	}
	now := time.Now().UTC()
	pol := &domain.Policy{
		TenantID: t.ID, Name: req.Name, Description: req.Description, Enabled: req.Enabled == nil || *req.Enabled,
		Statements: req.Statements, Version: 1, CreatedBy: principal(r).Username, CreatedAt: now, UpdatedAt: now,
	}
	if err := a.d.Store.CreatePolicy(ctx, pol); err != nil {
		if errors.Is(err, domain.ErrAlreadyExists) {
			return conflictf("policy %q already exists", req.Name)
		}
		return err
	}
	a.audit(r, "policy.create", t.ID, "policy", pol.ID, map[string]string{
		"name": pol.Name, "enabled": strconv.FormatBool(pol.Enabled), "statements": strconv.Itoa(len(pol.Statements))})
	writeJSON(w, http.StatusCreated, pol)
	return nil
}

// handleValidatePolicy answers POST /api/v1/tenants/{tenant}/policies/validate.
func (a *api) handleValidatePolicy(w http.ResponseWriter, r *http.Request) error {
	if _, err := a.iamTenant(r, domain.ActIAMRead); err != nil {
		return err
	}
	var req validatePolicyRequest
	if err := decodeJSON(w, r, &req); err != nil {
		return err
	}
	errs := a.validateStatements(req.Statements)
	writeJSON(w, http.StatusOK, validatePolicyResponse{Valid: len(errs) == 0, Errors: errs})
	return nil
}

// policyByPath loads policy {id} of tenant t.
func (a *api) policyByPath(r *http.Request, t *domain.Tenant) (*domain.Policy, error) {
	id := r.PathValue("id")
	p, err := a.d.Store.GetPolicy(r.Context(), t.ID, id)
	if errors.Is(err, domain.ErrNotFound) {
		return nil, notFoundf("policy %q not found", id)
	}
	return p, err
}

// handleGetPolicy answers GET /api/v1/tenants/{tenant}/policies/{id}.
func (a *api) handleGetPolicy(w http.ResponseWriter, r *http.Request) error {
	t, err := a.iamTenant(r, domain.ActIAMRead)
	if err != nil {
		return err
	}
	pol, err := a.policyByPath(r, t)
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusOK, pol)
	return nil
}

// handlePutPolicy answers PUT /api/v1/tenants/{tenant}/policies/{id}: a
// full replace that increments the version.
func (a *api) handlePutPolicy(w http.ResponseWriter, r *http.Request) error {
	t, err := a.iamTenant(r, domain.ActIAMAdmin)
	if err != nil {
		return err
	}
	var req policyRequest
	if err := decodeJSON(w, r, &req); err != nil {
		return err
	}
	if err := a.checkPolicyRequest(&req); err != nil {
		return err
	}
	pol, err := a.policyByPath(r, t)
	if err != nil {
		return err
	}
	if err := a.checkManaged(r, t.ID, pol.ManagedBy, "policy "+pol.Name); err != nil {
		return err
	}
	if req.Version != 0 && req.Version != pol.Version {
		return conflictf("policy was modified (version %d, request is based on %d)", pol.Version, req.Version)
	}
	ctx := r.Context()
	if err := a.checkPolicyName(ctx, t.ID, req.Name, pol.ID); err != nil {
		return err
	}
	prevEnabled := pol.Enabled
	pol.Name, pol.Description, pol.Statements = req.Name, req.Description, req.Statements
	pol.Enabled = req.Enabled == nil || *req.Enabled
	pol.Version++
	pol.UpdatedAt = time.Now().UTC()
	if err := a.d.Store.UpdatePolicy(ctx, pol); err != nil {
		return err
	}
	a.audit(r, "policy.update", t.ID, "policy", pol.ID, map[string]string{
		"name": pol.Name, "version": strconv.Itoa(pol.Version), "enabled": strconv.FormatBool(pol.Enabled),
		"previous_enabled": strconv.FormatBool(prevEnabled), "statements": strconv.Itoa(len(pol.Statements))})
	writeJSON(w, http.StatusOK, pol)
	return nil
}

// handleDeletePolicy answers DELETE /api/v1/tenants/{tenant}/policies/{id}.
func (a *api) handleDeletePolicy(w http.ResponseWriter, r *http.Request) error {
	t, err := a.iamTenant(r, domain.ActIAMAdmin)
	if err != nil {
		return err
	}
	pol, err := a.policyByPath(r, t)
	if err != nil {
		return err
	}
	if err := a.checkManaged(r, t.ID, pol.ManagedBy, "policy "+pol.Name); err != nil {
		return err
	}
	if err := a.d.Store.DeletePolicy(r.Context(), t.ID, pol.ID); err != nil {
		return err
	}
	a.audit(r, "policy.delete", t.ID, "policy", pol.ID, map[string]string{"name": pol.Name})
	w.WriteHeader(http.StatusNoContent)
	return nil
}
