package httpapi

import (
	"net/http"
	"strconv"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/notify"
)

// registerNotificationRoutes registers the v0.7 routes for notifications
// and watch settings (docs/ENGINEERING_PLAN.md §15.3). Every route acts on
// the caller's own account; without Deps.Notify they answer 501.
func (a *api) registerNotificationRoutes(tree string) {
	const base = "/api/v1/notifications"
	a.handle("GET "+base, a.handleListNotifications)
	a.handle("POST "+base+"/read-all", a.handleReadAllNotifications)
	a.handle("POST "+base+"/{id}/read", a.handleReadNotification)
	a.handle("DELETE "+base+"/{id}", a.handleDeleteNotification)
	a.handle("GET "+base+"/settings", a.handleGetNotificationSettings)
	a.handle("PUT "+base+"/settings", a.handlePutNotificationSettings)
	a.handle("GET "+base+"/watches", a.handleListWatches)
	a.handle("GET "+tree+"/watch", a.handleGetWatch)
	a.handle("PUT "+tree+"/watch", a.handlePutWatch)
	a.handle("GET /api/v1/tenants/{tenant}/watch", a.handleGetWatch)
	a.handle("PUT /api/v1/tenants/{tenant}/watch", a.handlePutWatch)
}

// notifyAccount returns the service and the caller's account id (user
// accounts only: service accounts have no inbox).
func (a *api) notifyAccount(r *http.Request) (*notify.Service, string, error) {
	if a.d.Notify == nil {
		return nil, "", errIAMNotImplemented("Notifications")
	}
	p, err := requireLogin(r)
	if err != nil {
		return nil, "", err
	}
	if p.AccountID == "" {
		return nil, "", forbiddenf("notifications belong to user accounts; service accounts have no inbox")
	}
	return a.d.Notify, p.AccountID, nil
}

// handleListNotifications answers GET /notifications?unread=true&limit=.
func (a *api) handleListNotifications(w http.ResponseWriter, r *http.Request) error {
	svc, acct, err := a.notifyAccount(r)
	if err != nil {
		return err
	}
	unread := false
	if v := r.URL.Query().Get("unread"); v != "" {
		if unread, err = strconv.ParseBool(v); err != nil {
			return badRequestf("unread must be true or false")
		}
	}
	limit, err := queryInt(r, "limit", notify.DefaultListLimit, notify.MaxListLimit)
	if err != nil {
		return err
	}
	res, err := svc.List(r.Context(), acct, unread, limit)
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusOK, res)
	return nil
}

// handleReadNotification answers POST /notifications/{id}/read.
func (a *api) handleReadNotification(w http.ResponseWriter, r *http.Request) error {
	svc, acct, err := a.notifyAccount(r)
	if err != nil {
		return err
	}
	n, err := svc.MarkRead(r.Context(), acct, r.PathValue("id"))
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusOK, n)
	return nil
}

type readAllResult struct {
	Updated int `json:"updated"`
}

// handleReadAllNotifications answers POST /notifications/read-all.
func (a *api) handleReadAllNotifications(w http.ResponseWriter, r *http.Request) error {
	svc, acct, err := a.notifyAccount(r)
	if err != nil {
		return err
	}
	n, err := svc.MarkAllRead(r.Context(), acct)
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusOK, readAllResult{Updated: n})
	return nil
}

// handleDeleteNotification answers DELETE /notifications/{id}.
func (a *api) handleDeleteNotification(w http.ResponseWriter, r *http.Request) error {
	svc, acct, err := a.notifyAccount(r)
	if err != nil {
		return err
	}
	if err := svc.Delete(r.Context(), acct, r.PathValue("id")); err != nil {
		return err
	}
	w.WriteHeader(http.StatusNoContent)
	return nil
}

// handleGetNotificationSettings answers GET /notifications/settings.
func (a *api) handleGetNotificationSettings(w http.ResponseWriter, r *http.Request) error {
	svc, acct, err := a.notifyAccount(r)
	if err != nil {
		return err
	}
	st, err := svc.GetSettings(r.Context(), acct)
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusOK, st)
	return nil
}

// handlePutNotificationSettings answers PUT /notifications/settings {email?}.
func (a *api) handlePutNotificationSettings(w http.ResponseWriter, r *http.Request) error {
	svc, acct, err := a.notifyAccount(r)
	if err != nil {
		return err
	}
	var in notify.SettingsPatch
	if err := decodeJSON(w, r, &in); err != nil {
		return err
	}
	st, err := svc.PutSettings(r.Context(), acct, in)
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusOK, st)
	return nil
}

// watchView is a WatchSetting plus the names of its tenant and tree.
type watchView struct {
	domain.WatchSetting
	Tenant string `json:"tenant"`
	Tree   string `json:"tree,omitempty"`
}

type watchList struct {
	Watches []watchView `json:"watches"`
}

// handleListWatches answers GET /notifications/watches: the caller's
// stored watch settings (with tenant slugs and tree names).
func (a *api) handleListWatches(w http.ResponseWriter, r *http.Request) error {
	svc, acct, err := a.notifyAccount(r)
	if err != nil {
		return err
	}
	ctx := r.Context()
	list, err := svc.Watches(ctx, acct)
	if err != nil {
		return err
	}
	out := watchList{Watches: []watchView{}}
	for _, ws := range list {
		v := watchView{WatchSetting: ws, Tenant: ws.TenantID}
		if t, err := a.d.Store.GetTenant(ctx, ws.TenantID); err == nil {
			v.Tenant = t.Slug
		} else {
			continue // tenant gone
		}
		if ws.TreeID != "" {
			tr, err := a.d.Store.GetTree(ctx, ws.TreeID)
			if err != nil {
				continue // tree gone
			}
			v.Tree = tr.Name
		}
		out.Watches = append(out.Watches, v)
	}
	writeJSON(w, http.StatusOK, out)
	return nil
}

// watchScope resolves the tenant (tenant:read) or tree (tree:read) of a
// watch route.
func (a *api) watchScope(r *http.Request) (tenantID, treeID string, err error) {
	if r.PathValue("tree") != "" {
		t, tree, err := a.treeFor(r, domain.ActTreeRead)
		if err != nil {
			return "", "", err
		}
		return t.ID, tree.ID, nil
	}
	t, err := a.tenantFor(r, domain.ActTenantRead)
	if err != nil {
		return "", "", err
	}
	return t.ID, "", nil
}

// handleGetWatch answers GET {tree}/watch and GET /tenants/{t}/watch.
func (a *api) handleGetWatch(w http.ResponseWriter, r *http.Request) error {
	svc, acct, err := a.notifyAccount(r)
	if err != nil {
		return err
	}
	tenantID, treeID, err := a.watchScope(r)
	if err != nil {
		return err
	}
	ws, err := svc.GetWatch(r.Context(), acct, tenantID, treeID)
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusOK, ws)
	return nil
}

// handlePutWatch answers PUT {tree}/watch and PUT /tenants/{t}/watch
// {level, email?}.
func (a *api) handlePutWatch(w http.ResponseWriter, r *http.Request) error {
	svc, acct, err := a.notifyAccount(r)
	if err != nil {
		return err
	}
	tenantID, treeID, err := a.watchScope(r)
	if err != nil {
		return err
	}
	var in notify.WatchPatch
	if err := decodeJSON(w, r, &in); err != nil {
		return err
	}
	ws, err := svc.PutWatch(r.Context(), acct, tenantID, treeID, in)
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusOK, ws)
	return nil
}
