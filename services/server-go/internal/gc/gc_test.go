package gc

import (
	"context"
	"errors"
	"log/slog"
	"sort"
	"sync"
	"testing"
	"time"

	"google.golang.org/protobuf/proto"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/limits"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
	"github.com/seanfilimon/worktree/services/server-go/internal/objstore"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
)

var quiet = slog.New(slog.DiscardHandler)

var t0 = time.Date(2026, 6, 1, 12, 0, 0, 0, time.UTC)

// clock is a settable fake clock.
type clock struct {
	mu  sync.Mutex
	now time.Time
}

func (c *clock) Now() time.Time      { c.mu.Lock(); defer c.mu.Unlock(); return c.now }
func (c *clock) Set(t time.Time)     { c.mu.Lock(); c.now = t; c.mu.Unlock() }
func (c *clock) Add(d time.Duration) { c.Set(c.Now().Add(d)) }

type fixture struct {
	t     *testing.T
	ctx   context.Context
	store domain.MetaStore
	objs  *objstore.DiskStore
	clk   *clock
	c     *Collector
	tree  *domain.Tree
	seq   int
}

func newFixture(t *testing.T) *fixture {
	t.Helper()
	f := &fixture{t: t, ctx: context.Background(), store: meta.NewMemory(), objs: objstore.New(t.TempDir()), clk: &clock{now: t0}}
	t.Cleanup(func() { _ = f.store.Close() })
	tenant := &domain.Tenant{ID: "ten-1", Slug: "acme", Name: "Acme", Kind: domain.TenantOrg}
	must(t, f.store.CreateTenant(f.ctx, tenant))
	f.tree = &domain.Tree{ID: "tree-1", TenantID: tenant.ID, Name: "web", DefaultBranch: "main", Visibility: domain.VisibilityPrivate}
	must(t, f.store.CreateTree(f.ctx, f.tree))
	f.c = New(Deps{Logger: quiet, Store: f.store, Objects: f.objs, Grace: 720 * time.Hour, StagedRetention: 168 * time.Hour,
		AuditRetention: 90 * 24 * time.Hour, Now: f.clk.Now})
	return f
}

func must(t *testing.T, err error) {
	t.Helper()
	if err != nil {
		t.Fatal(err)
	}
}

func (f *fixture) blob(data string) hash.Hash {
	f.t.Helper()
	h := hash.Sum([]byte(data))
	must(f.t, f.objs.Put(f.ctx, h, []byte(data)))
	return h
}

func ch(h hash.Hash) *syncpb.ContentHash { return &syncpb.ContentHash{Value: h[:]} }

// chunked stores a chunk list over the given chunk contents.
func (f *fixture) chunked(parts ...string) hash.Hash {
	f.t.Helper()
	cl := &syncpb.ChunkList{}
	for _, p := range parts {
		h := f.blob(p)
		cl.Chunks = append(cl.Chunks, &syncpb.Chunk{Hash: ch(h), Size: uint64(len(p))})
		cl.Size += uint64(len(p))
	}
	raw, err := proto.Marshal(cl)
	must(f.t, err)
	h := hash.Sum(raw)
	must(f.t, f.objs.Put(f.ctx, h, raw))
	return h
}

type entry struct {
	path    string
	h       hash.Hash
	chunked bool
}

func (f *fixture) manifest(entries ...entry) hash.Hash {
	f.t.Helper()
	m := &syncpb.ManifestRecord{}
	for _, e := range entries {
		m.Entries = append(m.Entries, &syncpb.ManifestEntry{Path: e.path, BlobHash: ch(e.h), Chunked: e.chunked})
	}
	raw, err := proto.Marshal(m)
	must(f.t, err)
	h := hash.Sum(raw)
	must(f.t, f.objs.Put(f.ctx, h, raw))
	return h
}

func (f *fixture) snapshot(manifest hash.Hash, parents ...string) string {
	f.t.Helper()
	f.seq++
	id := hash.Sum([]byte{byte(f.seq), 0xAB}).Hex()
	must(f.t, f.store.PutSnapshot(f.ctx, &domain.Snapshot{ID: id, TreeID: f.tree.ID, Parents: parents, ManifestHash: manifest,
		Author: "alice", Message: "m", CreatedAt: t0}))
	return id
}

func (f *fixture) branch(name, tip string) {
	f.t.Helper()
	must(f.t, f.store.PutBranch(f.ctx, &domain.Branch{TreeID: f.tree.ID, Name: name, Tip: tip}))
}

func (f *fixture) has(h hash.Hash) bool {
	f.t.Helper()
	ok, err := f.objs.Has(f.ctx, h)
	must(f.t, err)
	return ok
}

func (f *fixture) run(dry bool) *domain.GCReport {
	f.t.Helper()
	rep, err := f.c.Run(f.ctx, dry)
	must(f.t, err)
	return rep
}

func (f *fixture) marks() int {
	f.t.Helper()
	raws, err := f.store.ListDocs(f.ctx, CollMarks, "")
	must(f.t, err)
	return len(raws)
}

func TestMarkAndSweep(t *testing.T) {
	f := newFixture(t)
	ctx := f.ctx

	// main: s1 <- s2. s2 adds a chunked file.
	a1 := f.blob("a v1")
	m1 := f.manifest(entry{"a.txt", a1, false})
	s1 := f.snapshot(m1)
	a2 := f.blob("a v2")
	cl := f.chunked("chunk-one", "chunk-two")
	m2 := f.manifest(entry{"a.txt", a2, false}, entry{"big.bin", cl, true})
	s2 := f.snapshot(m2, s1)
	f.branch("main", s2)
	// A tag on an otherwise unreferenced snapshot.
	tagBlob := f.blob("tagged")
	sTag := f.snapshot(f.manifest(entry{"t", tagBlob, false}))
	must(t, f.store.PutTag(ctx, &domain.Tag{TreeID: f.tree.ID, Name: "v1", Target: sTag, Tagger: "alice"}))
	// A merged MR's merged snapshot.
	mrBlob := f.blob("merged")
	sMR := f.snapshot(f.manifest(entry{"mr", mrBlob, false}))
	must(t, f.store.CreateMergeRequest(ctx, &domain.MergeRequest{TreeID: f.tree.ID, Title: "x", SourceBranch: "gone",
		TargetBranch: "main", AuthorID: "u", Author: "alice", State: domain.MRStateMerged, MergedSnapshot: sMR}))
	// A release with an asset.
	asset := f.blob("release asset bytes")
	relBlob := f.blob("release content")
	sRel := f.snapshot(f.manifest(entry{"r", relBlob, false}))
	must(t, f.store.PutDoc(ctx, collReleases, domain.DocKey(f.tree.ID, "v1"), domain.Release{ID: "r1", TreeID: f.tree.ID, Tag: "v1",
		SnapshotID: sRel, Assets: []domain.ReleaseAsset{{Name: "a.zip", Hash: asset, Size: 19}}}))
	// Staged snapshots: one fresh, one expired.
	freshBlob := f.blob("fresh wip")
	sFresh := f.snapshot(f.manifest(entry{"w", freshBlob, false}))
	must(t, f.store.PutStaged(ctx, &domain.StagedSnapshot{SnapshotID: sFresh, TreeID: f.tree.ID, TenantID: f.tree.TenantID,
		AccountID: "acct-a", BranchName: "main", StagedAt: t0.Add(-time.Hour)}))
	oldBlob := f.blob("old wip")
	oldManifest := f.manifest(entry{"o", oldBlob, false})
	sOld := f.snapshot(oldManifest)
	must(t, f.store.PutStaged(ctx, &domain.StagedSnapshot{SnapshotID: sOld, TreeID: f.tree.ID, TenantID: f.tree.TenantID,
		AccountID: "acct-b", BranchName: "main", StagedAt: t0.Add(-200 * time.Hour)}))
	// Garbage: an orphan blob and an orphan snapshot (record + manifest + blob).
	orphan := f.blob("orphan")
	orphanSnapBlob := f.blob("orphan snapshot blob")
	orphanManifest := f.manifest(entry{"x", orphanSnapBlob, false})
	f.snapshot(orphanManifest)
	// Audit events: one past retention, one recent.
	must(t, f.store.AppendAudit(ctx, &domain.AuditEvent{Action: "old.event", At: t0.Add(-100 * 24 * time.Hour)}))
	must(t, f.store.AppendAudit(ctx, &domain.AuditEvent{Action: "new.event", At: t0.Add(-time.Hour)}))

	reachable := []hash.Hash{a1, m1, a2, cl, m2, tagBlob, mrBlob, asset, relBlob, freshBlob}
	// Unreachable after the expired staged entry is dropped: old wip blob +
	// its manifest, the orphan blob, the orphan snapshot's blob + manifest.
	const wantUnreachable = 5

	// 1. Dry run: reports, changes nothing.
	rep := f.run(true)
	if !rep.DryRun || rep.Unreachable != wantUnreachable || rep.Marked != wantUnreachable || rep.Deleted != 0 ||
		rep.StagedDeleted != 1 || rep.AuditPruned != 1 || rep.Scanned != rep.Reachable+rep.Unreachable {
		t.Fatalf("dry run report = %+v", rep)
	}
	if f.marks() != 0 {
		t.Fatal("dry run wrote marks")
	}
	if st, _ := f.store.ListStaged(ctx, f.tree.ID, ""); len(st) != 2 {
		t.Fatalf("dry run deleted staged entries: %d left", len(st))
	}
	if evs, _ := f.store.ListAudit(ctx, domain.AuditFilter{}); len(evs) != 2 {
		t.Fatalf("dry run pruned audit: %d left", len(evs))
	}

	// 2. Live run: marks, deletes nothing yet, applies retention.
	f.clk.Add(time.Second)
	rep = f.run(false)
	if rep.Marked != wantUnreachable || rep.Deleted != 0 || rep.StagedDeleted != 1 || rep.AuditPruned != 1 {
		t.Fatalf("first live run = %+v", rep)
	}
	if f.marks() != wantUnreachable {
		t.Fatalf("marks = %d", f.marks())
	}
	if st, _ := f.store.ListStaged(ctx, f.tree.ID, ""); len(st) != 1 || st[0].SnapshotID != sFresh {
		t.Fatalf("staged after retention = %+v", st)
	}
	if evs, _ := f.store.ListAudit(ctx, domain.AuditFilter{}); len(evs) != 1 || evs[0].Action != "new.event" {
		t.Fatalf("audit after retention = %+v", evs)
	}
	// Exact per-tenant usage = every stored object but the unreachable ones
	// (chunks and the manifests of tagged/staged/released snapshots count).
	_, want, err := f.objs.Stats(ctx)
	must(t, err)
	for _, h := range []hash.Hash{oldBlob, oldManifest, orphan, orphanSnapBlob, orphanManifest} {
		n, err := f.objs.Size(ctx, h)
		must(t, err)
		want -= n
	}
	cls, err := f.objs.Get(ctx, cl)
	must(t, err)
	var list syncpb.ChunkList
	must(t, proto.Unmarshal(cls, &list))
	if got, err := limits.StorageBytes(ctx, f.store, f.tree.TenantID); err != nil || got != want {
		t.Fatalf("tenant usage = %d, %v; want %d", got, err, want)
	}

	// 3. The orphan blob becomes reachable again before the grace ends.
	f.clk.Add(500 * time.Hour)
	f.branch("rescue", f.snapshot(f.manifest(entry{"orphan", orphan, false})))
	restage := func() {
		must(t, f.store.PutStaged(ctx, &domain.StagedSnapshot{SnapshotID: sFresh, TreeID: f.tree.ID, TenantID: f.tree.TenantID,
			AccountID: "acct-a", BranchName: "main", StagedAt: f.clk.Now()}))
	}
	restage() // keep the fresh WIP within its retention
	rep = f.run(false)
	if rep.Deleted != 0 || rep.Marked != 0 {
		t.Fatalf("run within grace = %+v", rep)
	}
	if f.marks() != wantUnreachable-1 {
		t.Fatalf("orphan was not unmarked: %d marks", f.marks())
	}

	// 4. Past the grace period the still-unreachable objects are deleted.
	f.clk.Add(300 * time.Hour)
	restage()
	rep = f.run(false)
	if rep.Deleted != wantUnreachable-1 || rep.FreedBytes <= 0 || rep.Unreachable != wantUnreachable-1 {
		t.Fatalf("sweep after grace = %+v", rep)
	}
	if f.has(oldBlob) || f.has(orphanSnapBlob) {
		t.Fatal("unreachable objects survived the sweep")
	}
	for _, h := range append(reachable, orphan) {
		if !f.has(h) {
			t.Fatalf("reachable object %s was deleted", h)
		}
	}
	for _, c := range list.Chunks {
		var h hash.Hash
		copy(h[:], c.Hash.Value)
		if !f.has(h) {
			t.Fatal("chunk of a reachable chunked file was deleted")
		}
	}
	if f.marks() != 0 {
		t.Fatalf("marks left after sweep: %d", f.marks())
	}

	// 5. Runs are listed newest first.
	runs, err := f.c.Runs(ctx)
	must(t, err)
	if len(runs) != 4 || !runs[0].StartedAt.After(runs[1].StartedAt) || runs[3].DryRun != true {
		t.Fatalf("runs = %+v", runs)
	}
}

func TestDryRunPastGraceDeletesNothing(t *testing.T) {
	f := newFixture(t)
	orphan := f.blob("orphan")
	f.run(false) // mark
	f.clk.Add(721 * time.Hour)
	rep := f.run(true)
	if rep.Deleted != 1 || rep.FreedBytes != int64(len("orphan")) {
		t.Fatalf("dry run past grace = %+v", rep)
	}
	if !f.has(orphan) || f.marks() != 1 {
		t.Fatal("dry run deleted or unmarked an object")
	}
}

func TestRetentionDisabled(t *testing.T) {
	f := newFixture(t)
	f.c = New(Deps{Logger: quiet, Store: f.store, Objects: f.objs, Grace: time.Hour, Now: f.clk.Now})
	must(t, f.store.AppendAudit(f.ctx, &domain.AuditEvent{Action: "ancient", At: t0.Add(-1000 * 24 * time.Hour)}))
	must(t, f.store.PutStaged(f.ctx, &domain.StagedSnapshot{SnapshotID: "x", TreeID: f.tree.ID, AccountID: "a",
		BranchName: "main", StagedAt: t0.Add(-1000 * time.Hour)}))
	rep := f.run(false)
	if rep.AuditPruned != 0 || rep.StagedDeleted != 0 {
		t.Fatalf("retention with 0 = %+v", rep)
	}
}

func TestKeepsLastRuns(t *testing.T) {
	f := newFixture(t)
	for i := 0; i < KeepRuns+5; i++ {
		f.clk.Add(time.Second)
		f.run(true)
	}
	runs, err := f.c.Runs(f.ctx)
	must(t, err)
	if len(runs) != KeepRuns {
		t.Fatalf("kept %d runs, want %d", len(runs), KeepRuns)
	}
	if !runs[0].StartedAt.Equal(f.clk.Now()) {
		t.Fatalf("newest run = %v, want %v", runs[0].StartedAt, f.clk.Now())
	}
	if !sort.SliceIsSorted(runs, func(i, j int) bool { return runs[i].StartedAt.After(runs[j].StartedAt) }) {
		t.Fatal("runs not newest first")
	}
}

func TestOneRunAtATime(t *testing.T) {
	f := newFixture(t)
	f.c.run.Lock()
	_, err := f.c.Run(f.ctx, false)
	f.c.run.Unlock()
	if !errors.Is(err, ErrRunning) {
		t.Fatalf("err = %v, want ErrRunning", err)
	}
}

// A storage error during marking aborts the run before anything is deleted.
func TestMarkErrorAbortsSweep(t *testing.T) {
	f := newFixture(t)
	orphan := f.blob("orphan")
	f.run(false)
	f.clk.Add(1000 * time.Hour)
	f.branch("main", "missing-but-error")
	f.c.d.Store = errSnapshots{f.store}
	rep, err := f.c.Run(f.ctx, false)
	if err == nil || rep == nil || rep.Error == "" || rep.Deleted != 0 {
		t.Fatalf("rep = %+v, err = %v", rep, err)
	}
	if !f.has(orphan) {
		t.Fatal("object deleted although marking failed")
	}
}

type errSnapshots struct{ domain.MetaStore }

func (errSnapshots) GetSnapshot(context.Context, string) (*domain.Snapshot, error) {
	return nil, errors.New("disk on fire")
}

func TestTickerRunsAndStops(t *testing.T) {
	f := newFixture(t)
	f.c = New(Deps{Logger: quiet, Store: f.store, Objects: f.objs, Grace: time.Hour, Interval: 10 * time.Millisecond})
	stop := f.c.Start(f.ctx)
	deadline := time.Now().Add(5 * time.Second)
	for {
		runs, err := f.c.Runs(f.ctx)
		must(t, err)
		if len(runs) > 0 {
			break
		}
		if time.Now().After(deadline) {
			t.Fatal("ticker never ran")
		}
		time.Sleep(5 * time.Millisecond)
	}
	if f.c.NextRun().IsZero() {
		t.Fatal("NextRun not reported while the ticker runs")
	}
	stop()
	if !f.c.NextRun().IsZero() {
		t.Fatal("NextRun still set after stop")
	}
	// Interval 0 disables the ticker.
	stop = New(Deps{Logger: quiet, Store: f.store}).Start(f.ctx)
	stop()
}

// TestStagedManifestIsARoot: the objects of a staged working manifest are
// reachable although no snapshot record references them.
func TestStagedManifestIsARoot(t *testing.T) {
	f := newFixture(t)
	wip := f.blob("work in progress")
	m := f.manifest(entry{path: "wip.txt", h: wip})
	orphan := f.blob("orphan")
	must(t, f.store.PutStaged(f.ctx, &domain.StagedSnapshot{SnapshotID: "wip-1", TreeID: f.tree.ID, TenantID: f.tree.TenantID,
		AccountID: "a", BranchName: "main", StagedAt: t0, ManifestHash: m.Hex()}))
	rep := f.run(false)
	if rep.Marked != 1 || f.marks() != 1 {
		t.Fatalf("marked = %d (docs %d), want only the orphan", rep.Marked, f.marks())
	}
	if !f.has(wip) || !f.has(m) || !f.has(orphan) {
		t.Fatal("objects deleted before the grace period")
	}
}
