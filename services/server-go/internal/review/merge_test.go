package review

import (
	"context"
	"errors"
	"fmt"
	"slices"
	"sort"
	"strings"
	"sync"
	"testing"
	"time"

	"google.golang.org/protobuf/proto"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/merge3"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
	"github.com/seanfilimon/worktree/services/server-go/internal/objstore"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
)

// memObjects is an in-memory objstore.Store counting reads.
type memObjects struct {
	mu    sync.Mutex
	objs  map[hash.Hash][]byte
	reads int
}

func newMemObjects() *memObjects { return &memObjects{objs: map[hash.Hash][]byte{}} }

func (o *memObjects) Put(_ context.Context, h hash.Hash, data []byte) error {
	if hash.Sum(data) != h {
		return objstore.ErrHashMismatch
	}
	o.mu.Lock()
	defer o.mu.Unlock()
	o.objs[h] = append([]byte(nil), data...)
	return nil
}

func (o *memObjects) Get(_ context.Context, h hash.Hash) ([]byte, error) {
	o.mu.Lock()
	defer o.mu.Unlock()
	o.reads++
	d, ok := o.objs[h]
	if !ok {
		return nil, objstore.ErrNotFound
	}
	return d, nil
}

func (o *memObjects) Has(_ context.Context, h hash.Hash) (bool, error) {
	o.mu.Lock()
	defer o.mu.Unlock()
	_, ok := o.objs[h]
	return ok, nil
}

func (o *memObjects) Delete(_ context.Context, h hash.Hash) error {
	o.mu.Lock()
	defer o.mu.Unlock()
	delete(o.objs, h)
	return nil
}

func (o *memObjects) readCount() int {
	o.mu.Lock()
	defer o.mu.Unlock()
	return o.reads
}

// casHookStore runs beforeCAS before every compare-and-swap.
type casHookStore struct {
	*meta.Memory
	beforeCAS func()
}

func (s *casHookStore) CompareAndSwapBranchTip(ctx context.Context, tree, name, oldTip, newTip string) error {
	if s.beforeCAS != nil {
		s.beforeCAS()
	}
	return s.Memory.CompareAndSwapBranchTip(ctx, tree, name, oldTip, newTip)
}

// menv is a review service over a real in-memory store, history and object
// store, for server-side merges.
type menv struct {
	svc   *Service
	store *casHookStore
	objs  *memObjects
	bus   *fakeBus
	audit *fakeAudit
	tree  *domain.Tree
	n     int
	clock time.Time
}

func newMergeEnv(t *testing.T) *menv {
	t.Helper()
	ctx := context.Background()
	store := &casHookStore{Memory: meta.NewMemory()}
	e := &menv{store: store, objs: newMemObjects(), bus: &fakeBus{}, audit: &fakeAudit{}, clock: fixedNow.Add(-time.Hour)}
	must := func(err error) {
		t.Helper()
		if err != nil {
			t.Fatal(err)
		}
	}
	must(store.CreateTenant(ctx, &domain.Tenant{ID: "ten-acme", Slug: "acme", Name: "Acme", Kind: domain.TenantOrg}))
	e.tree = &domain.Tree{ID: "tree-1", TenantID: "ten-acme", Name: "proj", DefaultBranch: "main"}
	must(store.CreateTree(ctx, e.tree))
	authz := &fakeAuthz{grants: map[string][]domain.Action{
		"alice": developer, "bob": developer, "carol": maintainer, "eve": {domain.ActTreeRead},
	}}
	e.svc = New(store, meta.NewHistory(store), authz, e.bus, e.audit)
	e.svc.SetObjects(e.objs)
	e.svc.now = func() time.Time { return fixedNow }
	ids := 0
	e.svc.newID = func() string { ids++; return fmt.Sprintf("srv-%d", ids) }
	return e
}

// commit stores a snapshot with the given files (path → content; a trailing
// "*" in the path marks it executable) and returns its id.
func (e *menv) commit(t *testing.T, files map[string]string, parents ...string) string {
	t.Helper()
	ctx := context.Background()
	m := &syncpb.ManifestRecord{}
	paths := make([]string, 0, len(files))
	for p := range files {
		paths = append(paths, p)
	}
	sort.Strings(paths)
	for _, p := range paths {
		data := []byte(files[p])
		h := hash.Sum(data)
		if err := e.objs.Put(ctx, h, data); err != nil {
			t.Fatal(err)
		}
		exec := strings.HasSuffix(p, "*")
		m.Entries = append(m.Entries, &syncpb.ManifestEntry{
			Path: strings.TrimSuffix(p, "*"), BlobHash: &syncpb.ContentHash{Value: h[:]}, Size: uint64(len(data)), Executable: exec,
		})
	}
	raw, err := proto.Marshal(m)
	if err != nil {
		t.Fatal(err)
	}
	mh := hash.Sum(raw)
	if err := e.objs.Put(ctx, mh, raw); err != nil {
		t.Fatal(err)
	}
	e.n++
	e.clock = e.clock.Add(time.Minute)
	id := fmt.Sprintf("c%02d", e.n)
	if err := e.store.PutSnapshot(ctx, &domain.Snapshot{ID: id, TreeID: e.tree.ID, Parents: parents, ManifestHash: mh, Author: "alice", Message: id, CreatedAt: e.clock}); err != nil {
		t.Fatal(err)
	}
	return id
}

func (e *menv) branch(t *testing.T, name, tip string) {
	t.Helper()
	if err := e.store.PutBranch(context.Background(), &domain.Branch{TreeID: e.tree.ID, Name: name, Tip: tip}); err != nil {
		t.Fatal(err)
	}
}

func (e *menv) tip(t *testing.T, name string) string {
	t.Helper()
	b, err := e.store.GetBranch(context.Background(), e.tree.ID, name)
	if err != nil {
		t.Fatal(err)
	}
	return b.Tip
}

func (e *menv) open(t *testing.T, source string) *domain.MergeRequest {
	t.Helper()
	mr, err := e.svc.Open(context.Background(), alice, e.tree, "Add "+source, "", source, "main")
	if err != nil {
		t.Fatal(err)
	}
	return mr
}

func (e *menv) protect(t *testing.T, rules ...domain.BranchProtection) {
	t.Helper()
	for i := range rules {
		rules[i].TreeID = e.tree.ID
		if err := e.store.PutProtection(context.Background(), &rules[i]); err != nil {
			t.Fatal(err)
		}
	}
}

// files returns the content of snapshot id ("*" suffix = executable).
func (e *menv) files(t *testing.T, id string) map[string]string {
	t.Helper()
	ctx := context.Background()
	snap, err := e.store.GetSnapshot(ctx, id)
	if err != nil {
		t.Fatal(err)
	}
	m, err := e.svc.loadManifest(ctx, snap.ManifestHash)
	if err != nil {
		t.Fatal(err)
	}
	out := map[string]string{}
	for _, en := range m.GetEntries() {
		var h hash.Hash
		copy(h[:], en.GetBlobHash().GetValue())
		data, err := e.objs.Get(ctx, h)
		if err != nil {
			t.Fatalf("%s: %v", en.GetPath(), err)
		}
		p := en.GetPath()
		if en.GetExecutable() {
			p += "*"
		}
		out[p] = string(data)
	}
	return out
}

const fiveLines = "1\n2\n3\n4\n5\n"

// diverged builds base ← main (edits a.txt line 1) and base ← feature
// (edits a.txt line 5, adds c.txt), and opens feature → main.
func (e *menv) diverged(t *testing.T) (base, mainTip, featTip string) {
	t.Helper()
	base = e.commit(t, map[string]string{"a.txt": fiveLines, "b.txt": "b\n"})
	mainTip = e.commit(t, map[string]string{"a.txt": "ONE\n2\n3\n4\n5\n", "b.txt": "b\n"}, base)
	featTip = e.commit(t, map[string]string{"a.txt": "1\n2\n3\n4\nFIVE\n", "b.txt": "b\n", "c.txt": "new\n"}, base)
	e.branch(t, "main", mainTip)
	e.branch(t, "feature", featTip)
	e.open(t, "feature")
	return base, mainTip, featTip
}

func mergedFiles() map[string]string {
	return map[string]string{"a.txt": "ONE\n2\n3\n4\nFIVE\n", "b.txt": "b\n", "c.txt": "new\n"}
}

func TestServerMergeOfDivergedBranches(t *testing.T) {
	e := newMergeEnv(t)
	_, mainTip, featTip := e.diverged(t)

	// Readiness and mergeability agree: no fast-forward, merge and squash
	// possible, no conflicts.
	c, err := e.svc.Checks(ctx, eve, e.tree, 1)
	if err != nil {
		t.Fatal(err)
	}
	if !c.Mergeable || len(c.Blockers) != 0 || !slices.Equal(c.Methods, []string{domain.MergeCommit, domain.MergeSquash}) {
		t.Fatalf("checks = %+v", c)
	}
	mb, err := e.svc.Mergeability(ctx, eve, e.tree, 1)
	if err != nil {
		t.Fatal(err)
	}
	if mb.FastForward || mb.UpToDate || mb.Default != domain.MergeCommit || len(mb.Conflicts) != 0 ||
		!slices.Equal(mb.Methods, []string{domain.MergeCommit, domain.MergeSquash}) ||
		!slices.Equal(mb.Allowed, AllMergeMethods) || mb.MergeBase != "c01" || mb.Reasons[domain.MergeFastForward] == "" {
		t.Fatalf("mergeability = %+v", mb)
	}

	// Default method: merge.
	mr, err := e.svc.Merge(ctx, carol, e.tree, 1)
	if err != nil {
		t.Fatal(err)
	}
	if mr.MergeMethod != domain.MergeCommit || mr.MergedSnapshot != "srv-1" || mr.State != domain.MRStateMerged {
		t.Fatalf("mr = %+v", mr)
	}
	if got := e.tip(t, "main"); got != "srv-1" {
		t.Fatalf("main = %s", got)
	}
	snap, err := e.store.GetSnapshot(ctx, "srv-1")
	if err != nil {
		t.Fatal(err)
	}
	if !slices.Equal(snap.Parents, []string{mainTip, featTip}) || snap.Author != "carol" || snap.PushedBy != carol.AccountID ||
		snap.Message != "Merge branch 'feature' into main (!1)" || snap.BranchName != "main" || !snap.CreatedAt.Equal(fixedNow) {
		t.Fatalf("merge snapshot = %+v", snap)
	}
	eq(t, e.files(t, "srv-1"), mergedFiles())

	push := e.bus.events[len(e.bus.events)-1]
	if push.Type != domain.EventBranchPushed || push.Payload.(map[string]any)["new_tip"] != "srv-1" ||
		push.Payload.(map[string]any)["merge_method"] != domain.MergeCommit {
		t.Fatalf("last event = %+v", push)
	}
	if d := e.audit.events[len(e.audit.events)-1].Details; d["method"] != domain.MergeCommit || d["new_tip"] != "srv-1" || d["old_tip"] != mainTip {
		t.Fatalf("audit = %v", d)
	}
}

func TestServerSquash(t *testing.T) {
	e := newMergeEnv(t)
	_, mainTip, _ := e.diverged(t)
	mr, err := e.svc.MergeWith(ctx, carol, e.tree, 1, MergeOptions{Method: domain.MergeSquash})
	if err != nil {
		t.Fatal(err)
	}
	snap, err := e.store.GetSnapshot(ctx, mr.MergedSnapshot)
	if err != nil {
		t.Fatal(err)
	}
	if mr.MergeMethod != domain.MergeSquash || !slices.Equal(snap.Parents, []string{mainTip}) || snap.Message != "Add feature" {
		t.Fatalf("squash: mr %+v snapshot %+v", mr, snap)
	}
	eq(t, e.files(t, snap.ID), mergedFiles())

	// Squash of a fast-forwardable request: one snapshot with the source tree.
	e2 := newMergeEnv(t)
	base := e2.commit(t, map[string]string{"a.txt": "a\n"})
	f1 := e2.commit(t, map[string]string{"a.txt": "a\n", "b.txt": "b\n"}, base)
	f2 := e2.commit(t, map[string]string{"a.txt": "A\n", "b.txt": "b\n"}, f1)
	e2.branch(t, "main", base)
	e2.branch(t, "feature", f2)
	e2.open(t, "feature")
	mr, err = e2.svc.MergeWith(ctx, carol, e2.tree, 1, MergeOptions{Method: domain.MergeSquash, Message: "  Squashed!  "})
	if err != nil {
		t.Fatal(err)
	}
	snap, _ = e2.store.GetSnapshot(ctx, mr.MergedSnapshot)
	if !slices.Equal(snap.Parents, []string{base}) || snap.Message != "Squashed!" {
		t.Fatalf("snapshot = %+v", snap)
	}
	eq(t, e2.files(t, snap.ID), e2.files(t, f2))
}

func TestServerMergeNoFastForward(t *testing.T) {
	e := newMergeEnv(t)
	base := e.commit(t, map[string]string{"a.txt": "a\n"})
	feat := e.commit(t, map[string]string{"a.txt": "A\n"}, base)
	e.branch(t, "main", base)
	e.branch(t, "feature", feat)
	e.open(t, "feature")
	mb, err := e.svc.Mergeability(ctx, eve, e.tree, 1)
	if err != nil {
		t.Fatal(err)
	}
	if !mb.FastForward || mb.Default != domain.MergeFastForward || !slices.Equal(mb.Methods, AllMergeMethods) || mb.MergeBase != base {
		t.Fatalf("mergeability = %+v", mb)
	}
	mr, err := e.svc.MergeWith(ctx, carol, e.tree, 1, MergeOptions{Method: domain.MergeCommit})
	if err != nil {
		t.Fatal(err)
	}
	snap, _ := e.store.GetSnapshot(ctx, mr.MergedSnapshot)
	if !slices.Equal(snap.Parents, []string{base, feat}) {
		t.Fatalf("parents = %v", snap.Parents)
	}
	eq(t, e.files(t, snap.ID), map[string]string{"a.txt": "A\n"})
}

func TestServerMergeConflicts(t *testing.T) {
	e := newMergeEnv(t)
	base := e.commit(t, map[string]string{"a.txt": fiveLines, "bin": "\x00x"})
	mainTip := e.commit(t, map[string]string{"a.txt": "1\nmain\n3\n4\n5\n", "bin": "\x00main"}, base)
	feat := e.commit(t, map[string]string{"a.txt": "1\nfeature\n3\n4\n5\n", "bin": "\x00feat"}, base)
	e.branch(t, "main", mainTip)
	e.branch(t, "feature", feat)
	e.open(t, "feature")
	want := []Conflict{{Path: "a.txt", Kind: merge3.KindContent}, {Path: "bin", Kind: merge3.KindBinary}}

	c, err := e.svc.Checks(ctx, eve, e.tree, 1)
	if err != nil {
		t.Fatal(err)
	}
	if c.Mergeable || !slices.Equal(kinds(c), []string{BlockerConflicts}) || !strings.Contains(c.Blockers[0].Message, "a.txt (content)") ||
		len(c.Methods) != 0 || !slices.Equal(c.Conflicts, want) {
		t.Fatalf("checks = %+v", c)
	}
	reads := e.objs.readCount()
	mb, err := e.svc.Mergeability(ctx, eve, e.tree, 1)
	if err != nil {
		t.Fatal(err)
	}
	if e.objs.readCount() != reads {
		t.Fatal("mergeability recomputed the cached three-way merge")
	}
	if len(mb.Methods) != 0 || mb.Default != "" || !slices.Equal(mb.Conflicts, want) {
		t.Fatalf("mergeability = %+v", mb)
	}

	for _, method := range []string{"", domain.MergeCommit, domain.MergeSquash} {
		_, err := e.svc.MergeWith(ctx, carol, e.tree, 1, MergeOptions{Method: method})
		var ce *ConflictError
		if !errors.As(err, &ce) || !errors.Is(err, domain.ErrConflict) || !slices.Equal(ce.Conflicts, want) {
			t.Fatalf("method %q: err = %v", method, err)
		}
	}
	if _, err := e.svc.MergeWith(ctx, carol, e.tree, 1, MergeOptions{Method: domain.MergeFastForward}); err == nil ||
		!strings.Contains(err.Error(), "diverged") {
		t.Fatalf("fast-forward err = %v", err)
	}
	if e.tip(t, "main") != mainTip {
		t.Fatal("main moved")
	}
	if ok, _ := e.store.HasSnapshot(ctx, "srv-1"); ok {
		t.Fatal("a snapshot was stored for a conflicting merge")
	}
}

func TestServerMergeMethodRestrictions(t *testing.T) {
	t.Run("merge methods list", func(t *testing.T) {
		e := newMergeEnv(t)
		base := e.commit(t, map[string]string{"a.txt": "a\n"})
		feat := e.commit(t, map[string]string{"a.txt": "A\n"}, base)
		e.branch(t, "main", base)
		e.branch(t, "feature", feat)
		e.open(t, "feature")
		e.protect(t, domain.BranchProtection{Pattern: "main", MergeMethods: []string{domain.MergeSquash, domain.MergeCommit}},
			domain.BranchProtection{Pattern: "*", MergeMethods: []string{domain.MergeSquash, domain.MergeFastForward}})
		mb, err := e.svc.Mergeability(ctx, eve, e.tree, 1)
		if err != nil {
			t.Fatal(err)
		}
		if !mb.FastForward || !slices.Equal(mb.Allowed, []string{domain.MergeSquash}) || !slices.Equal(mb.Methods, []string{domain.MergeSquash}) ||
			!strings.Contains(mb.Reasons[domain.MergeFastForward], "not allowed") {
			t.Fatalf("mergeability = %+v", mb)
		}
		for _, m := range []string{domain.MergeFastForward, domain.MergeCommit} {
			_, err := e.svc.MergeWith(ctx, carol, e.tree, 1, MergeOptions{Method: m})
			var me *MethodError
			if !errors.As(err, &me) || !errors.Is(err, domain.ErrConflict) || !strings.Contains(err.Error(), "allowed: squash") {
				t.Fatalf("%s: err = %v", m, err)
			}
		}
		mr, err := e.svc.Merge(ctx, carol, e.tree, 1) // default: the only allowed method
		if err != nil || mr.MergeMethod != domain.MergeSquash {
			t.Fatalf("default merge = %+v, %v", mr, err)
		}
	})

	t.Run("nothing allowed", func(t *testing.T) {
		e := newMergeEnv(t)
		e.diverged(t)
		e.protect(t, domain.BranchProtection{Pattern: "main", MergeMethods: []string{domain.MergeFastForward}},
			domain.BranchProtection{Pattern: "main*", MergeMethods: []string{domain.MergeSquash}})
		c := mustChecks(t, e)
		if c.Mergeable || !slices.Equal(kinds(c), []string{BlockerDiverged}) {
			t.Fatalf("checks = %+v", c)
		}
		_, err := e.svc.Merge(ctx, carol, e.tree, 1)
		var me *MethodError
		if errors.As(err, &me) || !errors.Is(err, domain.ErrConflict) || !strings.Contains(err.Error(), "diverged") {
			t.Fatalf("err = %v", err)
		}
	})

	t.Run("require linear", func(t *testing.T) {
		e := newMergeEnv(t)
		_, mainTip, _ := e.diverged(t)
		e.protect(t, domain.BranchProtection{Pattern: "main", RequireLinear: true})
		c := mustChecks(t, e)
		if !c.Mergeable || !slices.Equal(c.Methods, []string{domain.MergeSquash}) {
			t.Fatalf("checks = %+v", c)
		}
		_, err := e.svc.MergeWith(ctx, carol, e.tree, 1, MergeOptions{Method: domain.MergeCommit})
		var me *MethodError
		if !errors.As(err, &me) || !strings.Contains(err.Error(), "linear history") {
			t.Fatalf("merge err = %v", err)
		}
		mr, err := e.svc.Merge(ctx, carol, e.tree, 1)
		if err != nil || mr.MergeMethod != domain.MergeSquash {
			t.Fatalf("default = %+v, %v", mr, err)
		}
		snap, _ := e.store.GetSnapshot(ctx, mr.MergedSnapshot)
		if !slices.Equal(snap.Parents, []string{mainTip}) {
			t.Fatalf("parents = %v", snap.Parents)
		}
	})

	t.Run("require linear with merge snapshots in the source", func(t *testing.T) {
		e := newMergeEnv(t)
		base := e.commit(t, map[string]string{"a.txt": "a\n"})
		x := e.commit(t, map[string]string{"a.txt": "a\n", "x": "x\n"}, base)
		y := e.commit(t, map[string]string{"a.txt": "a\n", "y": "y\n"}, base)
		m := e.commit(t, map[string]string{"a.txt": "a\n", "x": "x\n", "y": "y\n"}, x, y)
		e.branch(t, "main", base)
		e.branch(t, "feature", m)
		e.open(t, "feature")
		e.protect(t, domain.BranchProtection{Pattern: "main", RequireLinear: true})
		c := mustChecks(t, e)
		if !c.Mergeable || !slices.Equal(c.Methods, []string{domain.MergeSquash}) {
			t.Fatalf("checks = %+v", c)
		}
		_, err := e.svc.MergeWith(ctx, carol, e.tree, 1, MergeOptions{Method: domain.MergeFastForward})
		blocked(t, err, CodeMergeBlocked)
		mr, err := e.svc.Merge(ctx, carol, e.tree, 1)
		if err != nil || mr.MergeMethod != domain.MergeSquash {
			t.Fatalf("default = %+v, %v", mr, err)
		}
		eq(t, e.files(t, mr.MergedSnapshot), e.files(t, m))
	})

	t.Run("require signed", func(t *testing.T) {
		e := newMergeEnv(t)
		e.diverged(t)
		e.protect(t, domain.BranchProtection{Pattern: "main", RequireSigned: true})
		mb, err := e.svc.Mergeability(ctx, eve, e.tree, 1)
		if err != nil {
			t.Fatal(err)
		}
		if !slices.Equal(mb.Allowed, []string{domain.MergeFastForward}) || len(mb.Methods) != 0 ||
			!strings.Contains(mb.Reasons[domain.MergeSquash], "signed") {
			t.Fatalf("mergeability = %+v", mb)
		}
	})
}

func mustChecks(t *testing.T, e *menv) *MergeChecks {
	t.Helper()
	c, err := e.svc.Checks(ctx, eve, e.tree, 1)
	if err != nil {
		t.Fatal(err)
	}
	return c
}

func TestServerMergeEdgeCases(t *testing.T) {
	t.Run("invalid options", func(t *testing.T) {
		e := newMergeEnv(t)
		e.diverged(t)
		for _, opts := range []MergeOptions{{Method: "rebase"}, {Message: strings.Repeat("x", MaxMergeMessage+1)}, {Message: "a\x00b"}} {
			if _, err := e.svc.MergeWith(ctx, carol, e.tree, 1, opts); !errors.Is(err, domain.ErrInvalid) {
				t.Fatalf("%+v: err = %v", opts.Method, err)
			}
		}
		if _, err := e.svc.MergeGroupWith(ctx, carol, e.tree.TenantID, "g", "octopus"); !errors.Is(err, domain.ErrInvalid) {
			t.Fatalf("group: err = %v", err)
		}
	})

	t.Run("custom message", func(t *testing.T) {
		e := newMergeEnv(t)
		e.diverged(t)
		mr, err := e.svc.MergeWith(ctx, carol, e.tree, 1, MergeOptions{Message: "Merge it\n\nbody"})
		if err != nil {
			t.Fatal(err)
		}
		snap, _ := e.store.GetSnapshot(ctx, mr.MergedSnapshot)
		if snap.Message != "Merge it\n\nbody" {
			t.Fatalf("message = %q", snap.Message)
		}
	})

	t.Run("up to date", func(t *testing.T) {
		e := newMergeEnv(t)
		base := e.commit(t, map[string]string{"a.txt": "a\n"})
		ahead := e.commit(t, map[string]string{"a.txt": "A\n"}, base)
		e.branch(t, "main", ahead)
		e.branch(t, "feature", base)
		e.open(t, "feature")
		mb, _ := e.svc.Mergeability(ctx, eve, e.tree, 1)
		if !mb.UpToDate || !slices.Equal(mb.Methods, AllMergeMethods) {
			t.Fatalf("mergeability = %+v", mb)
		}
		mr, err := e.svc.MergeWith(ctx, carol, e.tree, 1, MergeOptions{Method: domain.MergeSquash})
		if err != nil || mr.MergeMethod != domain.MergeSquash || mr.MergedSnapshot != base || e.tip(t, "main") != ahead {
			t.Fatalf("mr = %+v, %v", mr, err)
		}
	})

	t.Run("empty target", func(t *testing.T) {
		e := newMergeEnv(t)
		feat := e.commit(t, map[string]string{"a.txt": "a\n"})
		e.branch(t, "main", feat)
		e.branch(t, "feature", feat)
		e.open(t, "feature")
		e.branch(t, "main", "")
		if _, err := e.svc.MergeWith(ctx, carol, e.tree, 1, MergeOptions{Method: domain.MergeCommit}); !errors.Is(err, domain.ErrConflict) ||
			!strings.Contains(err.Error(), "is empty") {
			t.Fatalf("merge into empty: %v", err)
		}
		mr, err := e.svc.Merge(ctx, carol, e.tree, 1)
		if err != nil || mr.MergeMethod != domain.MergeFastForward || e.tip(t, "main") != feat {
			t.Fatalf("mr = %+v, %v", mr, err)
		}
	})

	t.Run("target moves during the merge", func(t *testing.T) {
		e := newMergeEnv(t)
		_, mainTip, _ := e.diverged(t)
		other := e.commit(t, map[string]string{"z": "z\n"}, mainTip)
		e.store.beforeCAS = func() { e.branch(t, "main", other) }
		if _, err := e.svc.Merge(ctx, carol, e.tree, 1); !errors.Is(err, domain.ErrConflict) || !strings.Contains(err.Error(), "moved during the merge") {
			t.Fatalf("err = %v", err)
		}
		e.store.beforeCAS = nil
		if mr, _ := e.store.GetMergeRequest(ctx, e.tree.ID, 1); mr.State != domain.MRStateOpen {
			t.Fatal("merge request changed state")
		}
	})

	t.Run("unrelated histories", func(t *testing.T) {
		e := newMergeEnv(t)
		a := e.commit(t, map[string]string{"a.txt": "a\n", "same": "s\n"})
		b := e.commit(t, map[string]string{"b.txt": "b\n", "same": "s\n"})
		e.branch(t, "main", a)
		e.branch(t, "feature", b)
		e.open(t, "feature")
		mr, err := e.svc.Merge(ctx, carol, e.tree, 1)
		if err != nil {
			t.Fatal(err)
		}
		eq(t, e.files(t, mr.MergedSnapshot), map[string]string{"a.txt": "a\n", "b.txt": "b\n", "same": "s\n"})
	})

	t.Run("executable bit merges", func(t *testing.T) {
		e := newMergeEnv(t)
		base := e.commit(t, map[string]string{"run.sh": "echo 1\n", "x": "x\n"})
		mainTip := e.commit(t, map[string]string{"run.sh*": "echo 1\n", "x": "X\n"}, base)
		feat := e.commit(t, map[string]string{"run.sh": "echo 2\n", "x": "x\n"}, base)
		e.branch(t, "main", mainTip)
		e.branch(t, "feature", feat)
		e.open(t, "feature")
		mr, err := e.svc.Merge(ctx, carol, e.tree, 1)
		if err != nil {
			t.Fatal(err)
		}
		eq(t, e.files(t, mr.MergedSnapshot), map[string]string{"run.sh*": "echo 2\n", "x": "X\n"})
	})
}

func TestServerMergeLinkedGroup(t *testing.T) {
	setup := func(t *testing.T, conflict bool) (*menv, *domain.Tree, *domain.LinkedGroup, [2]string) {
		e := newMergeEnv(t)
		e.diverged(t) // proj!1, mergeable with merge/squash
		t2 := &domain.Tree{ID: "tree-2", TenantID: "ten-acme", Name: "api", DefaultBranch: "main"}
		if err := e.store.CreateTree(ctx, t2); err != nil {
			t.Fatal(err)
		}
		saved := e.tree
		e.tree = t2
		base := e.commit(t, map[string]string{"x.txt": fiveLines})
		mainTip := e.commit(t, map[string]string{"x.txt": "1\nmain\n3\n4\n5\n"}, base)
		featContent := "1\n2\n3\n4\nfeature\n"
		if conflict {
			featContent = "1\nfeature\n3\n4\n5\n"
		}
		feat := e.commit(t, map[string]string{"x.txt": featContent}, base)
		e.branch(t, "main", mainTip)
		e.branch(t, "feature", feat)
		e.open(t, "feature")
		e.tree = saved
		g, err := e.svc.CreateGroup(ctx, carol, "ten-acme", "release", []GroupMember{{e.tree, 1}, {t2, 1}})
		if err != nil {
			t.Fatal(err)
		}
		return e, t2, g, [2]string{e.tip(t, "main"), mainTip}
	}

	t.Run("conflicts refuse the whole group", func(t *testing.T) {
		e, t2, g, tips := setup(t, true)
		_, err := e.svc.MergeGroupWith(ctx, carol, "ten-acme", g.ID, domain.MergeCommit)
		var ce *ConflictError
		if !errors.As(err, &ce) || len(ce.Conflicts) != 1 || ce.Conflicts[0] != (Conflict{Tree: "acme/api", Path: "x.txt", Kind: merge3.KindContent}) {
			t.Fatalf("err = %v", err)
		}
		if e.tip(t, "main") != tips[0] {
			t.Fatal("proj main moved")
		}
		b, _ := e.store.GetBranch(ctx, t2.ID, "main")
		if b.Tip != tips[1] {
			t.Fatal("api main moved")
		}
	})

	t.Run("merges every member with the method", func(t *testing.T) {
		e, t2, g, tips := setup(t, false)
		if _, err := e.svc.MergeGroupWith(ctx, carol, "ten-acme", g.ID, domain.MergeFastForward); err == nil {
			t.Fatal("fast-forward of diverged members succeeded")
		}
		merged, err := e.svc.MergeGroupWith(ctx, carol, "ten-acme", g.ID, domain.MergeSquash)
		if err != nil {
			t.Fatal(err)
		}
		if merged.State != domain.MRStateMerged {
			t.Fatalf("group = %+v", merged)
		}
		for i, tr := range []*domain.Tree{e.tree, t2} {
			mr, _ := e.store.GetMergeRequest(ctx, tr.ID, 1)
			b, _ := e.store.GetBranch(ctx, tr.ID, "main")
			snap, err := e.store.GetSnapshot(ctx, b.Tip)
			if err != nil || mr.MergeMethod != domain.MergeSquash || mr.MergedSnapshot != b.Tip || !slices.Equal(snap.Parents, []string{tips[i]}) {
				t.Fatalf("%s: mr %+v snapshot %+v (%v)", tr.Name, mr, snap, err)
			}
		}
	})

	t.Run("rollback restores merge snapshots", func(t *testing.T) {
		e, t2, g, tips := setup(t, false)
		calls := 0
		e.store.beforeCAS = func() {
			calls++
			if calls == 2 { // the second member's target moves concurrently
				_ = e.store.Memory.CompareAndSwapBranchTip(ctx, t2.ID, "main", tips[1], "c01")
			}
		}
		if _, err := e.svc.MergeGroup(ctx, carol, "ten-acme", g.ID); !errors.Is(err, domain.ErrConflict) {
			t.Fatalf("err = %v", err)
		}
		e.store.beforeCAS = nil
		if e.tip(t, "main") != tips[0] {
			t.Fatalf("proj main = %s, want rollback to %s", e.tip(t, "main"), tips[0])
		}
	})
}

func eq(t *testing.T, got, want map[string]string) {
	t.Helper()
	if len(got) != len(want) {
		t.Fatalf("got %v, want %v", got, want)
	}
	for k, v := range want {
		if got[k] != v {
			t.Fatalf("got %v, want %v", got, want)
		}
	}
}
