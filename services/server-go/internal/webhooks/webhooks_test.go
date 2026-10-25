package webhooks

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"log/slog"
	"net/http"
	"net/http/httptest"
	"net/netip"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/authz"
	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/events"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
)

// ---- fixture ---------------------------------------------------------------

var t0 = time.Date(2026, 5, 6, 7, 8, 9, 0, time.UTC)

func must(t testing.TB, err error) {
	t.Helper()
	if err != nil {
		t.Fatal(err)
	}
}

// received is one request seen by a receiver.
type received struct {
	Path   string
	Header http.Header
	Body   []byte
}

// receiver is an httptest server answering with a scripted status
// sequence (the last status repeats).
type receiver struct {
	srv *httptest.Server

	mu       sync.Mutex
	statuses []int
	got      []received
	notify   chan received
}

func newReceiver(t *testing.T, statuses ...int) *receiver {
	t.Helper()
	rc := &receiver{statuses: statuses, notify: make(chan received, 100)}
	rc.srv = httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		body, _ := io.ReadAll(r.Body)
		got := received{Path: r.URL.Path, Header: r.Header.Clone(), Body: body}
		rc.mu.Lock()
		rc.got = append(rc.got, got)
		status := http.StatusOK
		if len(rc.statuses) > 0 {
			status = rc.statuses[0]
			if len(rc.statuses) > 1 {
				rc.statuses = rc.statuses[1:]
			}
		}
		rc.mu.Unlock()
		rc.notify <- got
		w.WriteHeader(status)
		_, _ = io.WriteString(w, "thanks from "+r.URL.Path)
	}))
	t.Cleanup(rc.srv.Close)
	return rc
}

func (rc *receiver) url(path string) string { return rc.srv.URL + path }

func (rc *receiver) requests() []received {
	rc.mu.Lock()
	defer rc.mu.Unlock()
	return append([]received(nil), rc.got...)
}

func (rc *receiver) paths() []string {
	var out []string
	for _, r := range rc.requests() {
		out = append(out, r.Path)
	}
	return out
}

// env: tenants acme (alice owner, bob developer, carol viewer) and other
// (dave owner); private trees acme/web, acme/api, other/lib.
type env struct {
	t             *testing.T
	ctx           context.Context
	store         *meta.Memory
	bus           *events.Bus
	svc           *Service
	acme, other   domain.Tenant
	web, api, lib domain.Tree
	alice, dave   *domain.Principal
	clock         time.Time
	clockMu       sync.Mutex
}

func newEnv(t *testing.T, opts ...func(*Options)) *env {
	t.Helper()
	ctx := context.Background()
	store := meta.NewMemory()
	e := &env{t: t, ctx: ctx, store: store, bus: events.NewBus(), clock: t0}
	e.acme = domain.Tenant{ID: "t-acme", Slug: "acme", Name: "Acme", Kind: domain.TenantOrg, CreatedAt: t0}
	e.other = domain.Tenant{ID: "t-other", Slug: "other", Name: "Other", Kind: domain.TenantOrg, CreatedAt: t0}
	must(t, store.CreateTenant(ctx, &e.acme))
	must(t, store.CreateTenant(ctx, &e.other))
	roles := map[string][2]string{
		"alice": {e.acme.ID, domain.RoleOwner},
		"bob":   {e.acme.ID, domain.RoleDeveloper},
		"carol": {e.acme.ID, domain.RoleViewer},
		"dave":  {e.other.ID, domain.RoleOwner},
	}
	for _, u := range []string{"alice", "bob", "carol", "dave"} {
		personal := domain.Tenant{ID: "t-" + u, Slug: u, Kind: domain.TenantUser, CreatedAt: t0}
		must(t, store.CreateTenant(ctx, &personal))
		acct := domain.Account{ID: "a-" + u, Username: u, Email: u + "@x.test", TenantID: personal.ID, CreatedAt: t0}
		must(t, store.CreateAccount(ctx, &acct))
		r := roles[u]
		must(t, store.PutMembership(ctx, &domain.Membership{TenantID: r[0], AccountID: acct.ID, Role: r[1], CreatedAt: t0}))
		p := &domain.Principal{AccountID: acct.ID, Username: u, TenantID: personal.ID}
		switch u {
		case "alice":
			e.alice = p
		case "dave":
			e.dave = p
		}
	}
	mk := func(ten domain.Tenant, name string) domain.Tree {
		tr := domain.Tree{ID: "tree-" + name, TenantID: ten.ID, Name: name, DefaultBranch: "main", Visibility: domain.VisibilityPrivate, CreatedAt: t0, UpdatedAt: t0}
		must(t, store.CreateTree(ctx, &tr))
		return tr
	}
	e.web, e.api, e.lib = mk(e.acme, "web"), mk(e.acme, "api"), mk(e.other, "lib")
	e.svc = New(Deps{
		Store:  store,
		Authz:  authz.New(store, authz.Options{}),
		Bus:    e.bus,
		Logger: slog.New(slog.NewTextHandler(io.Discard, nil)),
	})
	o := Options{AllowPrivate: true, Version: "9.9.9", Now: e.now}
	for _, f := range opts {
		f(&o)
	}
	e.svc.Configure(o)
	return e
}

func (e *env) now() time.Time {
	e.clockMu.Lock()
	defer e.clockMu.Unlock()
	return e.clock
}

func (e *env) advance(d time.Duration) {
	e.clockMu.Lock()
	defer e.clockMu.Unlock()
	e.clock = e.clock.Add(d)
}

func (e *env) tenantScope() Scope { return Scope{Tenant: &e.acme} }

func (e *env) treeScope(tr *domain.Tree) Scope {
	ten := &e.acme
	if tr.TenantID == e.other.ID {
		ten = &e.other
	}
	return Scope{Tenant: ten, Tree: tr}
}

func (e *env) create(p *domain.Principal, sc Scope, in CreateInput) *domain.Webhook {
	e.t.Helper()
	h, _, err := e.svc.Create(e.ctx, p, sc, in)
	must(e.t, err)
	return h
}

func (e *env) push(tr *domain.Tree, branch string) domain.Event {
	return domain.Event{Type: domain.EventBranchPushed, TenantID: tr.TenantID, TreeID: tr.ID, Actor: "alice",
		Payload: map[string]any{"branch": branch, "old_tip": "", "new_tip": "snap-1"}, At: e.now()}
}

func (e *env) handle(ev domain.Event) {
	e.t.Helper()
	must(e.t, e.svc.HandleEvent(e.ctx, ev))
}

func (e *env) deliveries(sc Scope, id string) []domain.WebhookDelivery {
	e.t.Helper()
	list, err := e.svc.Deliveries(e.ctx, sc, id, 0)
	must(e.t, err)
	return list
}

func (e *env) hook(sc Scope, id string) *domain.Webhook {
	e.t.Helper()
	h, err := e.svc.Get(e.ctx, sc, id)
	must(e.t, err)
	return h
}

// ---- tests -------------------------------------------------------------------

func TestMatches(t *testing.T) {
	for _, c := range []struct {
		patterns []string
		typ      string
		want     bool
	}{
		{[]string{"*"}, "branch.pushed", true},
		{[]string{"branch.pushed"}, "branch.pushed", true},
		{[]string{"branch.pushed"}, "branch.created", false},
		{[]string{"mr.*"}, "mr.opened", true},
		{[]string{"mr.*"}, "mrx.opened", false},
		{[]string{"mr.*"}, "mr", false},
		{[]string{"tag.pushed", "mr.*"}, "mr.merged", true},
		{nil, "x", false},
	} {
		if got := Matches(c.patterns, c.typ); got != c.want {
			t.Errorf("Matches(%v, %q) = %v", c.patterns, c.typ, got)
		}
	}
	if _, err := NormalizeEvents([]string{"Branch.Pushed"}); !errors.Is(err, domain.ErrInvalid) {
		t.Fatalf("uppercase pattern: %v", err)
	}
	for _, bad := range [][]string{nil, {""}, {"*.x"}, {"mr.*.x"}, {"a b"}} {
		if _, err := NormalizeEvents(bad); !errors.Is(err, domain.ErrInvalid) {
			t.Errorf("NormalizeEvents(%q) = %v", bad, err)
		}
	}
	got, err := NormalizeEvents([]string{"mr.*", " mr.* ", "*"})
	if err != nil || len(got) != 2 {
		t.Fatalf("dedupe = %v, %v", got, err)
	}
}

func TestSignAndVerify(t *testing.T) {
	sig := Sign("key", []byte("The quick brown fox jumps over the lazy dog"))
	// Well-known HMAC-SHA256 test vector.
	if sig != "sha256=f7bc83f430538424b13298e6aa6fb143ef4d59a14946175997479dbc2d1a3cd8" {
		t.Fatalf("sig = %s", sig)
	}
	if !Verify("key", []byte("The quick brown fox jumps over the lazy dog"), sig) || Verify("other", []byte("x"), sig) {
		t.Fatal("verify")
	}
}

func TestCRUD(t *testing.T) {
	e := newEnv(t)
	rc := newReceiver(t)
	sc := e.tenantScope()

	h, secret, err := e.svc.Create(e.ctx, e.alice, sc, CreateInput{URL: rc.url("/a"), Events: []string{"branch.pushed"}, Description: "CI"})
	must(t, err)
	if !strings.HasPrefix(secret, "whsec_") || !h.HasSecret || !h.Active || h.CreatedBy != "alice" || h.TreeID != "" {
		t.Fatalf("created = %+v secret %q", h, secret)
	}
	inactive := false
	h2, secret2, err := e.svc.Create(e.ctx, e.alice, e.treeScope(&e.web), CreateInput{URL: rc.url("/b"), Events: []string{"*"}, Secret: "mine", Active: &inactive})
	must(t, err)
	if secret2 != "" || h2.Active || h2.TreeID != e.web.ID {
		t.Fatalf("tree hook = %+v secret %q", h2, secret2)
	}
	raw, _ := json.Marshal(h2)
	if strings.Contains(string(raw), "mine") {
		t.Fatalf("secret leaked: %s", raw)
	}

	list, err := e.svc.List(e.ctx, sc)
	must(t, err)
	if len(list) != 1 || list[0].ID != h.ID {
		t.Fatalf("tenant list = %+v", list)
	}
	list, err = e.svc.List(e.ctx, e.treeScope(&e.web))
	must(t, err)
	if len(list) != 1 || list[0].ID != h2.ID {
		t.Fatalf("tree list = %+v", list)
	}
	if _, err := e.svc.Get(e.ctx, e.treeScope(&e.api), h2.ID); !errors.Is(err, domain.ErrNotFound) {
		t.Fatalf("wrong scope: %v", err)
	}
	if _, err := e.svc.Get(e.ctx, sc, h2.ID); !errors.Is(err, domain.ErrNotFound) {
		t.Fatalf("tree hook via tenant scope: %v", err)
	}

	for _, in := range []CreateInput{
		{URL: "", Events: []string{"*"}},
		{URL: "ftp://x.test/", Events: []string{"*"}},
		{URL: "/relative", Events: []string{"*"}},
		{URL: rc.url("/"), Events: nil},
		{URL: rc.url("/"), Events: []string{"BAD"}},
		{URL: rc.url("/"), Events: []string{"*"}, Description: strings.Repeat("x", MaxDescription+1)},
		{URL: rc.url("/"), Events: []string{"*"}, Secret: strings.Repeat("x", MaxSecret+1)},
	} {
		if _, _, err := e.svc.Create(e.ctx, e.alice, sc, in); !errors.Is(err, domain.ErrInvalid) {
			t.Errorf("Create(%+v) = %v", in, err)
		}
	}
	if _, _, err := e.svc.Create(e.ctx, nil, sc, CreateInput{URL: rc.url("/"), Events: []string{"*"}}); !errors.Is(err, domain.ErrUnauthenticated) {
		t.Fatalf("anonymous create: %v", err)
	}

	desc, url := "renamed", rc.url("/c")
	up, changed, err := e.svc.Update(e.ctx, sc, h.ID, Patch{URL: &url, Events: []string{"mr.*", "tag.pushed"}, Description: &desc})
	must(t, err)
	if up.URL != url || up.Description != desc || len(up.Events) != 2 || strings.Join(changed, ",") != "url,events,description" {
		t.Fatalf("updated = %+v changed %v", up, changed)
	}
	if _, _, err := e.svc.Update(e.ctx, sc, h.ID, Patch{Events: []string{}}); !errors.Is(err, domain.ErrInvalid) {
		t.Fatalf("empty events: %v", err)
	}
	if _, _, err := e.svc.Update(e.ctx, sc, "nope", Patch{}); !errors.Is(err, domain.ErrNotFound) {
		t.Fatalf("missing: %v", err)
	}

	// Delete removes the hook and its log.
	if _, err := e.svc.Ping(e.ctx, e.alice, sc, h.ID); err != nil {
		t.Fatal(err)
	}
	must(t, e.svc.Delete(e.ctx, sc, h.ID))
	if _, err := e.svc.Get(e.ctx, sc, h.ID); !errors.Is(err, domain.ErrNotFound) {
		t.Fatalf("deleted: %v", err)
	}
	if raws, _ := e.store.ListDocs(e.ctx, CollDeliveries, meta.DocPrefix(h.ID)); len(raws) != 0 {
		t.Fatalf("deliveries left: %d", len(raws))
	}
	if err := e.svc.Delete(e.ctx, sc, h.ID); !errors.Is(err, domain.ErrNotFound) {
		t.Fatalf("delete twice: %v", err)
	}
}

func TestDeliverySignatureAndEnvelope(t *testing.T) {
	e := newEnv(t)
	rc := newReceiver(t)
	sc := e.tenantScope()
	h := e.create(e.alice, sc, CreateInput{URL: rc.url("/hook"), Events: []string{"branch.*"}, Secret: "s3cret"})

	e.handle(e.push(&e.web, "main"))
	reqs := rc.requests()
	if len(reqs) != 1 {
		t.Fatalf("requests = %d", len(reqs))
	}
	r := reqs[0]
	if r.Header.Get("Content-Type") != "application/json" || r.Header.Get("User-Agent") != "W0rkTree-Hookshot/9.9.9" ||
		r.Header.Get("X-WT-Event") != "branch.pushed" || r.Header.Get("X-WT-Delivery") == "" {
		t.Fatalf("headers = %v", r.Header)
	}
	if !Verify("s3cret", r.Body, r.Header.Get("X-WT-Signature")) {
		t.Fatalf("bad signature %q over %s", r.Header.Get("X-WT-Signature"), r.Body)
	}
	var env struct {
		ID, Event, Tenant, Tree, Actor string
		At                             time.Time
		Payload                        map[string]any
	}
	must(t, json.Unmarshal(r.Body, &env))
	if env.ID != r.Header.Get("X-WT-Delivery") || env.Event != "branch.pushed" || env.Tenant != "acme" || env.Tree != "web" ||
		env.Actor != "alice" || !env.At.Equal(t0) || env.Payload["branch"] != "main" {
		t.Fatalf("envelope = %+v", env)
	}

	ds := e.deliveries(sc, h.ID)
	if len(ds) != 1 {
		t.Fatalf("deliveries = %+v", ds)
	}
	d := ds[0]
	if d.ID != env.ID || !d.Succeeded || d.Attempts != 1 || d.StatusCode != 200 || d.Response != "thanks from /hook" ||
		d.NextRetryAt != nil || d.DeliveredAt == nil || string(d.Payload) != string(r.Body) {
		t.Fatalf("delivery = %+v", d)
	}
	got := e.hook(sc, h.ID)
	if got.LastStatus != 200 || got.LastDeliveryAt == nil || got.FailureCount != 0 {
		t.Fatalf("hook = %+v", got)
	}
	one, err := e.svc.Delivery(e.ctx, sc, h.ID, d.ID)
	must(t, err)
	if one.ID != d.ID {
		t.Fatalf("delivery = %+v", one)
	}
	if _, err := e.svc.Delivery(e.ctx, sc, h.ID, "nope"); !errors.Is(err, domain.ErrNotFound) {
		t.Fatalf("missing delivery: %v", err)
	}
}

func TestFilteringByPatternAndTree(t *testing.T) {
	e := newEnv(t)
	rc := newReceiver(t)
	off := false
	e.create(e.alice, e.tenantScope(), CreateInput{URL: rc.url("/tenant-mr"), Events: []string{"mr.*"}})
	e.create(e.alice, e.tenantScope(), CreateInput{URL: rc.url("/tenant-access"), Events: []string{"access.applied"}})
	e.create(e.alice, e.treeScope(&e.web), CreateInput{URL: rc.url("/web-all"), Events: []string{"*"}})
	e.create(e.alice, e.treeScope(&e.api), CreateInput{URL: rc.url("/api-push"), Events: []string{"branch.pushed"}})
	e.create(e.alice, e.tenantScope(), CreateInput{URL: rc.url("/inactive"), Events: []string{"*"}, Active: &off})
	e.create(e.dave, e.treeScope(&e.lib), CreateInput{URL: rc.url("/other"), Events: []string{"*"}})

	e.handle(domain.Event{Type: domain.EventMROpened, TenantID: e.acme.ID, TreeID: e.web.ID, Actor: "bob", Payload: map[string]any{"number": 1}})
	e.handle(e.push(&e.api, "main"))
	e.handle(domain.Event{Type: domain.EventAccessApplied, TenantID: e.acme.ID, Actor: "alice"})
	e.handle(domain.Event{Type: domain.EventBranchPushed, Actor: "x"}) // no tenant: ignored

	got := strings.Join(rc.paths(), ",")
	if got != "/tenant-mr,/web-all,/api-push,/tenant-access" && got != "/web-all,/tenant-mr,/api-push,/tenant-access" {
		t.Fatalf("deliveries went to %s", got)
	}
}

func TestPermissionRecheck(t *testing.T) {
	e := newEnv(t)
	rc := newReceiver(t)
	// dave is not a member of acme: a hook he created there (e.g. before
	// losing access) sees nothing of acme.
	e.create(e.dave, e.tenantScope(), CreateInput{URL: rc.url("/dave"), Events: []string{"*"}})
	e.create(e.alice, e.tenantScope(), CreateInput{URL: rc.url("/alice"), Events: []string{"*"}})

	e.handle(e.push(&e.web, "main"))
	e.handle(domain.Event{Type: domain.EventAccessApplied, TenantID: e.acme.ID})
	if got := strings.Join(rc.paths(), ","); got != "/alice,/alice" {
		t.Fatalf("paths = %s", got)
	}

	// Disabling the creator stops the deliveries.
	a, err := e.store.GetAccount(e.ctx, e.alice.AccountID)
	must(t, err)
	a.Disabled = true
	must(t, e.store.UpdateAccount(e.ctx, a))
	e.handle(e.push(&e.web, "main"))
	if n := len(rc.requests()); n != 2 {
		t.Fatalf("requests after disabling = %d", n)
	}
}

func TestRetriesWithBackoff(t *testing.T) {
	e := newEnv(t)
	rc := newReceiver(t, 500, 503, 200)
	sc := e.tenantScope()
	h := e.create(e.alice, sc, CreateInput{URL: rc.url("/r"), Events: []string{"*"}})

	e.handle(e.push(&e.web, "main"))
	d := e.deliveries(sc, h.ID)[0]
	if d.Succeeded || d.Attempts != 1 || d.StatusCode != 500 || d.NextRetryAt == nil || !d.NextRetryAt.Equal(t0.Add(time.Minute)) ||
		!strings.Contains(d.Error, "500") {
		t.Fatalf("after attempt 1: %+v", d)
	}
	if e.svc.Pending() != 1 {
		t.Fatalf("pending = %d", e.svc.Pending())
	}
	e.advance(30 * time.Second)
	if n := e.svc.RunDue(e.ctx); n != 0 {
		t.Fatalf("ran %d early", n)
	}
	e.advance(30 * time.Second)
	if n := e.svc.RunDue(e.ctx); n != 1 {
		t.Fatalf("ran %d", n)
	}
	d = e.deliveries(sc, h.ID)[0]
	if d.Attempts != 2 || d.StatusCode != 503 || !d.NextRetryAt.Equal(t0.Add(time.Minute+5*time.Minute)) {
		t.Fatalf("after attempt 2: %+v", d)
	}
	if got := e.hook(sc, h.ID); got.FailureCount != 2 || got.LastStatus != 503 {
		t.Fatalf("hook = %+v", got)
	}
	e.advance(5 * time.Minute)
	e.svc.RunDue(e.ctx)
	d = e.deliveries(sc, h.ID)[0]
	if !d.Succeeded || d.Attempts != 3 || d.NextRetryAt != nil || d.Error != "" {
		t.Fatalf("after attempt 3: %+v", d)
	}
	if got := e.hook(sc, h.ID); got.FailureCount != 0 || got.LastStatus != 200 {
		t.Fatalf("hook = %+v", got)
	}
	if e.svc.Pending() != 0 || len(rc.requests()) != 3 {
		t.Fatalf("pending %d, requests %d", e.svc.Pending(), len(rc.requests()))
	}
	// Every request of one delivery carries the same delivery id.
	reqs := rc.requests()
	if reqs[0].Header.Get("X-WT-Delivery") != reqs[2].Header.Get("X-WT-Delivery") {
		t.Fatal("delivery id changed between retries")
	}
}

func TestRetriesGiveUp(t *testing.T) {
	e := newEnv(t)
	rc := newReceiver(t, 500)
	sc := e.tenantScope()
	h := e.create(e.alice, sc, CreateInput{URL: rc.url("/r"), Events: []string{"*"}})
	e.handle(e.push(&e.web, "main"))
	for _, d := range DefaultBackoff {
		e.advance(d)
		if n := e.svc.RunDue(e.ctx); n != 1 {
			t.Fatalf("ran %d", n)
		}
	}
	d := e.deliveries(sc, h.ID)[0]
	if d.Attempts != 1+len(DefaultBackoff) || d.Succeeded || d.NextRetryAt != nil || e.svc.Pending() != 0 {
		t.Fatalf("gave up = %+v pending %d", d, e.svc.Pending())
	}
}

func TestDeactivationPublishesFailing(t *testing.T) {
	e := newEnv(t, func(o *Options) { o.FailureLimit = 3; o.Backoff = []time.Duration{} })
	rc := newReceiver(t, 500)
	sc := e.tenantScope()
	h := e.create(e.alice, sc, CreateInput{URL: rc.url("/down"), Events: []string{"branch.pushed"}})
	failing, cancel := e.bus.Subscribe(domain.EventFilter{Types: []string{domain.EventWebhookFailing}})
	defer cancel()

	for range 3 {
		e.handle(e.push(&e.web, "main"))
	}
	got := e.hook(sc, h.ID)
	if got.Active || got.FailureCount != 3 {
		t.Fatalf("hook = %+v", got)
	}
	select {
	case ev := <-failing:
		pl := ev.Payload.(map[string]any)
		if ev.TenantID != e.acme.ID || pl["webhook_id"] != h.ID || pl["failure_count"] != 3 || pl["last_status"] != 500 {
			t.Fatalf("event = %+v", ev)
		}
	case <-time.After(time.Second):
		t.Fatal("webhook.failing not published")
	}
	e.handle(e.push(&e.web, "main"))
	if n := len(rc.requests()); n != 3 {
		t.Fatalf("inactive hook got %d requests", n)
	}

	// Manual pings work on inactive hooks and do not count as failures.
	d, err := e.svc.Ping(e.ctx, e.alice, sc, h.ID)
	must(t, err)
	if d.Succeeded || d.Attempts != 1 || d.Event != EventPing {
		t.Fatalf("ping = %+v", d)
	}
	if got := e.hook(sc, h.ID); got.FailureCount != 3 {
		t.Fatalf("failure count = %d", got.FailureCount)
	}

	on := true
	re, _, err := e.svc.Update(e.ctx, sc, h.ID, Patch{Active: &on})
	must(t, err)
	if !re.Active || re.FailureCount != 0 {
		t.Fatalf("reactivated = %+v", re)
	}
}

func TestPingAndRedeliver(t *testing.T) {
	e := newEnv(t)
	rc := newReceiver(t, 500, 200)
	sc := e.treeScope(&e.web)
	h, secret, err := e.svc.Create(e.ctx, e.alice, sc, CreateInput{URL: rc.url("/x"), Events: []string{"branch.pushed"}})
	must(t, err)

	ping, err := e.svc.Ping(e.ctx, e.alice, sc, h.ID)
	must(t, err)
	if ping.Event != EventPing || ping.Succeeded || ping.StatusCode != 500 || ping.NextRetryAt != nil || e.svc.Pending() != 0 {
		t.Fatalf("ping = %+v", ping)
	}
	r := rc.requests()[0]
	if r.Header.Get("X-WT-Event") != "ping" || !Verify(secret, r.Body, r.Header.Get("X-WT-Signature")) {
		t.Fatalf("ping request = %v %s", r.Header, r.Body)
	}
	var env Envelope
	must(t, json.Unmarshal(r.Body, &env))
	if env.Tenant != "acme" || env.Tree != "web" || env.Actor != "alice" || env.ID != ping.ID {
		t.Fatalf("ping envelope = %+v", env)
	}

	e.advance(time.Second)
	re, err := e.svc.Redeliver(e.ctx, sc, h.ID, ping.ID)
	must(t, err)
	if re.ID == ping.ID || !re.Redelivery || !re.Succeeded || re.Event != EventPing {
		t.Fatalf("redelivery = %+v", re)
	}
	r2 := rc.requests()[1]
	var env2 Envelope
	must(t, json.Unmarshal(r2.Body, &env2))
	if env2.ID != re.ID || r2.Header.Get("X-WT-Delivery") != re.ID || env2.Tree != "web" || !env2.At.Equal(env.At) {
		t.Fatalf("redelivered envelope = %+v", env2)
	}
	ds := e.deliveries(sc, h.ID)
	if len(ds) != 2 || ds[0].ID != re.ID {
		t.Fatalf("deliveries newest first = %+v", ds)
	}
	if _, err := e.svc.Redeliver(e.ctx, sc, h.ID, "nope"); !errors.Is(err, domain.ErrNotFound) {
		t.Fatalf("redeliver missing: %v", err)
	}
}

func TestRetention(t *testing.T) {
	e := newEnv(t, func(o *Options) { o.Keep = 3 })
	rc := newReceiver(t)
	sc := e.tenantScope()
	h := e.create(e.alice, sc, CreateInput{URL: rc.url("/k"), Events: []string{"*"}})
	var last string
	for range 5 {
		e.advance(time.Second)
		e.handle(e.push(&e.web, "main"))
		last = rc.requests()[len(rc.requests())-1].Header.Get("X-WT-Delivery")
	}
	ds := e.deliveries(sc, h.ID)
	if len(ds) != 3 || ds[0].ID != last {
		t.Fatalf("kept = %d, newest %s want %s", len(ds), ds[0].ID, last)
	}
	limited, err := e.svc.Deliveries(e.ctx, sc, h.ID, 2)
	must(t, err)
	if len(limited) != 2 {
		t.Fatalf("limit = %d", len(limited))
	}
	e.advance(DefaultRetention + time.Hour)
	must(t, e.svc.Prune(e.ctx))
	if ds := e.deliveries(sc, h.ID); len(ds) != 0 {
		t.Fatalf("after retention = %d", len(ds))
	}
}

func TestTreeDeletedRemovesHooks(t *testing.T) {
	e := newEnv(t)
	rc := newReceiver(t)
	tenantHook := e.create(e.alice, e.tenantScope(), CreateInput{URL: rc.url("/tenant"), Events: []string{"tree.deleted"}})
	treeHook := e.create(e.alice, e.treeScope(&e.web), CreateInput{URL: rc.url("/tree"), Events: []string{"*"}})
	_, err := e.svc.Ping(e.ctx, e.alice, e.treeScope(&e.web), treeHook.ID)
	must(t, err)

	must(t, e.store.DeleteTree(e.ctx, e.web.ID))
	e.handle(domain.Event{Type: domain.EventTreeDeleted, TenantID: e.acme.ID, TreeID: e.web.ID, Actor: "alice", Payload: map[string]any{"name": "web"}})

	if _, err := e.svc.Get(e.ctx, e.treeScope(&e.web), treeHook.ID); !errors.Is(err, domain.ErrNotFound) {
		t.Fatalf("tree hook kept: %v", err)
	}
	if raws, _ := e.store.ListDocs(e.ctx, CollDeliveries, meta.DocPrefix(treeHook.ID)); len(raws) != 0 {
		t.Fatalf("tree hook deliveries kept: %d", len(raws))
	}
	reqs := rc.requests()
	if len(reqs) != 2 || reqs[1].Path != "/tenant" {
		t.Fatalf("requests = %v", rc.paths())
	}
	var env Envelope
	must(t, json.Unmarshal(reqs[1].Body, &env))
	if env.Tree != "web" || env.Event != domain.EventTreeDeleted {
		t.Fatalf("envelope = %+v", env)
	}
	if ds := e.deliveries(e.tenantScope(), tenantHook.ID); len(ds) != 1 || !ds[0].Succeeded {
		t.Fatalf("tenant hook deliveries = %+v", ds)
	}
}

func TestSSRFGuard(t *testing.T) {
	resolve := func(_ context.Context, host string) ([]netip.Addr, error) {
		switch host {
		case "internal.example":
			return []netip.Addr{netip.MustParseAddr("93.184.216.34"), netip.MustParseAddr("10.1.2.3")}, nil
		case "public.example":
			return []netip.Addr{netip.MustParseAddr("93.184.216.34")}, nil
		case "localhost":
			return []netip.Addr{netip.MustParseAddr("127.0.0.1")}, nil
		}
		return nil, errors.New("no such host")
	}
	e := newEnv(t, func(o *Options) { o.AllowPrivate = false; o.Resolver = resolve })
	sc := e.tenantScope()
	for _, u := range []string{
		"http://127.0.0.1:8080/x", "http://[::1]/x", "http://169.254.169.254/latest/meta-data",
		"http://10.0.0.1/", "https://192.168.1.10/", "http://172.16.0.1/", "http://[fd00::1]/", "http://[::ffff:127.0.0.1]/",
		"http://0.0.0.0/", "http://100.64.0.1/", "http://localhost:7420/", "http://internal.example/hook", "http://nowhere.example/",
	} {
		_, _, err := e.svc.Create(e.ctx, e.alice, sc, CreateInput{URL: u, Events: []string{"*"}})
		if !errors.Is(err, domain.ErrInvalid) {
			t.Errorf("Create(%s) = %v, want refusal", u, err)
		}
	}
	h := e.create(e.alice, sc, CreateInput{URL: "http://public.example/hook", Events: []string{"*"}})
	priv := "http://10.9.9.9/"
	if _, _, err := e.svc.Update(e.ctx, sc, h.ID, Patch{URL: &priv}); !errors.Is(err, domain.ErrInvalid) || !strings.Contains(err.Error(), "--webhook-allow-private") {
		t.Fatalf("update to private: %v", err)
	}

	// The dial guard refuses private addresses even when a stored hook
	// points at one (e.g. DNS changed after creation).
	rc := newReceiver(t)
	must(t, meta.UpdateDocT(e.ctx, e.store, CollHooks, HookKey(e.acme.ID, h.ID), func(cur *hookDoc) (*hookDoc, error) {
		cur.URL = rc.url("/sneaky")
		return cur, nil
	}))
	d, err := e.svc.Ping(e.ctx, e.alice, sc, h.ID)
	must(t, err)
	if d.Succeeded || !strings.Contains(d.Error, "refusing to connect") || len(rc.requests()) != 0 {
		t.Fatalf("guarded delivery = %+v (requests %d)", d, len(rc.requests()))
	}

	for ip, want := range map[string]bool{
		"8.8.8.8": false, "2001:4860:4860::8888": false, "127.0.0.2": true, "fe80::1": true, "::": true, "198.18.0.1": true,
	} {
		if got := Blocked(netip.MustParseAddr(ip)); got != want {
			t.Errorf("Blocked(%s) = %v", ip, got)
		}
	}
}

func TestStartDeliversFromBusAndResumesPending(t *testing.T) {
	e := newEnv(t, func(o *Options) { o.PollInterval = 10 * time.Millisecond })
	rc := newReceiver(t)
	sc := e.tenantScope()
	h := e.create(e.alice, sc, CreateInput{URL: rc.url("/live"), Events: []string{"branch.pushed"}})

	// A delivery stored but never attempted (server stopped) is resumed.
	_, err := e.svc.newDelivery(e.ctx, h.ID, Envelope{Event: "branch.pushed", Tenant: "acme", At: t0}, false)
	must(t, err)

	ctx, cancel := context.WithCancel(e.ctx)
	defer cancel()
	e.svc.Start(ctx)
	e.bus.Publish(e.push(&e.web, "main"))
	for range 2 {
		select {
		case <-rc.notify:
		case <-time.After(5 * time.Second):
			t.Fatalf("delivery not received; got %v", rc.paths())
		}
	}
	deadline := time.Now().Add(5 * time.Second)
	for {
		ds := e.deliveries(sc, h.ID)
		if len(ds) == 2 && ds[0].Succeeded && ds[1].Succeeded {
			break
		}
		if time.Now().After(deadline) {
			t.Fatalf("deliveries = %+v", ds)
		}
		time.Sleep(10 * time.Millisecond)
	}
}
