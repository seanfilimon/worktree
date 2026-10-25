package httpapi

import (
	"bytes"
	"encoding/json"
	"errors"
	"net/http"

	"github.com/seanfilimon/worktree/services/server-go/internal/accesscfg"
	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// registerAccessRoutes registers the v0.5 declarative access routes
// (docs/ENGINEERING_PLAN.md §13.7):
//
//	GET  /tenants/{t}/access-config          -> AccessConfigDoc | 404
//	POST /tenants/{t}/access-config/plan     {"content": {...}} -> {accepted, errors, changes}
//	POST /tenants/{t}/access-config/apply    {"content": {...}} -> {accepted, errors, changes}
//	(the same three under {tree}/access-config for tree-level documents)
//	PATCH /admin/tenants/{t}                 {status, status_reason} -> tenant (server admins)
//
// The policy dry run is POST /iam/simulate with "candidate_policies"
// (iam_simulate.go).
func (a *api) registerAccessRoutes(tree string) {
	const t = "/api/v1/tenants/{tenant}/access-config"
	a.handle("GET "+t, a.handleGetAccessConfig(false))
	a.handle("POST "+t+"/plan", a.handleApplyAccessConfig(false, true))
	a.handle("POST "+t+"/apply", a.handleApplyAccessConfig(false, false))
	a.handle("GET "+tree+"/access-config", a.handleGetAccessConfig(true))
	a.handle("POST "+tree+"/access-config/plan", a.handleApplyAccessConfig(true, true))
	a.handle("POST "+tree+"/access-config/apply", a.handleApplyAccessConfig(true, false))

	a.handle("PATCH /api/v1/admin/tenants/{tenant}", a.handleAdminPatchTenant)
}

// accessCfg returns Deps.AccessCfg or a 501 error.
func (a *api) accessCfg() (*accesscfg.Service, error) {
	if a.d.AccessCfg == nil {
		return nil, errIAMNotImplemented("declarative access configuration")
	}
	return a.d.AccessCfg, nil
}

// accessScope resolves {tenant} (and {tree} when withTree) without
// authorizing: accesscfg authorizes the operation itself.
func (a *api) accessScope(r *http.Request, withTree bool) (*domain.Tenant, *domain.Tree, error) {
	if _, err := requireLogin(r); err != nil {
		return nil, nil, err
	}
	ctx := r.Context()
	t, err := a.tenantByRef(ctx, r.PathValue("tenant"))
	if err != nil {
		return nil, nil, err
	}
	if !withTree {
		return t, nil, nil
	}
	name := r.PathValue("tree")
	tree, err := a.d.Store.GetTreeByName(ctx, t.ID, name)
	if errors.Is(err, domain.ErrNotFound) {
		return nil, nil, notFoundf("tree %s/%s not found", t.Slug, name)
	}
	if err != nil {
		return nil, nil, err
	}
	return t, tree, nil
}

func (a *api) handleGetAccessConfig(withTree bool) handlerFunc {
	return func(w http.ResponseWriter, r *http.Request) error {
		svc, err := a.accessCfg()
		if err != nil {
			return err
		}
		t, tree, err := a.accessScope(r, withTree)
		if err != nil {
			return err
		}
		doc, err := svc.Get(r.Context(), principal(r), t, tree)
		if errors.Is(err, domain.ErrNotFound) {
			return notFoundf("no access document has been applied to %s", scopeName(t, tree))
		}
		if err != nil {
			return err
		}
		writeJSON(w, http.StatusOK, doc)
		return nil
	}
}

// accessConfigRequest is the body of plan / apply. Content is the canonical
// JSON object; a JSON string holding the document is accepted too.
type accessConfigRequest struct {
	Content json.RawMessage `json:"content"`
}

func (a *api) handleApplyAccessConfig(withTree, dryRun bool) handlerFunc {
	return func(w http.ResponseWriter, r *http.Request) error {
		svc, err := a.accessCfg()
		if err != nil {
			return err
		}
		t, tree, err := a.accessScope(r, withTree)
		if err != nil {
			return err
		}
		var req accessConfigRequest
		if err := decodeJSON(w, r, &req); err != nil {
			return err
		}
		content := bytes.TrimSpace(req.Content)
		if len(content) > 0 && content[0] == '"' {
			var s string
			if err := json.Unmarshal(content, &s); err != nil {
				return badRequestf("content must be a JSON object")
			}
			content = []byte(s)
		}
		if len(content) == 0 || bytes.Equal(content, []byte("null")) {
			return badRequestf("content is required")
		}
		res, err := svc.Apply(r.Context(), principal(r), t, tree, content, dryRun)
		if err != nil {
			return err
		}
		writeJSON(w, http.StatusOK, res)
		return nil
	}
}

func scopeName(t *domain.Tenant, tree *domain.Tree) string {
	if tree == nil {
		return "tenant " + t.Slug
	}
	return "tree " + t.Slug + "/" + tree.Name
}
