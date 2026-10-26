package httpapi

import (
	"context"
	"io"
	"log/slog"
	"strings"
	"testing"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/events"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
	"github.com/seanfilimon/worktree/services/server-go/internal/notify"
)

// newNotifyFixture wires a notify.Service over depsStore and a real bus
// (whose Recent ring backs GET /events/recent).
func newNotifyFixture(t *testing.T) (*fixture, *notify.Service, *events.Bus) {
	t.Helper()
	var svc *notify.Service
	bus := events.NewBus()
	f := newFixture(t, func(d *Deps) {
		ds := &depsStore{manageStore: &manageStore{fakeStore: d.Store.(*fakeStore)}, DocStore: meta.NewMemory()}
		d.Store = ds
		d.Bus = bus
		svc = notify.New(notify.Deps{Store: ds, Authz: d.Authz, Bus: bus, Logger: slog.New(slog.NewTextHandler(io.Discard, nil))})
		d.Notify = svc
	})
	return f, svc, bus
}

func TestNotificationEndpoints(t *testing.T) {
	f, svc, _ := newNotifyFixture(t)
	do := func(method, path, as string, body any, want int) []byte {
		t.Helper()
		rec := f.do(t, method, path, as, body)
		if rec.Code != want {
			t.Fatalf("%s %s as %q: %d, want %d; %s", method, path, as, rec.Code, want, rec.Body.String())
		}
		return rec.Body.Bytes()
	}
	ev := domain.Event{Type: domain.EventMRCommented, TenantID: f.acme.ID, TreeID: f.web.ID, Actor: "alice",
		Payload: domain.ReviewComment{ID: "1", TreeID: f.web.ID, MRNumber: 1, AuthorID: "acct-alice", Author: "alice", Body: "@carol please check"}}
	ns, err := svc.HandleEvent(context.Background(), ev)
	if err != nil || len(ns) != 2 {
		t.Fatalf("HandleEvent = %+v, %v", ns, err)
	}

	do("GET", "/api/v1/notifications", "", nil, 401)
	list := decode[notify.ListResult](t, f.do(t, "GET", "/api/v1/notifications", "carol", nil))
	if len(list.Notifications) != 1 || list.UnreadCount != 1 || list.Notifications[0].Reason != notify.ReasonMentioned {
		t.Fatalf("carol = %+v", list)
	}
	id := list.Notifications[0].ID
	if l := decode[notify.ListResult](t, f.do(t, "GET", "/api/v1/notifications?unread=true&limit=5", "bob", nil)); len(l.Notifications) != 1 ||
		l.Notifications[0].Reason != notify.ReasonMRCommented {
		t.Fatalf("bob = %+v", l)
	}
	do("GET", "/api/v1/notifications?unread=maybe", "carol", nil, 400)
	do("GET", "/api/v1/notifications?limit=0", "carol", nil, 400)
	if l := decode[notify.ListResult](t, f.do(t, "GET", "/api/v1/notifications", "alice", nil)); len(l.Notifications) != 0 || l.Notifications == nil {
		t.Fatalf("alice (actor) = %+v", l)
	}

	// Read, read-all, delete act on the caller's own inbox only.
	do("POST", "/api/v1/notifications/"+id+"/read", "bob", nil, 404)
	read := decode[domain.Notification](t, f.do(t, "POST", "/api/v1/notifications/"+id+"/read", "carol", nil))
	if read.ReadAt == nil {
		t.Fatalf("read = %+v", read)
	}
	if l := decode[notify.ListResult](t, f.do(t, "GET", "/api/v1/notifications?unread=true", "carol", nil)); len(l.Notifications) != 0 || l.UnreadCount != 0 {
		t.Fatalf("unread after read = %+v", l)
	}
	if r := decode[readAllResult](t, f.do(t, "POST", "/api/v1/notifications/read-all", "bob", nil)); r.Updated != 1 {
		t.Fatalf("read-all = %+v", r)
	}
	do("DELETE", "/api/v1/notifications/"+id, "bob", nil, 404)
	do("DELETE", "/api/v1/notifications/"+id, "carol", nil, 204)
	do("DELETE", "/api/v1/notifications/"+id, "carol", nil, 404)

	// Settings.
	st := decode[notify.Settings](t, f.do(t, "GET", "/api/v1/notifications/settings", "bob", nil))
	if !st.Email || st.EmailConfigured {
		t.Fatalf("settings = %+v", st)
	}
	st = decode[notify.Settings](t, f.do(t, "PUT", "/api/v1/notifications/settings", "bob", map[string]any{"email": false}))
	if st.Email {
		t.Fatalf("settings after put = %+v", st)
	}
	do("PUT", "/api/v1/notifications/settings", "bob", map[string]any{"email": "yes"}, 400)

	// Watch settings: tree (tree:read) and tenant (tenant:read).
	w := decode[domain.WatchSetting](t, f.do(t, "GET", "/api/v1/trees/acme/web/watch", "carol", nil))
	if w.Level != domain.WatchParticipating || w.TreeID != f.web.ID {
		t.Fatalf("default watch = %+v", w)
	}
	do("GET", "/api/v1/trees/acme/web/watch", "dave", nil, 403)
	do("PUT", "/api/v1/trees/acme/web/watch", "carol", map[string]any{"level": "everything"}, 400)
	w = decode[domain.WatchSetting](t, f.do(t, "PUT", "/api/v1/trees/acme/web/watch", "carol", map[string]any{"level": "watching", "email": true}))
	if w.Level != domain.WatchWatching || !w.Email || w.At.IsZero() {
		t.Fatalf("put watch = %+v", w)
	}
	w = decode[domain.WatchSetting](t, f.do(t, "PUT", "/api/v1/tenants/acme/watch", "carol", map[string]any{"level": "ignoring"}))
	if w.Level != domain.WatchIgnoring || w.TreeID != "" {
		t.Fatalf("tenant watch = %+v", w)
	}
	do("PUT", "/api/v1/tenants/acme/watch", "dave", map[string]any{"level": "watching"}, 403)
	do("GET", "/api/v1/tenants/nope/watch", "carol", nil, 404)
	watches := decode[watchList](t, f.do(t, "GET", "/api/v1/notifications/watches", "carol", nil))
	if len(watches.Watches) != 2 {
		t.Fatalf("watches = %+v", watches)
	}
	for _, v := range watches.Watches {
		if v.Tenant != "acme" || (v.TreeID != "" && v.Tree != "web") {
			t.Fatalf("watch view = %+v", v)
		}
	}

	// Service accounts have no inbox.
	f.tokens["sa-token"] = &domain.Principal{Username: "sa:acme/ci", TenantID: f.acme.ID, Kind: domain.PrincipalServiceAccount, ServiceAccountID: "sa1"}
	do("GET", "/api/v1/notifications", "sa-token", nil, 403)
}

func TestNotificationsNotImplemented(t *testing.T) {
	f := newFixture(t)
	if rec := f.do(t, "GET", "/api/v1/notifications", "alice", nil); rec.Code != 501 {
		t.Fatalf("GET /notifications without Notify = %d", rec.Code)
	}
}

// notification.created reaches only its own account, even on the admin
// firehose, and never tenant-scoped streams.
func TestNotificationEventsArePrivate(t *testing.T) {
	f, svc, _ := newNotifyFixture(t)
	ev := domain.Event{Type: domain.EventMRCommented, TenantID: f.acme.ID, TreeID: f.web.ID, Actor: "alice",
		Payload: domain.ReviewComment{ID: "1", TreeID: f.web.ID, MRNumber: 1, AuthorID: "acct-alice", Author: "alice", Body: "@carol hi"}}
	if _, err := svc.HandleEvent(context.Background(), ev); err != nil {
		t.Fatal(err)
	}
	recent := func(as, query string) []domain.Event {
		t.Helper()
		var out []domain.Event
		for _, e := range decode[[]domain.Event](t, f.do(t, "GET", "/api/v1/events/recent?types=notification.created"+query, as, nil)) {
			out = append(out, e)
		}
		return out
	}
	accountOf := func(e domain.Event) string {
		m, _ := e.Payload.(map[string]any)
		s, _ := m["account_id"].(string)
		return s
	}
	if got := recent("carol", ""); len(got) != 1 || accountOf(got[0]) != "acct-carol" {
		t.Fatalf("carol sees %+v", got)
	}
	if got := recent("bob", ""); len(got) != 1 || accountOf(got[0]) != "acct-bob" {
		t.Fatalf("bob sees %+v", got)
	}
	if got := recent("admin", ""); len(got) != 0 {
		t.Fatalf("admin sees %+v", got)
	}
	if got := recent("carol", "&tenant=acme"); len(got) != 0 {
		t.Fatalf("tenant stream sees %+v", got)
	}
	// Ordinary events still flow.
	all := decode[[]domain.Event](t, f.do(t, "GET", "/api/v1/events/recent", "admin", nil))
	for _, e := range all {
		if e.Type == domain.EventNotification {
			t.Fatalf("admin firehose leaks %+v", e)
		}
	}
	if !strings.Contains(f.do(t, "GET", "/api/v1/events/recent", "carol", nil).Body.String(), "notification.created") {
		t.Fatal("carol's unscoped recent events lack her notification")
	}
}
