package httpapi

import (
	"net/http"
	"strings"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/webhooks"
)

// registerWebhookRoutes registers the v0.6 routes for webhooks and their
// deliveries (docs/ENGINEERING_PLAN.md §14.2), once under the tenant
// (tenant:admin) and once under a tree (tree:admin). Without
// Deps.Webhooks they answer 501.
func (a *api) registerWebhookRoutes(tree string) {
	for _, base := range []string{"/api/v1/tenants/{tenant}", tree} {
		a.handle("GET "+base+"/webhooks", a.handleListWebhooks)
		a.handle("POST "+base+"/webhooks", a.handleCreateWebhook)
		a.handle("GET "+base+"/webhooks/{id}", a.handleGetWebhook)
		a.handle("PATCH "+base+"/webhooks/{id}", a.handlePatchWebhook)
		a.handle("DELETE "+base+"/webhooks/{id}", a.handleDeleteWebhook)
		a.handle("GET "+base+"/webhooks/{id}/deliveries", a.handleListDeliveries)
		a.handle("GET "+base+"/webhooks/{id}/deliveries/{did}", a.handleGetDelivery)
		a.handle("POST "+base+"/webhooks/{id}/deliveries/{did}/redeliver", a.handleRedeliver)
		a.handle("POST "+base+"/webhooks/{id}/test", a.handleTestWebhook)
	}
}

// webhookScope resolves the service and the tenant or tree scope of r,
// authorizing tenant:admin or tree:admin.
func (a *api) webhookScope(r *http.Request) (*webhooks.Service, webhooks.Scope, error) {
	if a.d.Webhooks == nil {
		return nil, webhooks.Scope{}, errIAMNotImplemented("Webhooks")
	}
	if r.PathValue("tree") != "" {
		t, tree, err := a.treeFor(r, domain.ActTreeAdmin)
		if err != nil {
			return nil, webhooks.Scope{}, err
		}
		return a.d.Webhooks, webhooks.Scope{Tenant: t, Tree: tree}, nil
	}
	t, err := a.tenantFor(r, domain.ActTenantAdmin)
	if err != nil {
		return nil, webhooks.Scope{}, err
	}
	return a.d.Webhooks, webhooks.Scope{Tenant: t}, nil
}

type webhookList struct {
	Webhooks []domain.Webhook `json:"webhooks"`
}

type deliveryList struct {
	Deliveries []domain.WebhookDelivery `json:"deliveries"`
}

// createdWebhook is the create response: the hook plus its secret when
// the server generated it (shown only this once).
type createdWebhook struct {
	domain.Webhook
	Secret string `json:"secret,omitempty"`
}

func webhookAuditDetails(sc webhooks.Scope, h *domain.Webhook) map[string]string {
	d := map[string]string{"url": h.URL, "events": strings.Join(h.Events, ",")}
	if sc.Tree != nil {
		d["tree"] = sc.Tree.Name
	}
	return d
}

// handleListWebhooks answers GET .../webhooks.
func (a *api) handleListWebhooks(w http.ResponseWriter, r *http.Request) error {
	svc, sc, err := a.webhookScope(r)
	if err != nil {
		return err
	}
	list, err := svc.List(r.Context(), sc)
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusOK, webhookList{Webhooks: nonNil(list)})
	return nil
}

// handleCreateWebhook answers POST .../webhooks {url, events[], secret?,
// description?, active?}.
func (a *api) handleCreateWebhook(w http.ResponseWriter, r *http.Request) error {
	p, err := requireLogin(r)
	if err != nil {
		return err
	}
	svc, sc, err := a.webhookScope(r)
	if err != nil {
		return err
	}
	var in webhooks.CreateInput
	if err := decodeJSON(w, r, &in); err != nil {
		return err
	}
	h, secret, err := svc.Create(r.Context(), p, sc, in)
	if err != nil {
		return err
	}
	a.audit(r, "webhook.create", sc.Tenant.ID, "webhook", h.ID, webhookAuditDetails(sc, h))
	writeJSON(w, http.StatusCreated, createdWebhook{Webhook: *h, Secret: secret})
	return nil
}

// handleGetWebhook answers GET .../webhooks/{id}.
func (a *api) handleGetWebhook(w http.ResponseWriter, r *http.Request) error {
	svc, sc, err := a.webhookScope(r)
	if err != nil {
		return err
	}
	h, err := svc.Get(r.Context(), sc, r.PathValue("id"))
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusOK, h)
	return nil
}

// handlePatchWebhook answers PATCH .../webhooks/{id} {url?, events?,
// secret?, description?, active?}.
func (a *api) handlePatchWebhook(w http.ResponseWriter, r *http.Request) error {
	if _, err := requireLogin(r); err != nil {
		return err
	}
	svc, sc, err := a.webhookScope(r)
	if err != nil {
		return err
	}
	var patch webhooks.Patch
	if err := decodeJSON(w, r, &patch); err != nil {
		return err
	}
	h, changed, err := svc.Update(r.Context(), sc, r.PathValue("id"), patch)
	if err != nil {
		return err
	}
	details := webhookAuditDetails(sc, h)
	details["changed"] = strings.Join(changed, ",")
	a.audit(r, "webhook.update", sc.Tenant.ID, "webhook", h.ID, details)
	writeJSON(w, http.StatusOK, h)
	return nil
}

// handleDeleteWebhook answers DELETE .../webhooks/{id}.
func (a *api) handleDeleteWebhook(w http.ResponseWriter, r *http.Request) error {
	if _, err := requireLogin(r); err != nil {
		return err
	}
	svc, sc, err := a.webhookScope(r)
	if err != nil {
		return err
	}
	id := r.PathValue("id")
	h, err := svc.Get(r.Context(), sc, id)
	if err != nil {
		return err
	}
	if err := svc.Delete(r.Context(), sc, id); err != nil {
		return err
	}
	a.audit(r, "webhook.delete", sc.Tenant.ID, "webhook", id, webhookAuditDetails(sc, h))
	w.WriteHeader(http.StatusNoContent)
	return nil
}

// handleListDeliveries answers GET .../webhooks/{id}/deliveries?limit=
// (newest first; default 50, at most the service's retention count).
func (a *api) handleListDeliveries(w http.ResponseWriter, r *http.Request) error {
	svc, sc, err := a.webhookScope(r)
	if err != nil {
		return err
	}
	limit, err := queryInt(r, "limit", 50, svc.Keep())
	if err != nil {
		return err
	}
	list, err := svc.Deliveries(r.Context(), sc, r.PathValue("id"), limit)
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusOK, deliveryList{Deliveries: nonNil(list)})
	return nil
}

// handleGetDelivery answers GET .../webhooks/{id}/deliveries/{did}.
func (a *api) handleGetDelivery(w http.ResponseWriter, r *http.Request) error {
	svc, sc, err := a.webhookScope(r)
	if err != nil {
		return err
	}
	d, err := svc.Delivery(r.Context(), sc, r.PathValue("id"), r.PathValue("did"))
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusOK, d)
	return nil
}

// handleRedeliver answers POST .../webhooks/{id}/deliveries/{did}/redeliver
// with the new delivery (attempted once, synchronously).
func (a *api) handleRedeliver(w http.ResponseWriter, r *http.Request) error {
	if _, err := requireLogin(r); err != nil {
		return err
	}
	svc, sc, err := a.webhookScope(r)
	if err != nil {
		return err
	}
	d, err := svc.Redeliver(r.Context(), sc, r.PathValue("id"), r.PathValue("did"))
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusCreated, d)
	return nil
}

// handleTestWebhook answers POST .../webhooks/{id}/test: a "ping"
// delivery, attempted once synchronously.
func (a *api) handleTestWebhook(w http.ResponseWriter, r *http.Request) error {
	p, err := requireLogin(r)
	if err != nil {
		return err
	}
	svc, sc, err := a.webhookScope(r)
	if err != nil {
		return err
	}
	d, err := svc.Ping(r.Context(), p, sc, r.PathValue("id"))
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusCreated, d)
	return nil
}
