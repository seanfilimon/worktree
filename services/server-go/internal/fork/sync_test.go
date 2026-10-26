package fork

import (
	"errors"
	"slices"
	"testing"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/fork/snapcopy"
)

func statusOf(t *testing.T, u *Upstream, name string) string {
	t.Helper()
	for _, b := range u.Branches {
		if b.Name == name {
			return b.Status
		}
	}
	t.Fatalf("branch %s not reported: %+v", name, u.Branches)
	return ""
}

func TestUpstreamAndSync(t *testing.T) {
	e := newEnv(t)
	bob := e.users["bob"]
	f, err := e.fork("bob", "bob", Input{Branches: []string{"main"}})
	must(t, err)

	u, err := e.svc.Upstream(e.ctx, bob, f)
	must(t, err)
	if !u.Fork || u.UpstreamTreeID != e.web.ID || u.Tenant != "acme" || u.Name != "web" || u.UpstreamMissing || u.ForkedBy != "bob" {
		t.Fatalf("upstream = %+v", u)
	}
	if statusOf(t, u, "main") != StatusUpToDate || statusOf(t, u, "feature") != StatusUpstreamOnly {
		t.Fatalf("statuses = %+v", u.Branches)
	}
	if nu, err := e.svc.Upstream(e.ctx, bob, e.web); err != nil || nu.Fork || len(nu.Branches) != 0 {
		t.Fatalf("upstream of a non-fork = %+v, %v", nu, err)
	}

	// Upstream moves: the fork is behind and fast-forwards.
	oldMain := e.tip(e.web, "main")
	s4 := e.commit(e.web, map[string]string{"README.md": "# web 4\n"}, oldMain)
	e.branch(e.web, "main", s4)
	u, _ = e.svc.Upstream(e.ctx, bob, f)
	if statusOf(t, u, "main") != StatusBehind {
		t.Fatalf("after upstream push: %+v", u.Branches)
	}
	before := e.tip(f, "main")
	res, err := e.svc.Sync(e.ctx, bob, f, nil)
	must(t, err)
	if res.Updated != 1 || len(res.Diverged) != 0 {
		t.Fatalf("sync = %+v", res)
	}
	want := snapcopy.DerivedID(f.ID, s4)
	if e.tip(f, "main") != want || res.Branches[1].Name != "main" || res.Branches[1].Result != ResultFastForwarded ||
		res.Branches[1].OldTip != before || res.Branches[1].NewTip != want {
		t.Fatalf("sync = %+v, main = %s", res, e.tip(f, "main"))
	}
	if res.Branches[0].Name != "feature" || res.Branches[0].Result != ResultUpstreamOnly {
		t.Fatalf("feature = %+v", res.Branches[0])
	}
	// Naming an upstream-only branch creates it.
	res, err = e.svc.Sync(e.ctx, bob, f, []string{"feature"})
	must(t, err)
	if res.Updated != 1 || res.Branches[0].Result != ResultCreated || e.tip(f, "feature") != snapcopy.DerivedID(f.ID, e.tip(e.web, "feature")) {
		t.Fatalf("create feature = %+v", res)
	}
	if !slices.Contains(e.bus.types(), domain.EventBranchPushed) || !slices.Contains(e.bus.types(), domain.EventBranchCreated) {
		t.Fatalf("events = %v", e.bus.types())
	}

	// The fork moves on: ahead. Then the upstream too: diverged.
	f5 := e.commit(f, map[string]string{"README.md": "fork\n"}, e.tip(f, "main"))
	e.branch(f, "main", f5)
	u, _ = e.svc.Upstream(e.ctx, bob, f)
	if statusOf(t, u, "main") != StatusAhead {
		t.Fatalf("fork ahead: %+v", u.Branches)
	}
	res, err = e.svc.Sync(e.ctx, bob, f, nil)
	must(t, err)
	if res.Updated != 0 || res.Branches[1].Result != ResultAhead {
		t.Fatalf("sync while ahead = %+v", res)
	}
	s6 := e.commit(e.web, map[string]string{"README.md": "# web 6\n"}, s4)
	e.branch(e.web, "main", s6)
	u, _ = e.svc.Upstream(e.ctx, bob, f)
	if statusOf(t, u, "main") != StatusDiverged {
		t.Fatalf("diverged: %+v", u.Branches)
	}
	res, err = e.svc.Sync(e.ctx, bob, f, []string{"main"})
	must(t, err)
	if !slices.Equal(res.Diverged, []string{"main"}) || e.tip(f, "main") != f5 {
		t.Fatalf("diverged sync = %+v", res)
	}
	var rec Record
	must(t, e.store.GetDoc(e.ctx, Coll, domain.DocKey(e.web.ID, f.ID), &rec))
	if rec.LastSyncAt == nil {
		t.Fatal("last_sync_at not recorded")
	}

	// Permissions: carol cannot write the fork; unknown upstream branch.
	if _, err := e.svc.Sync(e.ctx, e.users["carol"], f, nil); !errors.Is(err, domain.ErrForbidden) {
		t.Fatalf("carol sync: %v", err)
	}
	if _, err := e.svc.Sync(e.ctx, bob, f, []string{"nope"}); !errors.Is(err, domain.ErrNotFound) {
		t.Fatalf("unknown branch: %v", err)
	}
	if _, err := e.svc.Sync(e.ctx, e.users["alice"], e.web, nil); !errors.Is(err, domain.ErrInvalid) {
		t.Fatalf("sync of a non-fork: %v", err)
	}
}

func TestUpstreamDeletionLeavesForkIntact(t *testing.T) {
	e := newEnv(t)
	bob := e.users["bob"]
	f, err := e.fork("bob", "bob", Input{})
	must(t, err)
	tip := e.tip(f, "main")
	must(t, e.store.DeleteTree(e.ctx, e.web.ID))
	must(t, e.svc.DeleteTree(e.ctx, e.web.ID))

	u, err := e.svc.Upstream(e.ctx, bob, f)
	must(t, err)
	if !u.Fork || !u.UpstreamMissing || u.UpstreamTreeID != e.web.ID {
		t.Fatalf("upstream after deletion = %+v", u)
	}
	got, err := e.store.GetTree(e.ctx, f.ID)
	must(t, err)
	if got.ForkOf != e.web.ID || e.tip(f, "main") != tip {
		t.Fatalf("fork after upstream deletion = %+v", got)
	}
	if log, err := e.svc.d.History.Log(e.ctx, tip, 0); err != nil || len(log) != 2 {
		t.Fatalf("fork history = %d, %v", len(log), err)
	}
	if _, err := e.svc.Sync(e.ctx, bob, f, nil); !errors.Is(err, domain.ErrConflict) {
		t.Fatalf("sync without upstream: %v, want ErrConflict", err)
	}
	if docs, _ := e.store.ListDocs(e.ctx, Coll, ""); len(docs) != 0 {
		t.Fatalf("fork records of the deleted upstream remain: %d", len(docs))
	}
}

func TestForkDeletionDropsRecord(t *testing.T) {
	e := newEnv(t)
	f, err := e.fork("bob", "bob", Input{})
	must(t, err)
	must(t, e.store.DeleteTree(e.ctx, f.ID))
	must(t, e.svc.DeleteTree(e.ctx, f.ID))
	if docs, _ := e.store.ListDocs(e.ctx, Coll, ""); len(docs) != 0 {
		t.Fatalf("fork record remains")
	}
	if docs, _ := e.store.ListDocs(e.ctx, snapcopy.Coll, ""); len(docs) != 0 {
		t.Fatalf("snapshot origins remain: %d", len(docs))
	}
	list, err := e.svc.List(e.ctx, e.users["alice"], e.web)
	if err != nil || len(list) != 0 {
		t.Fatalf("list = %+v, %v", list, err)
	}
}

func TestListHidesUnreadableForks(t *testing.T) {
	e := newEnv(t)
	e.web.Visibility = domain.VisibilityPrivate
	must(t, e.store.UpdateTree(e.ctx, e.web))
	f, err := e.fork("bob", "bob", Input{})
	must(t, err)
	// alice owns the upstream but cannot read bob's private fork.
	list, err := e.svc.List(e.ctx, e.users["alice"], e.web)
	must(t, err)
	if len(list) != 0 {
		t.Fatalf("alice sees %+v", list)
	}
	list, err = e.svc.List(e.ctx, e.users["bob"], e.web)
	must(t, err)
	if len(list) != 1 || list[0].TreeID != f.ID || list[0].Visibility != domain.VisibilityPrivate {
		t.Fatalf("bob sees %+v", list)
	}
	if _, err := e.svc.List(e.ctx, e.users["carol"], e.web); err == nil {
		t.Fatal("carol listed forks of a tree she cannot read")
	}
}
