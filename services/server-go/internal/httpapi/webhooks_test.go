package httpapi

import (
	"io"
	"log/slog"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync"
	"testing"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
	"github.com/seanfilimon/worktree/services/server-go/internal/webhooks"
)

// newWebhookFixture wires a webhooks.Service over depsStore (see
// todos_test.go). allowPrivate lets hooks target the local test receiver.
func newWebhookFixture(t *testing.T, allowPrivate bool) *fixture {
	t.Helper()
	return newFixture(t, func(d *Deps) {
		ds := &depsStore{manageStore: &manageStore{fakeStore: d.Store.(*fakeStore)}, DocStore: meta.NewMemory()}
		d.Store = ds
		svc := webhooks.New(webhooks.Deps{Store: ds, Authz: d.Authz, Bus: d.Bus, Audit: d.Audit,
			Logger: slog.New(slog.NewTextHandler(io.Discard, nil))})
		svc.Configure(webhooks.Options{AllowPrivate: allowPrivate, Version: "test-1.0"})
		d.Webhooks = svc
	})
}

type hookReceiver struct {
	mu   sync.Mutex
	hits []http.Header
	srv  *httptest.Server
}

func newHookReceiver(t *testing.T) *hookReceiver {
	rc := &hookReceiver{}
	rc.srv = httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		_, _ = io.Copy(io.Discard, r.Body)
		rc.mu.Lock()
		rc.hits = append(rc.hits, r.Header.Clone())
		rc.mu.Unlock()
		_, _ = io.WriteString(w, "ok")
	}))
	t.Cleanup(rc.srv.Close)
	return rc
}

func (rc *hookReceiver) count() int {
	rc.mu.Lock()
	defer rc.mu.Unlock()
	return len(rc.hits)
}

func TestWebhookEndpoints(t *testing.T) {
	f := newWebhookFixture(t, true)
	rc := newHookReceiver(t)
	do := func(method, path, as string, body any, want int) *httptest.ResponseRecorder {
		t.Helper()
		rec := f.do(t, method, path, as, body)
		if rec.Code != want {
			t.Fatalf("%s %s as %q: %d, want %d; %s", method, path, as, rec.Code, want, rec.Body.String())
		}
		return rec
	}
	const tenantHooks = "/api/v1/tenants/acme/webhooks"
	const treeHooks = "/api/v1/trees/acme/web/webhooks"
	body := map[string]any{"url": rc.srv.URL + "/hook", "events": []string{"branch.pushed", "mr.*"}, "description": "CI"}

	// Create: tenant scope needs tenant:admin, tree scope tree:admin.
	created := decode[createdWebhook](t, do("POST", tenantHooks, "alice", body, 201))
	if created.ID == "" || !strings.HasPrefix(created.Secret, "whsec_") || !created.HasSecret || !created.Active || created.CreatedBy != "alice" {
		t.Fatalf("created = %+v", created)
	}
	do("POST", tenantHooks, "bob", body, 403)
	do("POST", tenantHooks, "carol", body, 403)
	do("POST", tenantHooks, "", body, 401)
	do("POST", tenantHooks, "alice", map[string]any{"url": rc.srv.URL, "events": []string{}}, 400)
	do("POST", tenantHooks, "alice", map[string]any{"url": "gopher://x", "events": []string{"*"}}, 400)
	do("POST", tenantHooks, "alice", map[string]any{"url": rc.srv.URL, "events": []string{"*"}, "bogus": true}, 400)
	do("POST", "/api/v1/tenants/nope/webhooks", "alice", body, 404)

	treeBody := map[string]any{"url": rc.srv.URL + "/tree", "events": []string{"*"}, "secret": "given"}
	treeHook := decode[createdWebhook](t, do("POST", treeHooks, "mallory", treeBody, 201))
	if treeHook.Secret != "" || treeHook.TreeID != f.web.ID {
		t.Fatalf("tree hook = %+v", treeHook)
	}
	if strings.Contains(f.do(t, "GET", treeHooks+"/"+treeHook.ID, "alice", nil).Body.String(), "given") {
		t.Fatal("secret returned by GET")
	}
	do("POST", treeHooks, "bob", treeBody, 403)

	// List and get are scoped.
	if l := decode[webhookList](t, do("GET", tenantHooks, "alice", nil, 200)); len(l.Webhooks) != 1 || l.Webhooks[0].ID != created.ID {
		t.Fatalf("tenant list = %+v", l)
	}
	if l := decode[webhookList](t, do("GET", treeHooks, "alice", nil, 200)); len(l.Webhooks) != 1 || l.Webhooks[0].ID != treeHook.ID {
		t.Fatalf("tree list = %+v", l)
	}
	do("GET", tenantHooks, "bob", nil, 403)
	do("GET", tenantHooks+"/"+treeHook.ID, "alice", nil, 404)
	do("GET", "/api/v1/trees/acme/docs/webhooks/"+treeHook.ID, "alice", nil, 404)
	got := decode[domain.Webhook](t, do("GET", tenantHooks+"/"+created.ID, "mallory", nil, 200))
	if got.URL != rc.srv.URL+"/hook" || got.Description != "CI" {
		t.Fatalf("get = %+v", got)
	}

	// Patch.
	patched := decode[domain.Webhook](t, do("PATCH", tenantHooks+"/"+created.ID, "alice", map[string]any{"events": []string{"*"}, "active": false}, 200))
	if patched.Active || len(patched.Events) != 1 || patched.Events[0] != "*" {
		t.Fatalf("patched = %+v", patched)
	}
	do("PATCH", tenantHooks+"/"+created.ID, "alice", map[string]any{"events": []string{"no way"}}, 400)
	do("PATCH", tenantHooks+"/"+created.ID, "bob", map[string]any{"active": true}, 403)
	do("PATCH", tenantHooks+"/missing", "alice", map[string]any{"active": true}, 404)

	// Test ping, deliveries, redeliver.
	ping := decode[domain.WebhookDelivery](t, do("POST", tenantHooks+"/"+created.ID+"/test", "alice", nil, 201))
	if !ping.Succeeded || ping.Event != webhooks.EventPing || ping.StatusCode != 200 || rc.count() != 1 {
		t.Fatalf("ping = %+v (hits %d)", ping, rc.count())
	}
	do("POST", tenantHooks+"/"+created.ID+"/test", "bob", nil, 403)
	list := decode[deliveryList](t, do("GET", tenantHooks+"/"+created.ID+"/deliveries?limit=10", "alice", nil, 200))
	if len(list.Deliveries) != 1 || list.Deliveries[0].ID != ping.ID {
		t.Fatalf("deliveries = %+v", list)
	}
	do("GET", tenantHooks+"/"+created.ID+"/deliveries?limit=0", "alice", nil, 400)
	one := decode[domain.WebhookDelivery](t, do("GET", tenantHooks+"/"+created.ID+"/deliveries/"+ping.ID, "alice", nil, 200))
	if one.ID != ping.ID || len(one.Payload) == 0 {
		t.Fatalf("delivery = %+v", one)
	}
	do("GET", tenantHooks+"/"+created.ID+"/deliveries/nope", "alice", nil, 404)
	re := decode[domain.WebhookDelivery](t, do("POST", tenantHooks+"/"+created.ID+"/deliveries/"+ping.ID+"/redeliver", "alice", nil, 201))
	if re.ID == ping.ID || !re.Redelivery || !re.Succeeded || rc.count() != 2 {
		t.Fatalf("redelivery = %+v", re)
	}
	do("POST", tenantHooks+"/"+created.ID+"/deliveries/nope/redeliver", "alice", nil, 404)
	if l := decode[deliveryList](t, do("GET", treeHooks+"/"+treeHook.ID+"/deliveries", "mallory", nil, 200)); l.Deliveries == nil || len(l.Deliveries) != 0 {
		t.Fatalf("tree deliveries = %+v", l)
	}

	// Delete.
	do("DELETE", tenantHooks+"/"+created.ID, "bob", nil, 403)
	do("DELETE", tenantHooks+"/"+created.ID, "alice", nil, 204)
	do("GET", tenantHooks+"/"+created.ID, "alice", nil, 404)
	do("DELETE", tenantHooks+"/"+created.ID, "alice", nil, 404)

	for _, a := range []string{"webhook.create", "webhook.update", "webhook.delete"} {
		hasAudit(t, f, a)
	}
}

func TestWebhookSSRFRefusedByDefault(t *testing.T) {
	f := newWebhookFixture(t, false)
	for _, u := range []string{"http://127.0.0.1:9/x", "http://169.254.169.254/", "http://[::1]/", "http://10.0.0.8/"} {
		rec := f.do(t, "POST", "/api/v1/tenants/acme/webhooks", "alice", map[string]any{"url": u, "events": []string{"*"}})
		if rec.Code != 400 || !strings.Contains(rec.Body.String(), "webhook-allow-private") {
			t.Fatalf("%s: %d %s", u, rec.Code, rec.Body.String())
		}
	}
}

func TestWebhookEndpointsWithoutService(t *testing.T) {
	f := newFixture(t)
	for _, c := range []struct{ method, path string }{
		{"GET", "/api/v1/tenants/acme/webhooks"},
		{"POST", "/api/v1/tenants/acme/webhooks/x/test"},
		{"GET", "/api/v1/trees/acme/web/webhooks/x/deliveries"},
	} {
		if rec := f.do(t, c.method, c.path, "alice", nil); rec.Code != http.StatusNotImplemented {
			t.Fatalf("%s %s: %d %s", c.method, c.path, rec.Code, rec.Body.String())
		}
	}
}
