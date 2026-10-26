package httpapi

import (
	"errors"
	"maps"
	"net/http"
	"slices"
	"strconv"
	"strings"

	"github.com/seanfilimon/worktree/services/server-go/internal/authz"
	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// ABAC attributes of tenants, accounts and trees (v0.8,
// docs/ENGINEERING_PLAN.md §16.5). Attributes feed the attribute
// conditions of policy statements (authz/attributes.go):
//
//	GET|PUT /api/v1/tenants/{tenant}/attributes          tenant:read | tenant:admin
//	GET|PUT /api/v1/admin/accounts/{username}/attributes server admin
//	GET     /api/v1/me/attributes                        signed in (own account)
//	GET|PUT {tree}/attributes                            tree:read | tree:admin
//
// PUT replaces the whole set; {"attributes": {}} clears it. Accounts
// cannot set their own attributes: they are an input to authorization.

// attributesRequest is the body of the attribute PUT endpoints.
type attributesRequest struct {
	// Attributes is the complete new set: keys [a-z][a-z0-9_.-]{0,63},
	// values 1..256 bytes, at most 64 entries. Required ({} clears).
	Attributes map[string]string `json:"attributes"`
}

// attributesResponse is the attribute set of a tenant, account or tree.
type attributesResponse struct {
	Attributes map[string]string `json:"attributes"`
}

// registerAttributeRoutes registers the v0.8 routes for ABAC attributes of
// tenants, accounts and trees (§16.5).
func (a *api) registerAttributeRoutes(tree string) {
	a.handle("GET /api/v1/tenants/{tenant}/attributes", a.handleGetTenantAttributes)
	a.handle("PUT /api/v1/tenants/{tenant}/attributes", a.handlePutTenantAttributes)
	a.handle("GET /api/v1/admin/accounts/{username}/attributes", a.handleGetAccountAttributes)
	a.handle("PUT /api/v1/admin/accounts/{username}/attributes", a.handlePutAccountAttributes)
	a.handle("GET /api/v1/me/attributes", a.handleGetMyAttributes)
	a.handle("GET "+tree+"/attributes", a.handleGetTreeAttributes)
	a.handle("PUT "+tree+"/attributes", a.handlePutTreeAttributes)
}

func writeAttributes(w http.ResponseWriter, attrs map[string]string) {
	out := maps.Clone(attrs)
	if out == nil {
		out = map[string]string{}
	}
	writeJSON(w, http.StatusOK, attributesResponse{Attributes: out})
}

// decodeAttributes reads and validates an attributesRequest. The result
// is nil for an empty set (stored as "no attributes").
func decodeAttributes(w http.ResponseWriter, r *http.Request) (map[string]string, error) {
	var req attributesRequest
	if err := decodeJSON(w, r, &req); err != nil {
		return nil, err
	}
	if req.Attributes == nil {
		return nil, badRequestf("attributes is required (use {} to remove every attribute)")
	}
	if err := domain.ValidateAttributes(req.Attributes); err != nil {
		return nil, badRequestf("%s", strings.TrimSuffix(err.Error(), ": "+domain.ErrInvalid.Error()))
	}
	if len(req.Attributes) == 0 {
		return nil, nil
	}
	return req.Attributes, nil
}

// attributeChanges describes the difference between two attribute sets
// for the audit log (keys only: values may be sensitive), or nil when
// nothing changed.
func attributeChanges(old, next map[string]string) map[string]string {
	var set, removed []string
	for k, v := range next {
		if ov, ok := old[k]; !ok || ov != v {
			set = append(set, k)
		}
	}
	for k := range old {
		if _, ok := next[k]; !ok {
			removed = append(removed, k)
		}
	}
	if len(set) == 0 && len(removed) == 0 {
		return nil
	}
	slices.Sort(set)
	slices.Sort(removed)
	d := map[string]string{"count": strconv.Itoa(len(next))}
	if len(set) > 0 {
		d["set"] = strings.Join(set, ",")
	}
	if len(removed) > 0 {
		d["removed"] = strings.Join(removed, ",")
	}
	return d
}

// memberAccount is an account as shown to other members (tenant and team
// member lists): without its attributes, which only server admins and the
// account itself may read (they can hold clearances and the like).
func memberAccount(a domain.Account) domain.Account {
	a.Attributes = nil
	return a
}

// ---- tenants ----

func (a *api) handleGetTenantAttributes(w http.ResponseWriter, r *http.Request) error {
	t, err := a.tenantFor(r, domain.ActTenantRead)
	if err != nil {
		return err
	}
	writeAttributes(w, t.Attributes)
	return nil
}

func (a *api) handlePutTenantAttributes(w http.ResponseWriter, r *http.Request) error {
	t, err := a.tenantFor(r, domain.ActTenantAdmin)
	if err != nil {
		return err
	}
	attrs, err := decodeAttributes(w, r)
	if err != nil {
		return err
	}
	if d := attributeChanges(t.Attributes, attrs); d != nil {
		t.Attributes = attrs
		if err := a.d.Store.UpdateTenant(r.Context(), t); err != nil {
			return err
		}
		authz.InvalidateAttributes(r.Context())
		d["tenant"] = t.Slug
		a.audit(r, "tenant.attributes.update", t.ID, "tenant", t.ID, d)
	}
	writeAttributes(w, attrs)
	return nil
}

// ---- accounts ----

// adminAccount authorizes a server admin and resolves {username}.
func (a *api) adminAccount(r *http.Request) (*domain.Account, error) {
	if err := a.requireServerAdmin(r); err != nil {
		return nil, err
	}
	username := r.PathValue("username")
	acct, err := a.d.Store.GetAccountByUsername(r.Context(), username)
	if errors.Is(err, domain.ErrNotFound) {
		return nil, notFoundf("account %q not found", username)
	}
	return acct, err
}

func (a *api) handleGetAccountAttributes(w http.ResponseWriter, r *http.Request) error {
	acct, err := a.adminAccount(r)
	if err != nil {
		return err
	}
	writeAttributes(w, acct.Attributes)
	return nil
}

func (a *api) handlePutAccountAttributes(w http.ResponseWriter, r *http.Request) error {
	acct, err := a.adminAccount(r)
	if err != nil {
		return err
	}
	attrs, err := decodeAttributes(w, r)
	if err != nil {
		return err
	}
	if d := attributeChanges(acct.Attributes, attrs); d != nil {
		acct.Attributes = attrs
		if err := a.d.Store.UpdateAccount(r.Context(), acct); err != nil {
			return err
		}
		authz.InvalidateAttributes(r.Context())
		d["username"] = acct.Username
		a.audit(r, "account.attributes.update", acct.TenantID, "account", acct.ID, d)
	}
	writeAttributes(w, attrs)
	return nil
}

// handleGetMyAttributes answers GET /api/v1/me/attributes: the caller's
// own account attributes (service accounts have none).
func (a *api) handleGetMyAttributes(w http.ResponseWriter, r *http.Request) error {
	p, err := requireLogin(r)
	if err != nil {
		return err
	}
	if p.IsServiceAccount() || p.AccountID == "" {
		writeAttributes(w, nil)
		return nil
	}
	acct, err := a.d.Store.GetAccount(r.Context(), p.AccountID)
	if err != nil {
		return err
	}
	writeAttributes(w, acct.Attributes)
	return nil
}

// ---- trees ----

func (a *api) handleGetTreeAttributes(w http.ResponseWriter, r *http.Request) error {
	_, tree, err := a.treeFor(r, domain.ActTreeRead)
	if err != nil {
		return err
	}
	writeAttributes(w, tree.Attributes)
	return nil
}

func (a *api) handlePutTreeAttributes(w http.ResponseWriter, r *http.Request) error {
	t, tree, err := a.treeFor(r, domain.ActTreeAdmin)
	if err != nil {
		return err
	}
	attrs, err := decodeAttributes(w, r)
	if err != nil {
		return err
	}
	if d := attributeChanges(tree.Attributes, attrs); d != nil {
		tree.Attributes = attrs
		if err := a.d.Store.UpdateTree(r.Context(), tree); err != nil {
			return err
		}
		d["tree"] = t.Slug + "/" + tree.Name
		a.audit(r, "tree.attributes.update", t.ID, "tree", tree.ID, d)
	}
	writeAttributes(w, attrs)
	return nil
}
