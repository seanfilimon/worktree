package httpapi

import (
	"context"
	"errors"
	"net/http"
	"strconv"
	"strings"

	"github.com/seanfilimon/worktree/services/server-go/internal/codesearch"
	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// Code search (docs/ENGINEERING_PLAN.md §14.3).
//
//	GET /api/v1/search/code?q=&tenant=&tree=&path=&regex=false&case=false&context=2&limit=50
//
// The scope is one tree (tree=TENANT/TREE, or tenant=T&tree=TREE), the
// trees of one tenant (tenant=T), or every tree on the server; within it
// only trees the caller may read (tree:read) are searched — anonymous
// callers therefore see public trees only. Hits in paths a path-deny
// policy or a license rule keeps from the caller are dropped. Without
// Deps.CodeSearch the endpoint answers 501 not_implemented.

func (a *api) registerCodeSearchRoutes() {
	a.handle("GET /api/v1/search/code", a.handleCodeSearch)
}

func (a *api) handleCodeSearch(w http.ResponseWriter, r *http.Request) error {
	if a.d.CodeSearch == nil {
		return &apiError{status: http.StatusNotImplemented, code: "not_implemented",
			msg: "code search is not enabled on this server"}
	}
	q, err := codeSearchQuery(r)
	if err != nil {
		return err
	}
	ctx := r.Context()
	p := principal(r)
	targets, err := a.codeSearchTargets(r)
	if err != nil {
		return err
	}
	filter := func(ctx context.Context, tree *domain.Tree, paths []string) (func(string) bool, error) {
		rs, err := a.readRestrictions(ctx, p, tree, paths)
		if err != nil {
			return nil, err
		}
		return rs.restricted, nil
	}
	res, err := a.d.CodeSearch.Search(ctx, q, targets, filter)
	if errors.Is(err, codesearch.ErrInvalidQuery) {
		return badRequestf("%s", err.Error())
	}
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusOK, res)
	return nil
}

// codeSearchQuery parses the query parameters.
func codeSearchQuery(r *http.Request) (codesearch.Query, error) {
	v := r.URL.Query()
	q := codesearch.Query{Pattern: v.Get("q"), Path: v.Get("path"), Context: codesearch.DefaultContext}
	if q.Pattern == "" {
		return q, badRequestf("query parameter q is required")
	}
	flag := func(name string) (bool, error) {
		s := v.Get(name)
		if s == "" {
			return false, nil
		}
		b, err := strconv.ParseBool(s)
		if err != nil {
			return false, badRequestf("%s must be true or false", name)
		}
		return b, nil
	}
	var err error
	if q.Regex, err = flag("regex"); err != nil {
		return q, err
	}
	if q.CaseSensitive, err = flag("case"); err != nil {
		return q, err
	}
	if s := v.Get("context"); s != "" {
		n, err := strconv.Atoi(s)
		if err != nil || n < 0 || n > codesearch.MaxContext {
			return q, badRequestf("context must be an integer between 0 and %d", codesearch.MaxContext)
		}
		q.Context = n
	}
	if q.Limit, err = queryInt(r, "limit", codesearch.DefaultLimit, codesearch.MaxLimit); err != nil {
		return q, err
	}
	return q, nil
}

// codeSearchTargets resolves the scope of a code search to the trees the
// caller may read.
func (a *api) codeSearchTargets(r *http.Request) ([]codesearch.Target, error) {
	ctx := r.Context()
	p := principal(r)
	v := r.URL.Query()
	tenantRef, treeRef := strings.TrimSpace(v.Get("tenant")), strings.TrimSpace(v.Get("tree"))
	if t, name, ok := strings.Cut(treeRef, "/"); ok {
		if tenantRef != "" && tenantRef != t {
			return nil, badRequestf("tree %q is not in tenant %q", treeRef, tenantRef)
		}
		tenantRef, treeRef = t, name
	}
	if treeRef != "" && tenantRef == "" {
		return nil, badRequestf("tree must be TENANT/TREE (or pass tenant)")
	}

	if treeRef != "" {
		t, err := a.tenantByRef(ctx, tenantRef)
		if err != nil {
			return nil, err
		}
		tree, err := a.d.Store.GetTreeByName(ctx, t.ID, treeRef)
		if errors.Is(err, domain.ErrNotFound) {
			return nil, notFoundf("tree %s/%s not found", t.Slug, treeRef)
		}
		if err != nil {
			return nil, err
		}
		if err := a.d.Authz.Authorize(ctx, p, domain.ActTreeRead, t.ID, tree); err != nil {
			return nil, err
		}
		return []codesearch.Target{{TenantSlug: t.Slug, Tree: tree}}, nil
	}

	var trees []domain.Tree
	slugs := map[string]string{} // tenant id -> slug
	if tenantRef != "" {
		t, err := a.tenantByRef(ctx, tenantRef)
		if err != nil {
			return nil, err
		}
		slugs[t.ID] = t.Slug
		if trees, err = a.d.Store.ListTrees(ctx, t.ID); err != nil {
			return nil, err
		}
	} else {
		var err error
		if trees, err = a.d.Store.ListTrees(ctx, ""); err != nil {
			return nil, err
		}
	}
	out := []codesearch.Target{}
	for i := range trees {
		tree := &trees[i]
		if p == nil && tree.Visibility != domain.VisibilityPublic {
			continue // anonymous callers search public trees only
		}
		if err := a.d.Authz.Authorize(ctx, p, domain.ActTreeRead, tree.TenantID, tree); err != nil {
			if isDenied(err) {
				continue
			}
			return nil, err
		}
		slug, ok := slugs[tree.TenantID]
		if !ok {
			t, err := a.d.Store.GetTenant(ctx, tree.TenantID)
			if errors.Is(err, domain.ErrNotFound) {
				continue // dangling tree
			}
			if err != nil {
				return nil, err
			}
			slug = t.Slug
			slugs[tree.TenantID] = slug
		}
		out = append(out, codesearch.Target{TenantSlug: slug, Tree: tree})
	}
	return out, nil
}
