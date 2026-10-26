package httpapi

import (
	"errors"
	"net/http"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/treeconfig"
)

// config-as-code (docs/ENGINEERING_PLAN.md §16.3,
// specs/dot-wt-tree/ConfigAsCode.md): the applied .wt-tree configuration,
// validation of files before pushing them, and branch metadata
// (description, owner). Served by internal/treeconfig behind
// Deps.TreeConfig; without it the routes answer 501.

// registerTreeConfigRoutes registers the v0.8 config-as-code routes.
func (a *api) registerTreeConfigRoutes(tree string) {
	a.handle("GET "+tree+"/config", a.handleGetTreeConfig)
	a.handle("POST "+tree+"/config/validate", a.handleValidateTreeConfig)
	a.handle("PATCH "+tree+"/branches/{name...}", a.handlePatchBranch)
}

func (a *api) treeConfigSvc() (*treeconfig.Service, error) {
	if a.d.TreeConfig == nil {
		return nil, &apiError{status: http.StatusNotImplemented, code: "not_implemented",
			msg: "config-as-code is not available on this server"}
	}
	return a.d.TreeConfig, nil
}

// handleGetTreeConfig answers GET {tree}/config (tree:read).
func (a *api) handleGetTreeConfig(w http.ResponseWriter, r *http.Request) error {
	_, tree, err := a.treeFor(r, domain.ActTreeRead)
	if err != nil {
		return err
	}
	svc, err := a.treeConfigSvc()
	if err != nil {
		return err
	}
	st, err := svc.Get(r.Context(), principal(r), tree)
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusOK, st)
	return nil
}

// validateTreeConfigRequest is the body of POST {tree}/config/validate.
type validateTreeConfigRequest struct {
	// Files maps tree-relative paths (.wt-tree/branches.toml, ...) to
	// their content.
	Files map[string]string `json:"files"`
}

// handleValidateTreeConfig answers POST {tree}/config/validate
// (tree:write): the files are validated as if pushed to the default
// branch; nothing is changed.
func (a *api) handleValidateTreeConfig(w http.ResponseWriter, r *http.Request) error {
	if _, err := requireLogin(r); err != nil {
		return err
	}
	_, tree, err := a.treeFor(r, domain.ActTreeRead)
	if err != nil {
		return err
	}
	svc, err := a.treeConfigSvc()
	if err != nil {
		return err
	}
	var req validateTreeConfigRequest
	if err := decodeJSON(w, r, &req); err != nil {
		return err
	}
	if len(req.Files) == 0 {
		return badRequestf("files is required (path → content)")
	}
	res, err := svc.ValidateFiles(r.Context(), principal(r), tree, req.Files)
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusOK, res)
	return nil
}

// handlePatchBranch answers PATCH {tree}/branches/{name...} with
// {"description"?, "owner"?} (description: tree:write, owner: tree:admin).
// Branches declared in the applied .wt-tree/branches.toml answer 409
// config_managed.
func (a *api) handlePatchBranch(w http.ResponseWriter, r *http.Request) error {
	if _, err := requireLogin(r); err != nil {
		return err
	}
	_, tree, err := a.treeFor(r, domain.ActTreeRead)
	if err != nil {
		return err
	}
	svc, err := a.treeConfigSvc()
	if err != nil {
		return err
	}
	name := r.PathValue("name")
	if name == "" {
		return badRequestf("branch name is required")
	}
	var req treeconfig.BranchUpdate
	if err := decodeJSON(w, r, &req); err != nil {
		return err
	}
	b, err := svc.UpdateBranch(r.Context(), principal(r), tree, name, req)
	switch {
	case errors.Is(err, treeconfig.ErrConfigManaged):
		return &apiError{status: http.StatusConflict, code: "config_managed",
			msg: "branch " + name + " is declared in .wt-tree/branches.toml; change the file and push it"}
	case errors.Is(err, domain.ErrNotFound):
		return notFoundf("branch %q not found", name)
	case err != nil:
		return err
	}
	writeJSON(w, http.StatusOK, b)
	return nil
}
