package todo

import (
	"context"
	"errors"
	"fmt"
	"io"
	"log/slog"
	"reflect"
	"slices"
	"sync"
	"testing"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/authz"
	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/events"
	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
)

// ---- fixture --------------------------------------------------------------------

type fakeAudit struct {
	mu  sync.Mutex
	evs []domain.AuditEvent
}

func (a *fakeAudit) Record(_ context.Context, e domain.AuditEvent) {
	a.mu.Lock()
	defer a.mu.Unlock()
	a.evs = append(a.evs, e)
}

func (a *fakeAudit) Query(context.Context, domain.AuditFilter) ([]domain.AuditEvent, error) {
	return nil, nil
}

func (a *fakeAudit) of(action string) []domain.AuditEvent {
	a.mu.Lock()
	defer a.mu.Unlock()
	var out []domain.AuditEvent
	for _, e := range a.evs {
		if e.Action == action {
			out = append(out, e)
		}
	}
	return out
}

// env: tenants acme (alice owner, bob developer, carol viewer) and other
// (dave owner); trees acme/web, acme/api, other/lib (all private).
type env struct {
	t                      *testing.T
	ctx                    context.Context
	store                  *meta.Memory
	bus                    *events.Bus
	audit                  *fakeAudit
	svc                    *Service
	acme, other            domain.Tenant
	web, api, lib          domain.Tree
	alice, bob, carol, dve *domain.Principal
	clock                  time.Time
}

func must(t testing.TB, err error) {
	t.Helper()
	if err != nil {
		t.Fatal(err)
	}
}

var t0 = time.Date(2026, 3, 4, 5, 6, 7, 0, time.UTC)

func newEnv(t *testing.T) *env {
	t.Helper()
	ctx := context.Background()
	store := meta.NewMemory()
	e := &env{t: t, ctx: ctx, store: store, bus: events.NewBus(), audit: &fakeAudit{}, clock: t0}
	e.acme = domain.Tenant{ID: "t-acme", Slug: "acme", Name: "Acme", Kind: domain.TenantOrg, CreatedAt: t0}
	e.other = domain.Tenant{ID: "t-other", Slug: "other", Name: "Other", Kind: domain.TenantOrg, CreatedAt: t0}
	must(t, store.CreateTenant(ctx, &e.acme))
	must(t, store.CreateTenant(ctx, &e.other))
	users := map[string]**domain.Principal{"alice": &e.alice, "bob": &e.bob, "carol": &e.carol, "dave": &e.dve}
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
		*users[u] = &domain.Principal{AccountID: acct.ID, Username: u, TenantID: personal.ID}
	}
	mk := func(ten domain.Tenant, name string) domain.Tree {
		tr := domain.Tree{ID: "tree-" + name, TenantID: ten.ID, Name: name, DefaultBranch: "main", Visibility: domain.VisibilityPrivate, CreatedAt: t0, UpdatedAt: t0}
		must(t, store.CreateTree(ctx, &tr))
		return tr
	}
	e.web, e.api, e.lib = mk(e.acme, "web"), mk(e.acme, "api"), mk(e.other, "lib")
	e.svc = New(Deps{
		Store:   store,
		History: meta.NewHistory(store),
		Authz:   authz.New(store, authz.Options{}),
		Bus:     e.bus,
		Audit:   e.audit,
		Logger:  slog.New(slog.NewTextHandler(io.Discard, nil)),
	})
	e.svc.now = func() time.Time { return e.clock }
	return e
}

var snapSeq int

// snap stores a snapshot of tree on top of parent ("" = root) pushed by p.
func (e *env) snap(tree domain.Tree, parent, msg string, p *domain.Principal, deps ...domain.Dependency) domain.Snapshot {
	e.t.Helper()
	snapSeq++
	s := domain.Snapshot{
		ID:           fmt.Sprintf("snap-%04d", snapSeq),
		TreeID:       tree.ID,
		BranchName:   "main",
		ManifestHash: hash.Sum([]byte(msg)),
		Author:       p.Username,
		Message:      msg,
		CreatedAt:    t0.Add(time.Duration(snapSeq) * time.Minute),
		Dependencies: deps,
		PushedBy:     p.AccountID,
		PushedAt:     t0,
	}
	if parent != "" {
		s.Parents = []string{parent}
	}
	must(e.t, e.store.PutSnapshot(e.ctx, &s))
	return s
}

// push moves branch of tree from old to tip and processes branch.pushed.
func (e *env) push(tree domain.Tree, branch, old, tip string, actor string) {
	e.t.Helper()
	must(e.t, e.store.PutBranch(e.ctx, &domain.Branch{TreeID: tree.ID, Name: branch, Tip: tip, CreatedAt: t0, UpdatedAt: t0}))
	must(e.t, e.svc.HandleEvent(e.ctx, pushedEvent(tree, branch, old, tip, actor)))
}

func pushedEvent(tree domain.Tree, branch, old, tip, actor string) domain.Event {
	return domain.Event{
		Type: domain.EventBranchPushed, TenantID: tree.TenantID, TreeID: tree.ID, Actor: actor,
		Payload: map[string]any{"branch": branch, "old_tip": old, "new_tip": tip, "count": 1},
	}
}

func (e *env) list(tree domain.Tree) []domain.Todo {
	e.t.Helper()
	l, err := e.svc.List(e.ctx, tree.ID, Filter{})
	must(e.t, err)
	return l
}

func drain(ch <-chan domain.Event) []domain.Event {
	var out []domain.Event
	for {
		select {
		case ev := <-ch:
			out = append(out, ev)
		default:
			return out
		}
	}
}

// ---- tests ------------------------------------------------------------------------

func TestPushCreatesTodosFromDependencies(t *testing.T) {
	e := newEnv(t)
	created, cancel := e.bus.Subscribe(domain.EventFilter{Types: []string{domain.EventTodoCreated}})
	defer cancel()

	s1 := e.snap(e.web, "", "Use the new API\n\nlong body", e.bob,
		domain.Dependency{Tree: "api", Branch: "develop", Title: "Add /v2/users", Details: "needs paging", Priority: "high", Blocking: true, Linked: true},
		domain.Dependency{Tree: "api"},                                     // title from the message
		domain.Dependency{TenantSlug: "other", Tree: "lib", Title: "Bump"}, // bob cannot read other/lib
		domain.Dependency{Tree: "missing", Title: "Nope"},
		domain.Dependency{TenantSlug: "nobody", Tree: "x", Title: "Nope"},
		domain.Dependency{Tree: "api", Priority: "bogus", Title: "Odd priority"},
	)
	e.push(e.web, "main", "", s1.ID, "bob")

	todos := e.list(e.api)
	if len(todos) != 3 {
		t.Fatalf("todos = %+v", todos)
	}
	first := todos[0]
	want := domain.Todo{
		ID: first.ID, TenantID: e.acme.ID, TreeID: e.api.ID, Number: 1, TargetBranch: "develop",
		Title: "Add /v2/users", Details: "needs paging", Priority: domain.TodoPriorityHigh, Blocking: true, Linked: true,
		Status: domain.TodoOpen, SourceTenantID: e.acme.ID, SourceTreeID: e.web.ID, SourceBranch: "main",
		SourceSnapshotID: s1.ID, CreatedBy: e.bob.AccountID, Author: "bob", CreatedAt: t0, UpdatedAt: t0,
	}
	if !reflect.DeepEqual(first, want) {
		t.Fatalf("todo #1 =\n%+v\nwant\n%+v", first, want)
	}
	if todos[1].Number != 2 || todos[1].Title != "Use the new API" || todos[1].TargetBranch != "main" || todos[1].Priority != domain.TodoPriorityMedium {
		t.Fatalf("todo #2 = %+v", todos[1])
	}
	if todos[2].Title != "Odd priority" || todos[2].Priority != domain.TodoPriorityMedium {
		t.Fatalf("todo #3 = %+v", todos[2])
	}
	if l := e.list(e.lib); len(l) != 0 {
		t.Fatalf("other/lib got todos without access: %+v", l)
	}

	evs := drain(created)
	if len(evs) != 3 || evs[0].TreeID != e.api.ID || evs[0].TenantID != e.acme.ID || evs[0].Actor != "bob" {
		t.Fatalf("todo.created events = %+v", evs)
	}
	if p, ok := evs[0].Payload.(*domain.Todo); !ok || p.Number != 1 {
		t.Fatalf("payload = %#v", evs[0].Payload)
	}
	if n := len(e.audit.of("todo.create")); n != 3 {
		t.Fatalf("todo.create audits = %d", n)
	}
	skipped := e.audit.of("todo.skipped")
	if len(skipped) != 3 {
		t.Fatalf("todo.skipped = %+v", skipped)
	}
	reasons := map[string]string{}
	for _, s := range skipped {
		reasons[s.Details["target"]] = s.Details["reason"]
		if s.TenantID != e.acme.ID || s.ActorID != e.bob.AccountID || s.ResourceID != s1.ID {
			t.Fatalf("skipped audit = %+v", s)
		}
	}
	if reasons["other/lib"] != "pusher may not read the target tree" || reasons["missing"] != "target tree not found" || reasons["nobody/x"] != "target tree not found" {
		t.Fatalf("reasons = %v", reasons)
	}

	// The same push again, and the same history on a new branch, are
	// deduplicated.
	must(t, e.svc.HandleEvent(e.ctx, pushedEvent(e.web, "main", "", s1.ID, "bob")))
	e.push(e.web, "feature", "", s1.ID, "bob")
	if n := len(e.list(e.api)); n != 3 {
		t.Fatalf("after re-processing: %d todos", n)
	}
	if evs := drain(created); len(evs) != 0 {
		t.Fatalf("duplicate todo.created: %+v", evs)
	}

	// Only snapshots new to the branch are processed: a child pushed on
	// top creates its own TODO, numbers continue.
	s2 := e.snap(e.web, s1.ID, "More", e.alice, domain.Dependency{TenantSlug: "acme", Tree: "api", Title: "Second"},
		domain.Dependency{TenantSlug: "other", Tree: "lib", Title: "Still no"})
	e.push(e.web, "main", s1.ID, s2.ID, "alice")
	todos = e.list(e.api)
	if len(todos) != 4 || todos[3].Number != 4 || todos[3].Title != "Second" || todos[3].SourceSnapshotID != s2.ID || todos[3].Author != "alice" {
		t.Fatalf("todos = %+v", todos)
	}
	// dave owns other/lib: his dependency works across tenants.
	s3 := e.snap(e.lib, "", "lib change", e.dve, domain.Dependency{TenantSlug: "acme", Tree: "web", Title: "x"})
	e.push(e.lib, "main", "", s3.ID, "dave")
	if l := e.list(e.web); len(l) != 0 {
		t.Fatalf("dave cannot read acme/web, got %+v", l)
	}
	s4 := e.snap(e.web, s2.ID, "to lib", e.alice, domain.Dependency{TenantSlug: "other", Tree: "lib", Title: "Rebuild"})
	must(t, e.store.PutMembership(e.ctx, &domain.Membership{TenantID: e.other.ID, AccountID: e.alice.AccountID, Role: domain.RoleViewer}))
	e.push(e.web, "main", s2.ID, s4.ID, "alice")
	lib := e.list(e.lib)
	if len(lib) != 1 || lib[0].TenantID != e.other.ID || lib[0].SourceTenantID != e.acme.ID || lib[0].Number != 1 {
		t.Fatalf("lib todos = %+v", lib)
	}
}

func TestPushCompletesTodos(t *testing.T) {
	e := newEnv(t)
	base := e.snap(e.web, "", "deps", e.alice,
		domain.Dependency{Tree: "api", Title: "one"},
		domain.Dependency{Tree: "api", Title: "two"},
		domain.Dependency{Tree: "api", Title: "three"},
		domain.Dependency{Tree: "api", Title: "four"},
	)
	e.push(e.web, "main", "", base.ID, "alice")
	if n := len(e.list(e.api)); n != 4 {
		t.Fatalf("setup: %d todos", n)
	}
	updated, cancel := e.bus.Subscribe(domain.EventFilter{Types: []string{domain.EventTodoUpdated}})
	defer cancel()

	e.clock = t0.Add(time.Hour)
	c1 := e.snap(e.api, "", "Implement one\n\nCompletes: #1, #2\ncompletes: #99 garbage #x", e.bob)
	e.push(e.api, "main", "", c1.ID, "bob")
	got := e.list(e.api)
	for _, i := range []int{0, 1} {
		if got[i].Status != domain.TodoDone || got[i].ResolvedBy != c1.ID || got[i].ClosedAt == nil || !got[i].ClosedAt.Equal(e.clock) || !got[i].UpdatedAt.Equal(e.clock) {
			t.Fatalf("todo %d = %+v", i+1, got[i])
		}
	}
	if got[2].Status != domain.TodoOpen {
		t.Fatalf("todo 3 = %+v", got[2])
	}
	if evs := drain(updated); len(evs) != 2 {
		t.Fatalf("todo.updated events = %+v", evs)
	}
	if au := e.audit.of("todo.complete"); len(au) != 2 || au[0].Details["resolved_by"] != c1.ID || au[0].Details["via"] != "push" {
		t.Fatalf("todo.complete audit = %+v", au)
	}

	// Already-closed TODOs are left alone.
	c2 := e.snap(e.api, c1.ID, "again\nCompletes: #1", e.bob)
	e.push(e.api, "main", c1.ID, c2.ID, "bob")
	if got := e.list(e.api); got[0].ResolvedBy != c1.ID {
		t.Fatalf("re-completed: %+v", got[0])
	}

	// Cross-tree references need tree:write on the referenced tree.
	carolSnap := e.snap(e.web, base.ID, "Completes: api#3", e.carol)
	e.push(e.web, "main", base.ID, carolSnap.ID, "carol")
	if got := e.list(e.api); got[2].Status != domain.TodoOpen {
		t.Fatalf("viewer completed a TODO: %+v", got[2])
	}
	x := e.snap(e.web, carolSnap.ID, "Completes: api#3 acme/api#4 other/lib#1 nosuch#1", e.alice)
	e.push(e.web, "main", carolSnap.ID, x.ID, "alice")
	got = e.list(e.api)
	if got[2].Status != domain.TodoDone || got[2].ResolvedBy != x.ID || got[3].Status != domain.TodoDone {
		t.Fatalf("cross-tree completion: %+v", got[2:])
	}
}

func TestStartProcessesBusEvents(t *testing.T) {
	e := newEnv(t)
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	e.svc.Start(ctx)

	s := e.snap(e.web, "", "async", e.alice, domain.Dependency{Tree: "api", Title: "via bus"})
	must(t, e.store.PutBranch(e.ctx, &domain.Branch{TreeID: e.web.ID, Name: "main", Tip: s.ID}))
	e.bus.Publish(pushedEvent(e.web, "main", "", s.ID, "alice"))
	waitFor(t, func() bool { return len(e.list(e.api)) == 1 })

	e.bus.Publish(domain.Event{Type: domain.EventTreeDeleted, TenantID: e.acme.ID, TreeID: e.api.ID, Payload: map[string]any{"name": "api"}})
	waitFor(t, func() bool { return len(e.list(e.api)) == 0 })
	if raws, _ := e.store.ListDocs(e.ctx, CollSources, ""); len(raws) != 0 {
		t.Fatalf("index not cleaned: %d", len(raws))
	}
	cancel()
}

func waitFor(t *testing.T, cond func() bool) {
	t.Helper()
	deadline := time.Now().Add(5 * time.Second)
	for !cond() {
		if time.Now().After(deadline) {
			t.Fatal("condition not reached")
		}
		time.Sleep(5 * time.Millisecond)
	}
}

func TestDeleteTreeKeepsOtherTrees(t *testing.T) {
	e := newEnv(t)
	s := e.snap(e.web, "", "x", e.alice, domain.Dependency{Tree: "api", Title: "a"})
	e.push(e.web, "main", "", s.ID, "alice")
	_, err := e.svc.Create(e.ctx, e.alice, &e.web, CreateInput{Title: "manual"})
	must(t, err)
	must(t, e.svc.HandleEvent(e.ctx, domain.Event{Type: domain.EventTreeDeleted, TreeID: e.api.ID}))
	if len(e.list(e.api)) != 0 || len(e.list(e.web)) != 1 {
		t.Fatalf("api=%v web=%v", e.list(e.api), e.list(e.web))
	}
	// Ignored events.
	must(t, e.svc.HandleEvent(e.ctx, domain.Event{Type: domain.EventTreeDeleted}))
	must(t, e.svc.HandleEvent(e.ctx, domain.Event{Type: domain.EventMROpened, TreeID: e.web.ID}))
	must(t, e.svc.HandleEvent(e.ctx, domain.Event{Type: domain.EventBranchPushed, TreeID: "gone", Payload: map[string]any{"new_tip": "x"}}))
	must(t, e.svc.HandleEvent(e.ctx, domain.Event{Type: domain.EventBranchPushed, TreeID: e.web.ID, Payload: map[string]any{}}))
}

func TestServiceAccountPusher(t *testing.T) {
	e := newEnv(t)
	sa := &domain.ServiceAccount{TenantID: e.acme.ID, Name: "ci", Role: domain.RoleDeveloper, CreatedAt: t0}
	must(t, e.store.CreateServiceAccount(e.ctx, sa))
	s := e.snap(e.web, "", "bot", &domain.Principal{Username: "sa:acme/ci"}, domain.Dependency{Tree: "api", Title: "from ci"})
	e.push(e.web, "main", "", s.ID, "sa:acme/ci")
	l := e.list(e.api)
	if len(l) != 1 || l[0].Author != "sa:acme/ci" || l[0].CreatedBy != "" {
		t.Fatalf("todos = %+v", l)
	}
	// An unknown actor is anonymous: private targets are skipped.
	s2 := e.snap(e.web, s.ID, "ghost", &domain.Principal{Username: "ghost"}, domain.Dependency{Tree: "api", Title: "ghost"})
	e.push(e.web, "main", s.ID, s2.ID, "ghost")
	if len(e.list(e.api)) != 1 {
		t.Fatal("anonymous pusher created a TODO in a private tree")
	}
}

func TestManualLifecycle(t *testing.T) {
	e := newEnv(t)
	ctx := e.ctx
	_, err := e.svc.Create(ctx, e.bob, &e.api, CreateInput{Title: "  "})
	if !errors.Is(err, domain.ErrInvalid) {
		t.Fatalf("empty title: %v", err)
	}
	_, err = e.svc.Create(ctx, e.bob, &e.api, CreateInput{Title: "x", Priority: "urgent"})
	if !errors.Is(err, domain.ErrInvalid) {
		t.Fatalf("bad priority: %v", err)
	}
	t1, err := e.svc.Create(ctx, e.bob, &e.api, CreateInput{Title: " Write docs ", Details: "d", Priority: "LOW", Blocking: true})
	must(t, err)
	if t1.Number != 1 || t1.Title != "Write docs" || t1.Priority != domain.TodoPriorityLow || t1.TargetBranch != "main" ||
		t1.SourceTreeID != e.api.ID || t1.SourceSnapshotID != "" || t1.Author != "bob" || t1.Status != domain.TodoOpen {
		t.Fatalf("created = %+v", t1)
	}
	t2, err := e.svc.Create(ctx, e.alice, &e.api, CreateInput{Title: "Other", TargetBranch: "dev"})
	must(t, err)

	got, err := e.svc.Claim(ctx, e.carol, &e.api, 1)
	must(t, err)
	if got.Status != domain.TodoInProgress || got.Assignee != "carol" || got.AssigneeID != e.carol.AccountID {
		t.Fatalf("claimed = %+v", got)
	}
	if _, err := e.svc.Claim(ctx, nil, &e.api, 1); !errors.Is(err, domain.ErrUnauthenticated) {
		t.Fatalf("anonymous claim: %v", err)
	}

	// Filters.
	cases := []struct {
		f    Filter
		want []int
	}{
		{Filter{}, []int{1, 2}},
		{Filter{Statuses: []string{domain.TodoOpen}}, []int{2}},
		{Filter{Statuses: []string{domain.TodoOpen, domain.TodoInProgress}}, []int{1, 2}},
		{Filter{Assignee: "carol"}, []int{1}},
		{Filter{Branch: "dev"}, []int{2}},
		{Filter{Branch: "dev", Assignee: "carol"}, nil},
	}
	for _, c := range cases {
		l, err := e.svc.List(ctx, e.api.ID, c.f)
		must(t, err)
		var nums []int
		for _, x := range l {
			nums = append(nums, x.Number)
		}
		if !slices.Equal(nums, c.want) {
			t.Fatalf("filter %+v = %v, want %v", c.f, nums, c.want)
		}
	}

	e.clock = t0.Add(time.Hour)
	done, err := e.svc.Complete(ctx, e.carol, &e.api, 1, "snap-x")
	must(t, err)
	if done.Status != domain.TodoDone || done.ResolvedBy != "snap-x" || done.ClosedAt == nil || !done.UpdatedAt.Equal(e.clock) {
		t.Fatalf("completed = %+v", done)
	}
	if _, err := e.svc.Claim(ctx, e.bob, &e.api, 1); !errors.Is(err, domain.ErrConflict) {
		t.Fatalf("claim done: %v", err)
	}
	if _, err := e.svc.Cancel(ctx, e.bob, &e.api, 1); !errors.Is(err, domain.ErrConflict) {
		t.Fatalf("cancel done: %v", err)
	}
	before := len(e.audit.of("todo.complete"))
	if again, err := e.svc.Complete(ctx, e.bob, &e.api, 1, ""); err != nil || again.ResolvedBy != "snap-x" {
		t.Fatalf("idempotent complete: %+v %v", again, err)
	}
	if len(e.audit.of("todo.complete")) != before {
		t.Fatal("no-op completion was audited")
	}

	c, err := e.svc.Cancel(ctx, e.bob, &e.api, t2.Number)
	must(t, err)
	if c.Status != domain.TodoCancelled || c.ClosedAt == nil {
		t.Fatalf("cancelled = %+v", c)
	}

	// Patch: reopen clears the resolution.
	open, title, prio, blocking := domain.TodoOpen, "Renamed", "critical", false
	p, err := e.svc.Update(ctx, e.bob, &e.api, 1, Patch{Status: &open, Title: &title, Priority: &prio, Blocking: &blocking})
	must(t, err)
	if p.Status != domain.TodoOpen || p.ResolvedBy != "" || p.ClosedAt != nil || p.Title != "Renamed" || p.Priority != "critical" || p.Blocking {
		t.Fatalf("patched = %+v", p)
	}
	bad := "nope"
	if _, err := e.svc.Update(ctx, e.bob, &e.api, 1, Patch{Status: &bad}); !errors.Is(err, domain.ErrInvalid) {
		t.Fatalf("bad status: %v", err)
	}
	empty := ""
	if _, err := e.svc.Update(ctx, e.bob, &e.api, 1, Patch{Title: &empty}); !errors.Is(err, domain.ErrInvalid) {
		t.Fatalf("empty title: %v", err)
	}
	if _, err := e.svc.Update(ctx, e.bob, &e.api, 42, Patch{}); !errors.Is(err, domain.ErrNotFound) {
		t.Fatalf("missing: %v", err)
	}
	if _, err := e.svc.Get(ctx, e.api.ID, 42); !errors.Is(err, domain.ErrNotFound) || err.Error() != "todo #42 not found" {
		t.Fatalf("get missing: %v", err)
	}
	for _, a := range []string{"todo.create", "todo.claim", "todo.complete", "todo.cancel", "todo.update"} {
		if len(e.audit.of(a)) == 0 {
			t.Fatalf("no %s audit", a)
		}
	}
	if au := e.audit.of("todo.update")[0]; au.Details["status"] != "open" || au.Details["title"] != "Renamed" || au.ResourceID != e.api.ID+"#1" {
		t.Fatalf("update audit = %+v", au)
	}
}

func TestOpenBlocking(t *testing.T) {
	e := newEnv(t)
	s := e.snap(e.web, "", "x", e.alice,
		domain.Dependency{Tree: "api", Title: "blocks", Blocking: true},
		domain.Dependency{Tree: "api", Title: "soft"},
	)
	e.push(e.web, "feature", "", s.ID, "alice")
	_, err := e.svc.Create(e.ctx, e.alice, &e.web, CreateInput{Title: "manual", Blocking: true})
	must(t, err)
	l, err := e.svc.OpenBlocking(e.ctx, e.web.ID, "feature")
	must(t, err)
	if len(l) != 1 || l[0].Title != "blocks" {
		t.Fatalf("blocking = %+v", l)
	}
	if l, _ := e.svc.OpenBlocking(e.ctx, e.web.ID, "main"); len(l) != 0 {
		t.Fatalf("main = %+v", l)
	}
	_, err = e.svc.Complete(e.ctx, e.alice, &e.api, 1, "")
	must(t, err)
	if l, _ := e.svc.OpenBlocking(e.ctx, e.web.ID, ""); len(l) != 0 {
		t.Fatalf("after completion = %+v", l)
	}
}

func TestDependencyReport(t *testing.T) {
	e := newEnv(t)
	must(t, e.store.PutMembership(e.ctx, &domain.Membership{TenantID: e.other.ID, AccountID: e.alice.AccountID, Role: domain.RoleViewer}))
	s1 := e.snap(e.web, "", "first", e.alice, domain.Dependency{Tree: "api", Title: "api work"})
	s2 := e.snap(e.web, s1.ID, "second", e.alice,
		domain.Dependency{TenantSlug: "other", Tree: "lib", Title: "lib work"},
		domain.Dependency{Tree: "gone", Title: "nowhere"},
	)
	e.push(e.web, "main", "", s2.ID, "alice")
	// lib -> web (incoming for web).
	s3 := e.snap(e.lib, "", "lib needs web", e.dve)
	_ = s3
	must(t, e.store.PutMembership(e.ctx, &domain.Membership{TenantID: e.acme.ID, AccountID: e.dve.AccountID, Role: domain.RoleViewer}))
	s4 := e.snap(e.lib, "", "lib needs web", e.dve, domain.Dependency{TenantSlug: "acme", Tree: "web", Title: "web work", Branch: "next"})
	e.push(e.lib, "main", "", s4.ID, "dave")
	_, err := e.svc.Create(e.ctx, e.alice, &e.web, CreateInput{Title: "manual"}) // not incoming
	must(t, err)

	rep, err := e.svc.Dependencies(e.ctx, e.alice, &e.web, "")
	must(t, err)
	if rep.Branch != "main" || len(rep.Outgoing) != 3 {
		t.Fatalf("report = %+v", rep)
	}
	// Newest snapshot first.
	if o := rep.Outgoing[0]; o.SnapshotID != s2.ID || o.Todo == nil || *o.Todo != (TodoRef{Tenant: "other", Tree: "lib", Number: 1, Status: "open", Title: "lib work"}) {
		t.Fatalf("outgoing[0] = %+v %+v", o, o.Todo)
	}
	if o := rep.Outgoing[1]; o.Todo != nil || o.Dependency.Tree != "gone" {
		t.Fatalf("outgoing[1] = %+v", o)
	}
	if o := rep.Outgoing[2]; o.SnapshotID != s1.ID || o.Todo == nil || o.Todo.Tree != "api" || o.Todo.Number != 1 {
		t.Fatalf("outgoing[2] = %+v", o)
	}
	if len(rep.Incoming) != 1 || rep.Incoming[0].Title != "web work" || rep.Incoming[0].TargetBranch != "next" {
		t.Fatalf("incoming = %+v", rep.Incoming)
	}
	wantTrees := map[string]string{e.web.ID: "acme/web", e.api.ID: "acme/api", e.lib.ID: "other/lib"}
	if !reflect.DeepEqual(rep.Trees, wantTrees) {
		t.Fatalf("trees = %v", rep.Trees)
	}

	// bob cannot read other/lib: no TODO details, no name.
	rep, err = e.svc.Dependencies(e.ctx, e.bob, &e.web, "")
	must(t, err)
	if rep.Outgoing[0].Todo != nil || rep.Trees[e.lib.ID] != "" {
		t.Fatalf("leaked to bob: %+v %v", rep.Outgoing[0].Todo, rep.Trees)
	}
	// Branch filter: incoming on "main" excludes the "next" TODO; an
	// unknown branch has no outgoing dependencies.
	rep, err = e.svc.Dependencies(e.ctx, e.alice, &e.web, "main")
	must(t, err)
	if len(rep.Incoming) != 0 || len(rep.Outgoing) != 3 {
		t.Fatalf("main report = %+v", rep)
	}
	rep, err = e.svc.Dependencies(e.ctx, e.alice, &e.web, "nope")
	must(t, err)
	if len(rep.Outgoing) != 0 || rep.Branch != "nope" {
		t.Fatalf("nope report = %+v", rep)
	}
}

func TestParseCompletes(t *testing.T) {
	cases := []struct {
		msg  string
		want []CompletesRef
	}{
		{"nothing", nil},
		{"Completes: #3", []CompletesRef{{Number: 3}}},
		{"title\n\n  completes:   #1,#2  web#3\r\nCOMPLETES: acme/api#4", []CompletesRef{{Number: 1}, {Number: 2}, {Tree: "web", Number: 3}, {Tenant: "acme", Tree: "api", Number: 4}}},
		{"Completes: #0 #-1 # #x a/b/c#1 /x#1 a/#1 #12abc", nil},
		{"Completes:#7", []CompletesRef{{Number: 7}}},
		{"Mention Completes: #1 inline", nil},
		{"Completes", nil},
	}
	for _, c := range cases {
		if got := ParseCompletes(c.msg); !reflect.DeepEqual(got, c.want) {
			t.Errorf("ParseCompletes(%q) = %+v, want %+v", c.msg, got, c.want)
		}
	}
}

func TestParseStatusesAndHelpers(t *testing.T) {
	cases := []struct {
		in   string
		want []string
		err  bool
	}{
		{"", nil, false},
		{"all", nil, false},
		{"open", []string{"open"}, false},
		{"active", []string{"open", "in_progress"}, false},
		{"done, cancelled", []string{"done", "cancelled"}, false},
		{"open,all", nil, false},
		{"bogus", nil, true},
	}
	for _, c := range cases {
		got, err := ParseStatuses(c.in)
		if (err != nil) != c.err || !slices.Equal(got, c.want) {
			t.Errorf("ParseStatuses(%q) = %v, %v", c.in, got, err)
		}
	}
	if got := truncate("héllo", 2); got != "h" {
		t.Errorf("truncate = %q", got)
	}
	long := make([]byte, 300)
	for i := range long {
		long[i] = 'a'
	}
	if got := DependencyTitle(domain.Dependency{Title: string(long)}, &domain.Snapshot{}); len(got) != MaxTitle {
		t.Errorf("title len = %d", len(got))
	}
	if got := DependencyTitle(domain.Dependency{}, &domain.Snapshot{ID: "0123456789abcdef", Message: "\n"}); got != "Dependency of snapshot 0123456789ab" {
		t.Errorf("generated title = %q", got)
	}
	if TodoKey("t", 7) != "t\x000000000007" {
		t.Errorf("key = %q", TodoKey("t", 7))
	}
}
