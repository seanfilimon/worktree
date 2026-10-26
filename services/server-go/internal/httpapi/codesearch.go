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

// Code search (docs/ENGINEERING_PLAN.md §14.3, §15.9).
//
//	GET  /api/v1/search/code?q=&tenant=&tree=&path=&regex=false&case=false&context=2&limit=50&branch=|branches=all
//	GET  /api/v1/search/symbols?q=&kind=&tenant=&tree=&path=&limit=50&branch=|branches=all
//	GET  /api/v1/admin/code-index                  (server admins)
//	POST /api/v1/admin/code-index/rebuild?tree=     (server admins)
//
// The scope is one tree (tree=TENANT/TREE, or tenant=T&tree=TREE), the
// trees of one tenant (tenant=T), or every tree on the server; within it
// only trees the caller may read (tree:read) are searched — anonymous
// callers therefore see public trees only. Hits in paths a path-deny
// policy or a license rule keeps from the caller are dropped. Without
// Deps.CodeSearch the endpoints answer 501 not_implemented.

const auditCodeIndexRebuild = "code_index.rebuild"

func (a *api) registerCodeSearchRoutes() {
	a.handle("GET /api/v1/search/code", a.handleCodeSearch)
	// search-v2 (§15.9)
	a.handle("GET /api/v1/search/symbols", a.handleSymbolSearch)
	a.handle("GET /api/v1/admin/code-index", a.handleCodeIndexStatus)
	a.handle("POST /api/v1/admin/code-index/rebuild", a.handleCodeIndexRebuild)
}

func (a *api) codeSearchEnabled() error {
	if a.d.CodeSearch == nil {
		return &apiError{status: http.StatusNotImplemented, code: "not_implemented",
			msg: "code search is not enabled on this server"}
	}
	return nil
}

// codeSearchFilter applies the §13.1 read restrictions of the caller.
func (a *api) codeSearchFilter(r *http.Request) codesearch.ReadFilter {
	p := principal(r)
	return func(ctx context.Context, tree *domain.Tree, paths []string) (func(string) bool, error) {
		rs, err := a.readRestrictions(ctx, p, tree, paths)
		if err != nil {
			return nil, err
		}
		return rs.restricted, nil
	}
}

func (a *api) handleCodeSearch(w http.ResponseWriter, r *http.Request) error {
	if err := a.codeSearchEnabled(); err != nil {
		return err
	}
	q, err := codeSearchQuery(r)
	if err != nil {
		return err
	}
	targets, err := a.codeSearchTargets(r)
	if err != nil {
		return err
	}
	res, err := a.d.CodeSearch.Search(r.Context(), q, targets, a.codeSearchFilter(r))
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
	if q.Branch, q.AllBranches, err = branchScope(r); err != nil {
		return q, err
	}
	return q, nil
}

// branchScope parses branch=NAME and branches=all|default (§15.9).
func branchScope(r *http.Request) (branch string, all bool, err error) {
	v := r.URL.Query()
	branch = strings.TrimSpace(v.Get("branch"))
	switch s := strings.ToLower(strings.TrimSpace(v.Get("branches"))); s {
	case "", "default":
	case "all":
		all = true
	default:
		return "", false, badRequestf("branches must be all or default")
	}
	if branch != "" && all {
		return "", false, badRequestf("branch and branches=all are mutually exclusive")
	}
	return branch, all, nil
}

// handleSymbolSearch answers GET /api/v1/search/symbols (§15.9).
func (a *api) handleSymbolSearch(w http.ResponseWriter, r *http.Request) error {
	if err := a.codeSearchEnabled(); err != nil {
		return err
	}
	v := r.URL.Query()
	q := codesearch.SymbolQuery{Query: v.Get("q"), Path: v.Get("path")}
	if strings.TrimSpace(q.Query) == "" {
		return badRequestf("query parameter q is required")
	}
	var err error
	if q.Kinds, err = codesearch.ParseKinds(v.Get("kind")); err != nil {
		return badRequestf("%s", err.Error())
	}
	if q.Limit, err = queryInt(r, "limit", codesearch.DefaultLimit, codesearch.MaxLimit); err != nil {
		return err
	}
	if q.Branch, q.AllBranches, err = branchScope(r); err != nil {
		return err
	}
	targets, err := a.codeSearchTargets(r)
	if err != nil {
		return err
	}
	res, err := a.d.CodeSearch.SearchSymbols(r.Context(), q, targets, a.codeSearchFilter(r))
	if errors.Is(err, codesearch.ErrInvalidQuery) {
		return badRequestf("%s", err.Error())
	}
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusOK, res)
	return nil
}

// handleCodeIndexStatus answers GET /api/v1/admin/code-index.
func (a *api) handleCodeIndexStatus(w http.ResponseWriter, r *http.Request) error {
	if err := a.requireServerAdmin(r); err != nil {
		return err
	}
	if err := a.codeSearchEnabled(); err != nil {
		return err
	}
	st, err := a.d.CodeSearch.IndexStatus(r.Context())
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusOK, st)
	return nil
}

// handleCodeIndexRebuild answers POST /api/v1/admin/code-index/rebuild:
// rebuilds one tree (tree=TENANT/TREE) or every indexed tree.
func (a *api) handleCodeIndexRebuild(w http.ResponseWriter, r *http.Request) error {
	if err := a.requireServerAdmin(r); err != nil {
		return err
	}
	if err := a.codeSearchEnabled(); err != nil {
		return err
	}
	// A rebuild outlives a client that disconnects.
	ctx := context.WithoutCancel(r.Context())
	ref := strings.TrimSpace(r.URL.Query().Get("tree"))
	if ref == "" {
		res, err := a.d.CodeSearch.RebuildAll(ctx)
		if err != nil {
			return err
		}
		a.audit(r, auditCodeIndexRebuild, "", "code_index", "*", map[string]string{"trees": strconv.Itoa(res.Rebuilt)})
		writeJSON(w, http.StatusOK, res)
		return nil
	}
	tenantRef, name, ok := strings.Cut(ref, "/")
	if !ok || tenantRef == "" || name == "" {
		return badRequestf("tree must be TENANT/TREE")
	}
	t, err := a.tenantByRef(ctx, tenantRef)
	if err != nil {
		return err
	}
	tree, err := a.d.Store.GetTreeByName(ctx, t.ID, name)
	if errors.Is(err, domain.ErrNotFound) {
		return notFoundf("tree %s/%s not found", t.Slug, name)
	}
	if err != nil {
		return err
	}
	st, err := a.d.CodeSearch.Rebuild(ctx, tree)
	if err != nil {
		return err
	}
	a.audit(r, auditCodeIndexRebuild, t.ID, "tree", tree.ID, map[string]string{"tree": t.Slug + "/" + tree.Name})
	writeJSON(w, http.StatusOK, codesearch.RebuildResult{Rebuilt: 1, Trees: []codesearch.CodeIndexTree{st}})
	return nil
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
