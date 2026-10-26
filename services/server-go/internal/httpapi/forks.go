package httpapi

// v0.8 forks (docs/ENGINEERING_PLAN.md §16.2, specs/tree/Forks.md):
//
//	POST {tree}/forks                      fork the tree into a tenant
//	GET  {tree}/forks                      forks of the tree the caller may read
//	GET  {tree}/fork                       the tree's upstream and branch relations
//	POST {tree}/fork/sync                  fast-forward fork branches to the upstream
//	GET  {tree}/merge-requests/{n}/source  where a merge request's source lives
//
// Merge requests from a fork are opened with POST {tree}/merge-requests and
// source_tenant + source_tree (mergerequests.go, resolved by mrSourceTree).

import (
	"context"
	"errors"
	"net/http"
	"strings"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/fork"
	"github.com/seanfilimon/worktree/services/server-go/internal/review"
)

// createForkRequest is the body of POST {tree}/forks.
type createForkRequest struct {
	// Tenant receives the fork (slug; default: the caller's personal tenant).
	Tenant      string   `json:"tenant"`
	Name        string   `json:"name"`
	Description string   `json:"description"`
	Visibility  string   `json:"visibility"`
	Branches    []string `json:"branches"`
}

// syncForkRequest is the optional body of POST {tree}/fork/sync.
type syncForkRequest struct {
	Branches []string `json:"branches"`
}

// mrSourcer is the fork surface of internal/review.Service.
type mrSourcer interface {
	MergeRequestSource(ctx context.Context, p *domain.Principal, tree *domain.Tree, number int) (*review.MRSource, error)
}

var _ mrSourcer = (*review.Service)(nil)

func (a *api) registerForkRoutes(tree string) {
	a.handle("GET "+tree+"/forks", a.handleListForks)
	a.handle("POST "+tree+"/forks", a.handleCreateFork)
	a.handle("GET "+tree+"/fork", a.handleGetUpstream)
	a.handle("POST "+tree+"/fork/sync", a.handleSyncFork)
	a.handle("GET "+tree+"/merge-requests/{n}/source", a.handleMRSource)
}

func errForksDisabled() error {
	return &apiError{status: http.StatusNotImplemented, code: "not_implemented", msg: "forks are not available on this server"}
}

// handleCreateFork answers POST {tree}/forks with the new tree (201).
func (a *api) handleCreateFork(w http.ResponseWriter, r *http.Request) error {
	p, err := requireLogin(r)
	if err != nil {
		return err
	}
	_, tree, err := a.treeFor(r, domain.ActTreeRead)
	if err != nil {
		return err
	}
	if a.d.Forks == nil {
		return errForksDisabled()
	}
	var req createForkRequest
	if err := decodeJSON(w, r, &req); err != nil {
		return err
	}
	if err := validateText("description", req.Description, maxDescription, false); err != nil {
		return err
	}
	if len(req.Branches) > 1000 {
		return badRequestf("at most 1000 branches can be named")
	}
	for _, b := range req.Branches {
		if err := validateBranchName(b); err != nil {
			return badRequestf("branches: %v", err)
		}
	}
	ctx := r.Context()
	var dst *domain.Tenant
	if slug := strings.TrimSpace(req.Tenant); slug != "" {
		if dst, err = a.tenantByRef(ctx, slug); err != nil {
			return err
		}
	} else {
		if p.TenantID == "" {
			return badRequestf("tenant is required")
		}
		if dst, err = a.d.Store.GetTenant(ctx, p.TenantID); err != nil {
			return err
		}
	}
	f, err := a.d.Forks.Fork(ctx, p, tree, fork.Input{
		Tenant: dst, Name: req.Name, Description: req.Description, Visibility: req.Visibility, Branches: req.Branches,
	})
	if err != nil {
		if errors.Is(err, domain.ErrAlreadyExists) {
			return conflictf("%v", err)
		}
		return quotaErr(err)
	}
	writeJSON(w, http.StatusCreated, f)
	return nil
}

// handleListForks answers GET {tree}/forks.
func (a *api) handleListForks(w http.ResponseWriter, r *http.Request) error {
	_, tree, err := a.treeFor(r, domain.ActTreeRead)
	if err != nil {
		return err
	}
	if a.d.Forks == nil {
		return errForksDisabled()
	}
	list, err := a.d.Forks.List(r.Context(), principal(r), tree)
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusOK, list)
	return nil
}

// handleGetUpstream answers GET {tree}/fork.
func (a *api) handleGetUpstream(w http.ResponseWriter, r *http.Request) error {
	_, tree, err := a.treeFor(r, domain.ActTreeRead)
	if err != nil {
		return err
	}
	if a.d.Forks == nil {
		return errForksDisabled()
	}
	u, err := a.d.Forks.Upstream(r.Context(), principal(r), tree)
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusOK, u)
	return nil
}

// handleSyncFork answers POST {tree}/fork/sync.
func (a *api) handleSyncFork(w http.ResponseWriter, r *http.Request) error {
	p, err := requireLogin(r)
	if err != nil {
		return err
	}
	_, tree, err := a.treeFor(r, domain.ActTreeRead)
	if err != nil {
		return err
	}
	if a.d.Forks == nil {
		return errForksDisabled()
	}
	var req syncForkRequest
	if err := decodeOptionalJSON(w, r, &req); err != nil {
		return err
	}
	for _, b := range req.Branches {
		if err := validateBranchName(b); err != nil {
			return badRequestf("branches: %v", err)
		}
	}
	res, err := a.d.Forks.Sync(r.Context(), p, tree, req.Branches)
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusOK, res)
	return nil
}

// handleMRSource answers GET {tree}/merge-requests/{n}/source.
func (a *api) handleMRSource(w http.ResponseWriter, r *http.Request) error {
	_, tree, err := a.treeFor(r, domain.ActTreeRead)
	if err != nil {
		return err
	}
	n, err := mrNumber(r)
	if err != nil {
		return err
	}
	ms, ok := a.d.Review.(mrSourcer)
	if !ok {
		return &apiError{status: http.StatusNotImplemented, code: "not_implemented", msg: "merge request sources are not available on this server"}
	}
	src, err := ms.MergeRequestSource(r.Context(), principal(r), tree, n)
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusOK, src)
	return nil
}

// mrSourceTree resolves source_tenant/source_tree of a merge request create
// (nil when absent). The caller must be able to read the source tree.
func (a *api) mrSourceTree(r *http.Request, req *createMRRequest) (*domain.Tree, error) {
	req.SourceTenant, req.SourceTree = strings.TrimSpace(req.SourceTenant), strings.TrimSpace(req.SourceTree)
	if req.SourceTree == "" && req.SourceTenant == "" {
		return nil, nil
	}
	if req.SourceTree == "" || req.SourceTenant == "" {
		return nil, badRequestf("source_tenant and source_tree go together")
	}
	ctx := r.Context()
	t, err := a.tenantByRef(ctx, req.SourceTenant)
	if err != nil {
		return nil, err
	}
	src, err := a.d.Store.GetTreeByName(ctx, t.ID, req.SourceTree)
	if errors.Is(err, domain.ErrNotFound) {
		return nil, notFoundf("tree %s/%s not found", t.Slug, req.SourceTree)
	}
	if err != nil {
		return nil, err
	}
	if err := a.d.Authz.Authorize(ctx, principal(r), domain.ActTreeRead, t.ID, src); err != nil {
		if isDenied(err) {
			return nil, notFoundf("tree %s/%s not found", t.Slug, req.SourceTree)
		}
		return nil, err
	}
	return src, nil
}
