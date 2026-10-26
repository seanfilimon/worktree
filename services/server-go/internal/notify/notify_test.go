package notify

import (
	"context"
	"io"
	"log/slog"
	"slices"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/authz"
	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/events"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
)

var t0 = time.Date(2026, 5, 6, 7, 8, 9, 0, time.UTC)

func must(t testing.TB, err error) {
	t.Helper()
	if err != nil {
		t.Fatal(err)
	}
}

// env: tenant acme (alice owner, bob developer, carol viewer; team "core"
// = bob + carol), tenant other (dave owner); private tree acme/web with
// merge request !1 by bob (feature -> main) whose source tip is snap-1.
type env struct {
	t     *testing.T
	ctx   context.Context
	store *meta.Memory
	bus   *events.Bus
	svc   *Service
	acme  domain.Tenant
	web   domain.Tree
	mr    domain.MergeRequest

	clockMu sync.Mutex
	clock   time.Time
}

func newEnv(t *testing.T, opts ...func(*Options)) *env {
	t.Helper()
	ctx := context.Background()
	store := meta.NewMemory()
	e := &env{t: t, ctx: ctx, store: store, bus: events.NewBus(), clock: t0}
	e.acme = domain.Tenant{ID: "t-acme", Slug: "acme", Name: "Acme", Kind: domain.TenantOrg, CreatedAt: t0}
	other := domain.Tenant{ID: "t-other", Slug: "other", Kind: domain.TenantOrg, CreatedAt: t0}
	must(t, store.CreateTenant(ctx, &e.acme))
	must(t, store.CreateTenant(ctx, &other))
	roles := map[string][2]string{
		"alice": {e.acme.ID, domain.RoleOwner},
		"bob":   {e.acme.ID, domain.RoleDeveloper},
		"carol": {e.acme.ID, domain.RoleViewer},
		"dave":  {other.ID, domain.RoleOwner},
	}
	for _, u := range []string{"alice", "bob", "carol", "dave"} {
		personal := domain.Tenant{ID: "t-" + u, Slug: u, Kind: domain.TenantUser, CreatedAt: t0}
		must(t, store.CreateTenant(ctx, &personal))
		acct := domain.Account{ID: "a-" + u, Username: u, DisplayName: strings.ToUpper(u[:1]) + u[1:], Email: u + "@x.test", TenantID: personal.ID, CreatedAt: t0}
		must(t, store.CreateAccount(ctx, &acct))
		r := roles[u]
		must(t, store.PutMembership(ctx, &domain.Membership{TenantID: r[0], AccountID: acct.ID, Role: r[1], CreatedAt: t0}))
	}
	team := domain.Team{TenantID: e.acme.ID, Slug: "core", Name: "Core", CreatedAt: t0}
	must(t, store.CreateTeam(ctx, &team))
	for _, a := range []string{"a-bob", "a-carol"} {
		must(t, store.PutTeamMember(ctx, &domain.TeamMember{TeamID: team.ID, AccountID: a, Role: domain.TeamRoleMember, CreatedAt: t0}))
	}
	e.web = domain.Tree{ID: "tree-web", TenantID: e.acme.ID, Name: "web", DefaultBranch: "main", Visibility: domain.VisibilityPrivate, CreatedAt: t0, UpdatedAt: t0}
	must(t, store.CreateTree(ctx, &e.web))
	must(t, store.PutSnapshot(ctx, &domain.Snapshot{ID: "snap-1", TreeID: e.web.ID, BranchName: "feature", Author: "bob", Message: "work", CreatedAt: t0, PushedBy: "a-bob", PushedAt: t0}))
	must(t, store.PutBranch(ctx, &domain.Branch{TreeID: e.web.ID, Name: "feature", Tip: "snap-1", CreatedAt: t0, UpdatedAt: t0}))
	e.mr = domain.MergeRequest{TreeID: e.web.ID, Title: "Add login", Description: "Please look", SourceBranch: "feature", TargetBranch: "main",
		AuthorID: "a-bob", Author: "bob", State: domain.MRStateOpen, CreatedAt: t0, UpdatedAt: t0}
	must(t, store.CreateMergeRequest(ctx, &e.mr))

	e.svc = New(Deps{Store: store, Authz: authz.New(store, authz.Options{}), Bus: e.bus,
		Logger: slog.New(slog.NewTextHandler(io.Discard, nil))})
	o := Options{Now: e.now}
	for _, f := range opts {
		f(&o)
	}
	must(t, e.svc.Configure(o))
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

func (e *env) event(typ, actor string, payload any) domain.Event {
	return domain.Event{Type: typ, TenantID: e.acme.ID, TreeID: e.web.ID, Actor: actor, Payload: payload, At: e.now()}
}

// handle runs ev and returns "account:reason" of the created notifications.
func (e *env) handle(ev domain.Event) []string {
	e.t.Helper()
	ns, err := e.svc.HandleEvent(e.ctx, ev)
	if err != nil {
		e.t.Fatalf("HandleEvent(%s): %v", ev.Type, err)
	}
	out := []string{}
	for _, n := range ns {
		out = append(out, strings.TrimPrefix(n.AccountID, "a-")+":"+n.Reason)
	}
	slices.Sort(out)
	return out
}

func want(t *testing.T, what string, got []string, exp ...string) {
	t.Helper()
	if exp == nil {
		exp = []string{}
	}
	slices.Sort(exp)
	if !slices.Equal(got, exp) {
		t.Fatalf("%s: got %v, want %v", what, got, exp)
	}
}

func (e *env) inbox(acct string) *ListResult {
	e.t.Helper()
	res, err := e.svc.List(e.ctx, acct, false, 0)
	must(e.t, err)
	return res
}

func TestMentions(t *testing.T) {
	got := Mentions("hi @Bob and @carol-, cc @bob; mail x@dave.test, path a/@eve, @@frank, (@core)\n@alice")
	if !slices.Equal(got, []string{"bob", "carol", "core", "alice"}) {
		t.Fatalf("Mentions = %v", got)
	}
}

func TestReviewRequestedExpandsTeamsAndExcludesActor(t *testing.T) {
	e := newEnv(t)
	ev := e.event(domain.EventMRReviewRequested, "bob", map[string]any{
		"number": 1, "reviewers": []string{"user:alice", "team:core", "user:nobody"}, "requested_by": "bob",
	})
	want(t, "review requested", e.handle(ev), "alice:review_requested", "carol:review_requested")
	in := e.inbox("a-alice")
	if len(in.Notifications) != 1 || in.UnreadCount != 1 {
		t.Fatalf("alice inbox = %+v", in)
	}
	n := in.Notifications[0]
	if n.URL != "/trees/acme/web/merge-requests/1" || n.Actor != "bob" || n.TenantID != e.acme.ID || n.TreeID != e.web.ID ||
		!strings.Contains(n.Title, "!1 Add login") || n.Event != domain.EventMRReviewRequested {
		t.Fatalf("notification = %+v", n)
	}
	// The same request again within a minute is deduplicated; later it is not.
	want(t, "dedup", e.handle(ev))
	e.advance(2 * time.Minute)
	want(t, "after window", e.handle(ev), "alice:review_requested", "carol:review_requested")
}

func TestMentionsRespectTreeReadAndActor(t *testing.T) {
	e := newEnv(t)
	c := domain.ReviewComment{ID: "1", TreeID: e.web.ID, MRNumber: 1, AuthorID: "a-alice", Author: "alice",
		Body: "@bob @carol @dave @alice see line 3; @core too", CreatedAt: t0}
	// bob is the author (mr_commented) but a mention is more specific;
	// dave cannot read the tree; alice is the actor.
	want(t, "comment", e.handle(e.event(domain.EventMRCommented, "alice", c)), "bob:mentioned", "carol:mentioned")
	n := e.inbox("a-bob").Notifications[0]
	if !strings.Contains(n.Title, "alice commented on !1") || !strings.Contains(n.Body, "see line 3") {
		t.Fatalf("notification = %+v", n)
	}
	// A comment without mentions notifies the merge request's author.
	e.advance(2 * time.Minute)
	c.Body = "LGTM"
	want(t, "plain comment", e.handle(e.event(domain.EventMRCommented, "alice", c)), "bob:mr_commented")
	// The author commenting on their own MR notifies nobody.
	want(t, "own comment", e.handle(e.event(domain.EventMRCommented, "bob", c)))

	// Mentions in the description of a new merge request.
	mr := e.mr
	mr.Description = "cc @carol"
	want(t, "opened", e.handle(e.event(domain.EventMROpened, "bob", mr)), "carol:mentioned")
}

func TestMergedReviewedApproved(t *testing.T) {
	e := newEnv(t)
	merged := e.mr
	merged.State = domain.MRStateMerged
	want(t, "merged", e.handle(e.event(domain.EventMRMerged, "alice", merged)), "bob:mr_merged")
	want(t, "merged by author", e.handle(e.event(domain.EventMRMerged, "bob", merged)))

	appr := e.mr
	appr.Approvals = []domain.Approval{{AccountID: "a-alice", Username: "alice", Body: "nice, @carol take a look"}}
	want(t, "approved", e.handle(e.event(domain.EventMRApproved, "alice", appr)), "bob:mr_commented", "carol:mentioned")
	e.advance(time.Second)
	reviewed := map[string]any{"merge_request": e.mr, "decision": domain.ReviewChangesRequested, "reviewer": "carol"}
	want(t, "reviewed", e.handle(e.event(domain.EventMRReviewed, "carol", reviewed)), "bob:mr_commented")
	if n := e.inbox("a-bob").Notifications[0]; !strings.Contains(n.Title, "carol requested changes on !1") {
		t.Fatalf("title = %q", n.Title)
	}
	// Closed: watchers only.
	want(t, "closed", e.handle(e.event(domain.EventMRClosed, "alice", e.mr)))
}

func TestWatchLevels(t *testing.T) {
	e := newEnv(t)
	ctx := e.ctx
	if w, err := e.svc.GetWatch(ctx, "a-carol", e.acme.ID, e.web.ID); err != nil || w.Level != domain.WatchParticipating || !w.At.IsZero() {
		t.Fatalf("default watch = %+v, %v", w, err)
	}
	if _, err := e.svc.PutWatch(ctx, "a-carol", e.acme.ID, e.web.ID, WatchPatch{Level: "loud"}); err == nil {
		t.Fatal("invalid level accepted")
	}
	_, err := e.svc.PutWatch(ctx, "a-carol", e.acme.ID, e.web.ID, WatchPatch{Level: domain.WatchWatching})
	must(t, err)
	// dave watches the whole tenant but cannot read the tree.
	_, err = e.svc.PutWatch(ctx, "a-dave", e.acme.ID, "", WatchPatch{Level: domain.WatchWatching})
	must(t, err)
	want(t, "opened (watching)", e.handle(e.event(domain.EventMROpened, "bob", e.mr)), "carol:watching")
	want(t, "closed (watching)", e.handle(e.event(domain.EventMRClosed, "bob", e.mr)), "carol:watching")

	// Tenant-level watching applies unless the tree says otherwise.
	_, err = e.svc.PutWatch(ctx, "a-alice", e.acme.ID, "", WatchPatch{Level: domain.WatchWatching})
	must(t, err)
	e.advance(2 * time.Minute)
	want(t, "tenant watch", e.handle(e.event(domain.EventMROpened, "bob", e.mr)), "alice:watching", "carol:watching")
	_, err = e.svc.PutWatch(ctx, "a-alice", e.acme.ID, e.web.ID, WatchPatch{Level: domain.WatchParticipating})
	must(t, err)
	e.advance(2 * time.Minute)
	want(t, "tree override", e.handle(e.event(domain.EventMROpened, "bob", e.mr)), "carol:watching")

	// Ignoring silences even participating reasons.
	_, err = e.svc.PutWatch(ctx, "a-carol", e.acme.ID, e.web.ID, WatchPatch{Level: domain.WatchIgnoring})
	must(t, err)
	c := domain.ReviewComment{ID: "2", TreeID: e.web.ID, MRNumber: 1, AuthorID: "a-alice", Author: "alice", Body: "@carol?"}
	want(t, "ignoring", e.handle(e.event(domain.EventMRCommented, "alice", c)), "bob:mr_commented")

	ws, err := e.svc.Watches(ctx, "a-alice")
	must(t, err)
	if len(ws) != 2 {
		t.Fatalf("alice watches = %+v", ws)
	}
	// tree.deleted drops the tree's watch settings.
	_, err = e.svc.HandleEvent(ctx, domain.Event{Type: domain.EventTreeDeleted, TenantID: e.acme.ID, TreeID: e.web.ID})
	must(t, err)
	if ws, _ := e.svc.Watches(ctx, "a-alice"); len(ws) != 1 || ws[0].TreeID != "" {
		t.Fatalf("after tree.deleted = %+v", ws)
	}
	if ws, _ := e.svc.Watches(ctx, "a-carol"); len(ws) != 0 {
		t.Fatalf("carol after tree.deleted = %+v", ws)
	}
}

func TestCIFailureTodoAndRelease(t *testing.T) {
	e := newEnv(t)
	ctx := e.ctx
	ok := domain.CIStatus{TreeID: e.web.ID, SnapshotID: "snap-1", Context: "ci/build", State: domain.CISuccess, Reporter: "sa:acme/ci"}
	want(t, "success", e.handle(e.event(domain.EventCIStatus, "sa:acme/ci", ok)))
	fail := ok
	fail.State = domain.CIFailure
	want(t, "failure", e.handle(e.event(domain.EventCIStatus, "sa:acme/ci", fail)), "bob:ci_failed")
	if n := e.inbox("a-bob").Notifications[0]; n.URL != "/trees/acme/web/merge-requests/1" || !strings.Contains(n.Title, "ci/build") {
		t.Fatalf("ci notification = %+v", n)
	}

	todo := domain.Todo{ID: "x", TenantID: e.acme.ID, TreeID: e.web.ID, Number: 4, Title: "Bump lib", Status: domain.TodoOpen}
	want(t, "unassigned", e.handle(e.event(domain.EventTodoCreated, "alice", todo)))
	todo.AssigneeID, todo.Assignee = "a-carol", "carol"
	want(t, "assigned", e.handle(e.event(domain.EventTodoUpdated, "alice", todo)), "carol:todo_assigned")
	e.advance(5 * time.Minute)
	todo.Status = domain.TodoInProgress
	want(t, "same assignee", e.handle(e.event(domain.EventTodoUpdated, "alice", todo)))
	todo.AssigneeID, todo.Assignee = "a-alice", "alice"
	want(t, "claimed by self", e.handle(e.event(domain.EventTodoUpdated, "alice", todo)))

	_, err := e.svc.PutWatch(ctx, "a-carol", e.acme.ID, "", WatchPatch{Level: domain.WatchWatching})
	must(t, err)
	rel := domain.Release{ID: "r1", TreeID: e.web.ID, Tag: "v1.0.0", Name: "One", Draft: true}
	want(t, "draft", e.handle(e.event(domain.EventReleaseCreated, "alice", rel)))
	rel.Draft = false
	want(t, "published", e.handle(e.event(EventReleaseUpdated, "alice", rel)), "carol:release")
	e.advance(5 * time.Minute)
	want(t, "updated again", e.handle(e.event(EventReleaseUpdated, "alice", rel)))
	// Watchers also hear about CI failures.
	e.advance(5 * time.Minute)
	want(t, "ci watching", e.handle(e.event(domain.EventCIStatus, "sa:acme/ci", fail)), "bob:ci_failed", "carol:watching")
}

func TestInboxReadDeleteRetentionAndEvents(t *testing.T) {
	e := newEnv(t, func(o *Options) { o.Keep = 3 })
	ctx := e.ctx
	created, cancel := e.bus.Subscribe(domain.EventFilter{Types: []string{domain.EventNotification}})
	defer cancel()
	for i := 1; i <= 5; i++ {
		c := domain.ReviewComment{ID: "c", TreeID: e.web.ID, MRNumber: 1, AuthorID: "a-alice", Author: "alice", Body: "@carol #" + string(rune('0'+i))}
		e.advance(2 * time.Minute)
		want(t, "comment", e.handle(e.event(domain.EventMRCommented, "alice", c)), "bob:mr_commented", "carol:mentioned")
	}
	in := e.inbox("a-carol")
	if len(in.Notifications) != 3 || in.UnreadCount != 3 || !strings.HasSuffix(in.Notifications[0].Body, "#5") {
		t.Fatalf("kept = %+v", in)
	}
	select {
	case ev := <-created:
		n, ok := ev.Payload.(domain.Notification)
		if !ok || ev.TenantID != "" || ev.TreeID != "" || n.AccountID == "" || ev.Actor != "alice" {
			t.Fatalf("notification.created = %+v", ev)
		}
	case <-time.After(time.Second):
		t.Fatal("no notification.created event")
	}

	first := in.Notifications[0]
	read, err := e.svc.MarkRead(ctx, "a-carol", first.ID)
	must(t, err)
	if read.ReadAt == nil {
		t.Fatal("not read")
	}
	if _, err := e.svc.MarkRead(ctx, "a-bob", first.ID); err == nil {
		t.Fatal("bob read carol's notification")
	}
	if res, _ := e.svc.List(ctx, "a-carol", true, 10); len(res.Notifications) != 2 || res.UnreadCount != 2 {
		t.Fatalf("unread = %+v", res)
	}
	n, err := e.svc.MarkAllRead(ctx, "a-carol")
	if err != nil || n != 2 {
		t.Fatalf("MarkAllRead = %d, %v", n, err)
	}
	if res := e.inbox("a-carol"); res.UnreadCount != 0 || len(res.Notifications) != 3 {
		t.Fatalf("after read-all = %+v", res)
	}
	must(t, e.svc.Delete(ctx, "a-carol", first.ID))
	if err := e.svc.Delete(ctx, "a-carol", first.ID); err == nil {
		t.Fatal("deleted twice")
	}
	if res := e.inbox("a-carol"); len(res.Notifications) != 2 {
		t.Fatalf("after delete = %+v", res)
	}
	// Age-based retention.
	e.advance(DefaultRetention + time.Hour)
	if res := e.inbox("a-carol"); len(res.Notifications) != 0 {
		t.Fatalf("expired still listed: %+v", res)
	}
	must(t, e.svc.Prune(ctx))
	if all, _ := e.svc.listOf(ctx, "a-carol"); len(all) != 0 {
		t.Fatalf("prune kept %d", len(all))
	}
	if _, err := e.svc.List(ctx, "", false, 0); err == nil {
		t.Fatal("service account inbox")
	}
}

func TestSettingsDefaults(t *testing.T) {
	e := newEnv(t)
	st, err := e.svc.GetSettings(e.ctx, "a-bob")
	must(t, err)
	if !st.Email || st.EmailConfigured || st.UpdatedAt != nil {
		t.Fatalf("defaults = %+v", st)
	}
	off := false
	st, err = e.svc.PutSettings(e.ctx, "a-bob", SettingsPatch{Email: &off})
	must(t, err)
	if st.Email || st.UpdatedAt == nil {
		t.Fatalf("after put = %+v", st)
	}
}

func TestStartDeliversFromBus(t *testing.T) {
	e := newEnv(t)
	ctx, cancel := context.WithCancel(e.ctx)
	defer cancel()
	e.svc.Start(ctx)
	merged := e.mr
	e.bus.Publish(e.event(domain.EventMRMerged, "alice", merged))
	deadline := time.Now().Add(5 * time.Second)
	for time.Now().Before(deadline) {
		if res := e.inbox("a-bob"); len(res.Notifications) == 1 {
			return
		}
		time.Sleep(10 * time.Millisecond)
	}
	t.Fatal("no notification created from the bus")
}
