package httpapi

import (
	"context"
	"net/http"
	"strings"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/mirror"
)

// registerMirrorRoutes registers the v0.7 routes for Git push mirrors of a
// tree (docs/ENGINEERING_PLAN.md §15.6). Every route needs tree:admin;
// without Deps.Mirrors they answer 501.
//
//	GET|POST          {tree}/mirrors
//	GET|PATCH|DELETE  {tree}/mirrors/{id}
//	POST              {tree}/mirrors/{id}/sync
func (a *api) registerMirrorRoutes(tree string) {
	a.handle("GET "+tree+"/mirrors", a.handleListMirrors)
	a.handle("POST "+tree+"/mirrors", a.handleCreateMirror)
	a.handle("GET "+tree+"/mirrors/{id}", a.handleGetMirror)
	a.handle("PATCH "+tree+"/mirrors/{id}", a.handlePatchMirror)
	a.handle("DELETE "+tree+"/mirrors/{id}", a.handleDeleteMirror)
	a.handle("POST "+tree+"/mirrors/{id}/sync", a.handleSyncMirror)
}

// mirrorScope resolves the service and the tree of r (tree:admin).
func (a *api) mirrorScope(r *http.Request) (*mirror.Service, *domain.Tenant, *domain.Tree, error) {
	if a.d.Mirrors == nil {
		return nil, nil, nil, errIAMNotImplemented("Git mirrors")
	}
	t, tree, err := a.treeFor(r, domain.ActTreeAdmin)
	if err != nil {
		return nil, nil, nil, err
	}
	return a.d.Mirrors, t, tree, nil
}

// mirrorList is the body of GET {tree}/mirrors.
type mirrorList struct {
	Mirrors []domain.Mirror `json:"mirrors"`
	// Git reports whether the server can run mirrors.
	Git mirror.GitStatus `json:"git"`
}

// mirrorSync is the body of POST {tree}/mirrors/{id}/sync.
type mirrorSync struct {
	Mirror domain.Mirror      `json:"mirror"`
	Result *mirror.SyncResult `json:"result,omitempty"`
}

func mirrorAuditDetails(tree *domain.Tree, m *domain.Mirror) map[string]string {
	return map[string]string{"tree": tree.Name, "url": m.URL, "branches": strings.Join(m.Branches, ",")}
}

// handleListMirrors answers GET {tree}/mirrors.
func (a *api) handleListMirrors(w http.ResponseWriter, r *http.Request) error {
	svc, _, tree, err := a.mirrorScope(r)
	if err != nil {
		return err
	}
	list, err := svc.List(r.Context(), tree)
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusOK, mirrorList{Mirrors: nonNil(list), Git: svc.Git()})
	return nil
}

// handleCreateMirror answers POST {tree}/mirrors {url, branches?, tags?,
// credential?, active?}.
func (a *api) handleCreateMirror(w http.ResponseWriter, r *http.Request) error {
	p, err := requireLogin(r)
	if err != nil {
		return err
	}
	svc, t, tree, err := a.mirrorScope(r)
	if err != nil {
		return err
	}
	var in mirror.CreateInput
	if err := decodeJSON(w, r, &in); err != nil {
		return err
	}
	m, err := svc.Create(r.Context(), p, tree, in)
	if err != nil {
		return err
	}
	a.audit(r, "mirror.create", t.ID, "mirror", m.ID, mirrorAuditDetails(tree, m))
	writeJSON(w, http.StatusCreated, m)
	return nil
}

// handleGetMirror answers GET {tree}/mirrors/{id}.
func (a *api) handleGetMirror(w http.ResponseWriter, r *http.Request) error {
	svc, _, tree, err := a.mirrorScope(r)
	if err != nil {
		return err
	}
	m, err := svc.Get(r.Context(), tree, r.PathValue("id"))
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusOK, m)
	return nil
}

// handlePatchMirror answers PATCH {tree}/mirrors/{id} {url?, branches?,
// tags?, credential?, active?}.
func (a *api) handlePatchMirror(w http.ResponseWriter, r *http.Request) error {
	if _, err := requireLogin(r); err != nil {
		return err
	}
	svc, t, tree, err := a.mirrorScope(r)
	if err != nil {
		return err
	}
	var patch mirror.Patch
	if err := decodeJSON(w, r, &patch); err != nil {
		return err
	}
	m, changed, err := svc.Update(r.Context(), tree, r.PathValue("id"), patch)
	if err != nil {
		return err
	}
	details := mirrorAuditDetails(tree, m)
	details["changed"] = strings.Join(changed, ",")
	a.audit(r, "mirror.update", t.ID, "mirror", m.ID, details)
	writeJSON(w, http.StatusOK, m)
	return nil
}

// handleDeleteMirror answers DELETE {tree}/mirrors/{id}.
func (a *api) handleDeleteMirror(w http.ResponseWriter, r *http.Request) error {
	if _, err := requireLogin(r); err != nil {
		return err
	}
	svc, t, tree, err := a.mirrorScope(r)
	if err != nil {
		return err
	}
	id := r.PathValue("id")
	m, err := svc.Get(r.Context(), tree, id)
	if err != nil {
		return err
	}
	if err := svc.Delete(r.Context(), tree, id); err != nil {
		return err
	}
	a.audit(r, "mirror.delete", t.ID, "mirror", id, mirrorAuditDetails(tree, m))
	w.WriteHeader(http.StatusNoContent)
	return nil
}

// handleSyncMirror answers POST {tree}/mirrors/{id}/sync[?wait=false]: by
// default it exports and pushes now and answers 200 {mirror, result} (a
// failed push is reported in the result and the mirror's status, not as an
// error status); with wait=false it starts the sync in the background and
// answers 202 {mirror}.
func (a *api) handleSyncMirror(w http.ResponseWriter, r *http.Request) error {
	p, err := requireLogin(r)
	if err != nil {
		return err
	}
	svc, t, tree, err := a.mirrorScope(r)
	if err != nil {
		return err
	}
	wait := true
	switch v := r.URL.Query().Get("wait"); v {
	case "", "true", "1":
	case "false", "0":
		wait = false
	default:
		return badRequestf("wait must be true or false, got %q", v)
	}
	id := r.PathValue("id")
	m, err := svc.Get(r.Context(), tree, id)
	if err != nil {
		return err
	}
	if !m.Active {
		return &apiError{status: http.StatusConflict, code: codeConflict, msg: "mirror " + id + " is inactive; activate it before syncing"}
	}
	if st := svc.Git(); !st.Available {
		return &apiError{status: http.StatusConflict, code: codeConflict, msg: st.Error}
	}
	a.audit(r, "mirror.sync", t.ID, "mirror", id, mirrorAuditDetails(tree, m))
	if !wait {
		ctx := context.WithoutCancel(r.Context())
		go func() {
			if _, _, err := svc.Sync(ctx, tree, id, p.Username); err != nil {
				a.log.WarnContext(ctx, "mirror sync failed", "mirror", id, "error", err)
			}
		}()
		writeJSON(w, http.StatusAccepted, mirrorSync{Mirror: *m})
		return nil
	}
	out, res, err := svc.Sync(r.Context(), tree, id, p.Username)
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusOK, mirrorSync{Mirror: *out, Result: res})
	return nil
}
