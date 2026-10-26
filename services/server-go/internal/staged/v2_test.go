package staged

// v0.8 (§16.8) tests: labels, lifecycle/history, TTL sweep, overlaps.

import (
	"context"
	"errors"
	"strings"
	"testing"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
)

func stageL(t *testing.T, e *env, p *domain.Principal, branch, label, snap string, files ...string) domain.StagedSnapshot {
	t.Helper()
	s := &domain.StagedSnapshot{SnapshotID: snap, BranchName: branch, Label: label, FilesChanged: files}
	if err := e.svc.Stage(context.Background(), p, tree, s); err != nil {
		t.Fatalf("Stage(%s, %s, %q, %s): %v", p.Username, branch, label, snap, err)
	}
	return *s
}

func labelsOf(es []domain.StagedSnapshot) string {
	var out []string
	for _, e := range es {
		out = append(out, e.Author+"/"+e.BranchName+"/"+e.Label+"="+e.SnapshotID)
	}
	return strings.Join(out, ",")
}

func TestValidateLabel(t *testing.T) {
	for _, ok := range []string{"", "a", "0", "feature-x", "v1.2_rc", strings.Repeat("a", 64)} {
		if err := ValidateLabel(ok); err != nil {
			t.Errorf("ValidateLabel(%q) = %v", ok, err)
		}
	}
	for _, bad := range []string{"A", "-x", ".x", "_x", "a b", "a/b", strings.Repeat("a", 65), "é"} {
		if err := ValidateLabel(bad); !errors.Is(err, domain.ErrInvalid) {
			t.Errorf("ValidateLabel(%q) = %v, want ErrInvalid", bad, err)
		}
	}
	e := newEnv()
	err := e.svc.Stage(context.Background(), alice, tree, &domain.StagedSnapshot{SnapshotID: "s", BranchName: "main", Label: "Bad"})
	if !errors.Is(err, domain.ErrInvalid) {
		t.Fatalf("stage with bad label: %v", err)
	}
}

func TestLabeledSlotsAreIndependent(t *testing.T) {
	e := newEnv()
	stageL(t, e, alice, "main", "", "d1", "a.txt")
	e.clock.advance(time.Second)
	stageL(t, e, alice, "main", "exp", "x1", "b.txt")
	e.clock.advance(time.Second)
	stageL(t, e, alice, "main", "exp", "x2", "c.txt") // replaces x1
	got, err := e.svc.List(context.Background(), alice, tree, "main")
	if err != nil {
		t.Fatal(err)
	}
	if s := labelsOf(got); s != "alice/main/exp=x2,alice/main/=d1" {
		t.Fatalf("list = %s", s)
	}
	if got[0].Label != "exp" || got[0].FilesChanged[0] != "c.txt" {
		t.Fatalf("labeled entry = %+v", got[0])
	}
}

func TestLabelLimitPerBranch(t *testing.T) {
	e := newEnv()
	for i := 0; i < MaxLabelsPerBranch; i++ {
		stageL(t, e, alice, "main", string(rune('a'+i)), "s")
	}
	err := e.svc.Stage(context.Background(), alice, tree, &domain.StagedSnapshot{SnapshotID: "s", BranchName: "main", Label: "z"})
	if !errors.Is(err, domain.ErrInvalid) || !strings.Contains(err.Error(), "max 10") {
		t.Fatalf("11th label: %v", err)
	}
	// Replacing an existing slot, another branch and another account are fine.
	stageL(t, e, alice, "main", "a", "s2")
	stageL(t, e, alice, "dev", "z", "s")
	stageL(t, e, bob, "main", "z", "s")
	// Expired entries do not count.
	e.clock.advance(DefaultMaxAge + time.Minute)
	stageL(t, e, alice, "main", "z", "s3")
}

func TestPreV08EntryIsTheDefaultSlot(t *testing.T) {
	// An entry stored before labels existed has no label and the old key:
	// it is the default slot, replaced by an unlabeled stage.
	mem := meta.NewMemory()
	ctx := context.Background()
	if err := mem.PutStaged(ctx, &domain.StagedSnapshot{SnapshotID: "old", TreeID: tree.ID, AccountID: alice.AccountID,
		Author: "alice", BranchName: "main", StagedAt: time.Now()}); err != nil {
		t.Fatal(err)
	}
	svc := New(mem, &fakeAuthz{deny: map[domain.Action]bool{}}, nil)
	if err := svc.Stage(ctx, alice, tree, &domain.StagedSnapshot{SnapshotID: "new", BranchName: "main"}); err != nil {
		t.Fatal(err)
	}
	got, _ := svc.List(ctx, alice, tree, "")
	if len(got) != 1 || got[0].SnapshotID != "new" || got[0].Label != "" {
		t.Fatalf("got %+v", got)
	}
}

func TestClearOnPushLabelsAndHistory(t *testing.T) {
	e := newEnv()
	ctx := context.Background()
	mh := hash.Sum([]byte("manifest"))
	must := func(err error) {
		t.Helper()
		if err != nil {
			t.Fatal(err)
		}
	}
	must(e.store.PutSnapshot(ctx, &domain.Snapshot{ID: "tip", TreeID: tree.ID, BranchName: "main", ManifestHash: mh}))
	must(e.store.PutBranch(ctx, &domain.Branch{TreeID: tree.ID, Name: "main", Tip: "tip"}))

	stageL(t, e, alice, "main", "", "wip")
	same := &domain.StagedSnapshot{SnapshotID: "other-id", BranchName: "main", Label: "same", ManifestHash: mh.Hex()}
	must(e.svc.Stage(ctx, alice, tree, same))
	stageL(t, e, alice, "main", "tipid", "tip")
	stageL(t, e, alice, "main", "keep", "unrelated")
	stageL(t, e, bob, "main", "", "bobs")

	e.clock.advance(time.Minute)
	must(e.svc.ClearOnPush(ctx, alice, tree, "main"))
	got, _ := e.svc.List(ctx, alice, tree, "")
	if s := labelsOf(got); s != "alice/main/keep=unrelated,bob/main/=bobs" {
		t.Fatalf("after push: %s", s)
	}
	hist, err := e.svc.History(ctx, alice, tree, HistoryFilter{})
	must(err)
	if len(hist) != 3 {
		t.Fatalf("history = %+v", hist)
	}
	for _, h := range hist {
		if h.Status != StatusPushed || h.EndedBy != "alice" || h.PushedTip != "tip" || h.ID == "" || h.FilesChanged == nil {
			t.Fatalf("history entry = %+v", h)
		}
	}
	cleared := e.bus.ofType(domain.EventStagedCleared)
	if len(cleared) != 3 || cleared[0].Payload.(Cleared).Status != StatusPushed {
		t.Fatalf("events = %+v", cleared)
	}
}

func TestUnstageLabelAndHistory(t *testing.T) {
	e := newEnv()
	ctx := context.Background()
	stageL(t, e, alice, "main", "", "d")
	stageL(t, e, alice, "main", "a", "la")
	stageL(t, e, alice, "dev", "a", "da")

	removed, err := e.svc.UnstageLabel(ctx, alice, tree, "", "main", "a")
	if err != nil || labelsOf(removed) != "alice/main/a=la" {
		t.Fatalf("UnstageLabel = %s, %v", labelsOf(removed), err)
	}
	if removed, err = e.svc.UnstageLabel(ctx, alice, tree, "", "main", "a"); err != nil || len(removed) != 0 {
		t.Fatalf("second UnstageLabel = %v, %v", removed, err)
	}
	if _, err := e.svc.UnstageLabel(ctx, alice, tree, "", "", "a"); !errors.Is(err, domain.ErrInvalid) {
		t.Fatalf("no branch: %v", err)
	}
	if _, err := e.svc.UnstageLabel(ctx, alice, tree, "", "main", "BAD"); !errors.Is(err, domain.ErrInvalid) {
		t.Fatalf("bad label: %v", err)
	}
	// Admin removing another account's slot.
	e.authz.deny[domain.ActTreeAdmin] = true
	if _, err := e.svc.UnstageLabel(ctx, bob, tree, alice.AccountID, "main", ""); !errors.Is(err, errDenied) {
		t.Fatalf("bob without admin: %v", err)
	}
	delete(e.authz.deny, domain.ActTreeAdmin)
	// Unstage without label removes every label of the branch(es).
	removed, err = e.svc.Unstage(ctx, alice, tree, "", "")
	if err != nil || labelsOf(removed) != "alice/dev/a=da,alice/main/=d" {
		t.Fatalf("Unstage all = %s, %v", labelsOf(removed), err)
	}
	hist, _ := e.svc.History(ctx, alice, tree, HistoryFilter{Status: StatusCleared})
	if len(hist) != 3 {
		t.Fatalf("history = %+v", hist)
	}
	if hist, _ := e.svc.History(ctx, alice, tree, HistoryFilter{Branch: "dev"}); len(hist) != 1 || hist[0].Label != "a" {
		t.Fatalf("history dev = %+v", hist)
	}
	if hist, _ := e.svc.History(ctx, alice, tree, HistoryFilter{AccountID: bob.AccountID}); len(hist) != 0 {
		t.Fatalf("history bob = %+v", hist)
	}
	if hist, _ := e.svc.History(ctx, alice, tree, HistoryFilter{Limit: 2}); len(hist) != 2 {
		t.Fatalf("history limit = %+v", hist)
	}
	if hist, err := e.svc.History(ctx, nil, tree, HistoryFilter{}); err != nil || len(hist) != 0 {
		t.Fatalf("anonymous history = %+v, %v", hist, err)
	}
}

func TestSweepExpiresAndPrunes(t *testing.T) {
	e := newEnv()
	ctx := context.Background()
	if err := e.store.CreateTree(ctx, &domain.Tree{ID: tree.ID, TenantID: tree.TenantID, Name: tree.Name}); err != nil {
		t.Fatal(err)
	}
	stageL(t, e, alice, "main", "old", "o")
	e.clock.advance(DefaultMaxAge - time.Hour)
	stageL(t, e, bob, "main", "", "fresh")
	e.clock.advance(2 * time.Hour)

	rep, err := e.svc.Sweep(ctx)
	if err != nil || rep.Expired != 1 || rep.Pruned != 0 {
		t.Fatalf("sweep = %+v, %v", rep, err)
	}
	if got, _ := e.svc.List(ctx, bob, tree, ""); labelsOf(got) != "bob/main/=fresh" {
		t.Fatalf("live = %s", labelsOf(got))
	}
	hist, _ := e.svc.History(ctx, bob, tree, HistoryFilter{})
	if len(hist) != 1 || hist[0].Status != StatusExpired || hist[0].EndedBy != "system" || hist[0].Label != "old" {
		t.Fatalf("history = %+v", hist)
	}
	// History is kept HistoryRetention, then hidden and pruned.
	e.clock.advance(DefaultHistoryRetention + time.Hour)
	if hist, _ := e.svc.History(ctx, bob, tree, HistoryFilter{}); len(hist) != 0 {
		t.Fatalf("history after retention = %+v", hist)
	}
	rep, err = e.svc.Sweep(ctx)
	if err != nil || rep.Pruned != 1 || rep.Expired != 1 {
		t.Fatalf("second sweep = %+v, %v", rep, err)
	}
	raws, _ := e.store.ListDocs(ctx, CollHistory, "")
	if len(raws) != 1 { // bob's entry expired in the second sweep
		t.Fatalf("history docs = %d", len(raws))
	}
	e.svc.HandleTreeDeleted(ctx, tree.ID)
	if raws, _ := e.store.ListDocs(ctx, CollHistory, ""); len(raws) != 0 {
		t.Fatalf("history after tree deletion = %d", len(raws))
	}

	// TTL 0 disables expiry; retention 0 records no history.
	e2 := newEnv()
	e2.svc.MaxAge, e2.svc.HistoryRetention = 0, 0
	stageL(t, e2, alice, "main", "", "x")
	e2.clock.advance(10 * DefaultMaxAge)
	if rep, _ := e2.svc.Sweep(ctx); rep.Expired != 0 {
		t.Fatalf("TTL 0 sweep = %+v", rep)
	}
	if _, err := e2.svc.Unstage(ctx, alice, tree, "", ""); err != nil {
		t.Fatal(err)
	}
	if raws, _ := e2.store.ListDocs(ctx, CollHistory, ""); len(raws) != 0 {
		t.Fatalf("retention 0 kept history: %d", len(raws))
	}
}

func TestStartSweepsAndHandlesTreeDeletion(t *testing.T) {
	mem := meta.NewMemory()
	bus := newChanBus()
	svc := New(mem, &fakeAuthz{deny: map[domain.Action]bool{}}, bus)
	svc.SweepInterval = 10 * time.Millisecond
	svc.MaxAge = time.Millisecond
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	if err := mem.CreateTree(ctx, &domain.Tree{ID: tree.ID, TenantID: tree.TenantID, Name: tree.Name}); err != nil {
		t.Fatal(err)
	}
	if err := svc.Stage(ctx, alice, tree, &domain.StagedSnapshot{SnapshotID: "s", BranchName: "main"}); err != nil {
		t.Fatal(err)
	}
	svc.Start(ctx)
	deadline := time.Now().Add(5 * time.Second)
	for {
		raws, _ := mem.ListDocs(ctx, CollHistory, "")
		if len(raws) == 1 {
			break
		}
		if time.Now().After(deadline) {
			t.Fatal("sweeper did not expire the entry")
		}
		time.Sleep(5 * time.Millisecond)
	}
	bus.Publish(domain.Event{Type: domain.EventTreeDeleted, TreeID: tree.ID})
	for {
		raws, _ := mem.ListDocs(ctx, CollHistory, "")
		if len(raws) == 0 {
			break
		}
		if time.Now().After(deadline) {
			t.Fatal("history not deleted on tree.deleted")
		}
		time.Sleep(5 * time.Millisecond)
	}
}

// chanBus is a minimal subscribable bus.
type chanBus struct{ ch chan domain.Event }

func newChanBus() *chanBus { return &chanBus{ch: make(chan domain.Event, 16)} }

func (b *chanBus) Publish(e domain.Event) {
	if e.Type == domain.EventTreeDeleted {
		b.ch <- e
	}
}

func (b *chanBus) Subscribe(domain.EventFilter) (<-chan domain.Event, func()) { return b.ch, func() {} }

func TestOverlaps(t *testing.T) {
	e := newEnv()
	ctx := context.Background()
	stageL(t, e, alice, "main", "", "a1", "src/a.go", "src/b.go")
	stageL(t, e, bob, "main", "", "b1", "src/b.go", "README.md")
	stageL(t, e, bob, "dev", "exp", "b2", "src/a.go")
	stageL(t, e, carol, "main", "", "c1", "docs/x.md")

	got, err := e.svc.Overlaps(ctx, alice, tree, []string{"src/a.go", "./src/b.go", "README.md", "src/a.go"})
	if err != nil {
		t.Fatal(err)
	}
	if len(got) != 2 {
		t.Fatalf("overlaps = %+v", got)
	}
	if got[0].Author != "bob" || got[0].Branch != "dev" || got[0].Label != "exp" || strings.Join(got[0].Paths, ",") != "src/a.go" {
		t.Fatalf("first = %+v", got[0])
	}
	if got[1].Branch != "main" || strings.Join(got[1].Paths, ",") != "README.md,src/b.go" {
		t.Fatalf("second = %+v", got[1])
	}
	if got, _ := e.svc.Overlaps(ctx, alice, tree, nil); got == nil || len(got) != 0 {
		t.Fatalf("no paths = %+v", got)
	}
	if _, err := e.svc.Overlaps(ctx, nil, tree, []string{"x"}); !errors.Is(err, domain.ErrUnauthenticated) {
		t.Fatalf("anonymous: %v", err)
	}
	if _, err := e.svc.Overlaps(ctx, alice, tree, make([]string, MaxOverlapPaths+1)); !errors.Is(err, domain.ErrInvalid) {
		t.Fatalf("too many: %v", err)
	}
	e.authz.deny[domain.ActTreeRead] = true
	if _, err := e.svc.Overlaps(ctx, alice, tree, []string{"x"}); !errors.Is(err, errDenied) {
		t.Fatalf("denied: %v", err)
	}
}

func TestSummaryHiddenFromAnonymous(t *testing.T) {
	e := newEnv()
	stageL(t, e, alice, "main", "l", "s", "a")
	got, err := e.svc.Summary(context.Background(), nil, tree)
	if err != nil || got == nil || len(got) != 0 {
		t.Fatalf("anonymous summary = %+v, %v", got, err)
	}
	got, _ = e.svc.Summary(context.Background(), bob, tree)
	if len(got) != 1 || got[0].Authors[0].Label != "l" {
		t.Fatalf("summary = %+v", got)
	}
}
