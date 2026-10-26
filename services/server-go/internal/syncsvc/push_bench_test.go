package syncsvc

// Benchmarks of push validation (hardening, docs/ENGINEERING_PLAN.md
// §15.10): manifest decoding, object checks, manifest diffing and the whole
// Push RPC over in-process fakes. Run with
//
//	go test ./internal/syncsvc -run '^$' -bench . -benchmem
//
// Sizes model a large tree: 10k manifest entries, 10 changed per push.

import (
	"context"
	"fmt"
	"io"
	"log/slog"
	"sync"
	"testing"
	"time"

	"google.golang.org/protobuf/proto"
	"google.golang.org/protobuf/types/known/timestamppb"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/iam"
	"github.com/seanfilimon/worktree/services/server-go/internal/objstore"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
)

const (
	benchEntries = 10_000
	benchChanged = 10
)

// memObjects is a map-backed objstore.Store (no disk I/O), so the
// benchmarks measure the validation logic rather than the file system.
type memObjects struct {
	mu   sync.RWMutex
	objs map[hash.Hash][]byte
}

func newMemObjects() *memObjects { return &memObjects{objs: map[hash.Hash][]byte{}} }

func (m *memObjects) Put(_ context.Context, h hash.Hash, data []byte) error {
	m.mu.Lock()
	defer m.mu.Unlock()
	m.objs[h] = data
	return nil
}

func (m *memObjects) Get(_ context.Context, h hash.Hash) ([]byte, error) {
	m.mu.RLock()
	defer m.mu.RUnlock()
	d, ok := m.objs[h]
	if !ok {
		return nil, objstore.ErrNotFound
	}
	return d, nil
}

func (m *memObjects) Has(_ context.Context, h hash.Hash) (bool, error) {
	m.mu.RLock()
	defer m.mu.RUnlock()
	_, ok := m.objs[h]
	return ok, nil
}

func (m *memObjects) Delete(_ context.Context, h hash.Hash) error {
	m.mu.Lock()
	defer m.mu.Unlock()
	delete(m.objs, h)
	return nil
}

// benchManifest builds a sorted manifest of n entries spread over nested
// directories; variant changes the content of the first `changed` files.
func benchManifest(n, changed, variant int) (*syncpb.ManifestRecord, map[hash.Hash][]byte) {
	m := &syncpb.ManifestRecord{Entries: make([]*syncpb.ManifestEntry, 0, n)}
	blobs := make(map[hash.Hash][]byte, n)
	for i := 0; i < n; i++ {
		p := fmt.Sprintf("src/pkg%03d/sub%02d/file%05d.go", i/100, (i/10)%10, i)
		body := fmt.Sprintf("package pkg%03d\n\n// file %d\n", i/100, i)
		if i < changed {
			body += fmt.Sprintf("// variant %d\n", variant)
		}
		data := []byte(body)
		h := hash.Sum(data)
		blobs[h] = data
		m.Entries = append(m.Entries, &syncpb.ManifestEntry{Path: p, BlobHash: hashToPB(h), Size: uint64(len(data))})
	}
	return m, blobs
}

func marshalManifest(tb testing.TB, m *syncpb.ManifestRecord) []byte {
	tb.Helper()
	data, err := proto.MarshalOptions{Deterministic: true}.Marshal(m)
	if err != nil {
		tb.Fatal(err)
	}
	return data
}

func BenchmarkDecodeManifest(b *testing.B) {
	for _, n := range []int{1_000, benchEntries} {
		m, _ := benchManifest(n, 0, 0)
		data := marshalManifest(b, m)
		b.Run(fmt.Sprintf("entries=%d", n), func(b *testing.B) {
			b.SetBytes(int64(len(data)))
			b.ReportAllocs()
			for i := 0; i < b.N; i++ {
				if _, err := decodeManifest(data); err != nil {
					b.Fatal(err)
				}
			}
		})
	}
}

func BenchmarkDiffManifests(b *testing.B) {
	for _, changed := range []int{benchChanged, benchEntries} {
		a, _ := benchManifest(benchEntries, changed, 1)
		c, _ := benchManifest(benchEntries, changed, 2)
		b.Run(fmt.Sprintf("entries=%d/changed=%d", benchEntries, changed), func(b *testing.B) {
			b.ReportAllocs()
			for i := 0; i < b.N; i++ {
				if got := diffManifests(a, c); len(got) != changed {
					b.Fatalf("diff = %d paths, want %d", len(got), changed)
				}
			}
		})
	}
}

// pushBench is a Service over fakes with a tree whose main branch holds a
// 10k-entry snapshot, and two manifests (A, B) that each differ from the
// other in benchChanged files.
type pushBench struct {
	svc      *Service
	ctx      context.Context
	treeID   string
	manifest [2]hash.Hash
	tip      string
	clock    time.Time
}

func newPushBench(b *testing.B, objects objstore.Store) *pushBench {
	b.Helper()
	store := newFakeStore()
	store.tenants[tenantAcme] = &domain.Tenant{ID: tenantAcme, Slug: "acme", Name: "Acme", Kind: domain.TenantOrg}
	az := &fakeAuthz{}
	az.setRole(tenantAcme, alice.AccountID, domain.RoleOwner)
	svc := New(Deps{
		Store:   store,
		Objects: objects,
		History: fakeHistory{st: store},
		Authz:   az,
		Review:  &fakeReview{},
		License: &fakeLicense{},
		Staged:  &fakeStaged{},
		Bus:     &fakeBus{},
		Audit:   &fakeAudit{},
		Version: "bench",
	})
	svc.log = slog.New(slog.NewTextHandler(io.Discard, nil))
	ctx := iam.WithPrincipal(context.Background(), alice)
	info, err := svc.ResolveTree(ctx, &syncpb.ResolveTreeRequest{TenantSlug: "acme", TreeName: "big", CreateIfMissing: true})
	if err != nil {
		b.Fatal(err)
	}
	pb := &pushBench{svc: svc, ctx: ctx, treeID: info.GetTreeId().GetValue(), clock: commitClock}
	for v := 0; v < 2; v++ {
		m, blobs := benchManifest(benchEntries, benchChanged, v)
		for h, d := range blobs {
			if err := objects.Put(ctx, h, d); err != nil {
				b.Fatal(err)
			}
		}
		data := marshalManifest(b, m)
		mh := hash.Sum(data)
		if err := objects.Put(ctx, mh, data); err != nil {
			b.Fatal(err)
		}
		pb.manifest[v] = mh
	}
	pb.push(b, 0) // the initial tip
	return pb
}

// push pushes one new snapshot (manifest variant v) on top of the tip.
func (pb *pushBench) push(b *testing.B, v int) {
	pb.clock = pb.clock.Add(time.Second)
	id := newID()
	rec := &syncpb.SnapshotRecord{
		Id:           &syncpb.SnapshotId{Value: id},
		TreeId:       &syncpb.TreeId{Value: pb.treeID},
		BranchName:   "main",
		ManifestHash: hashToPB(pb.manifest[v]),
		Author:       "alice",
		Message:      "bench",
		CreatedAt:    timestamppb.New(pb.clock),
	}
	if pb.tip != "" {
		rec.Parents = []*syncpb.SnapshotId{{Value: pb.tip}}
	}
	req := &syncpb.PushRequest{
		TreeId:        &syncpb.TreeId{Value: pb.treeID},
		BranchName:    "main",
		OldTip:        snapshotIDToPB(pb.tip),
		NewTip:        &syncpb.SnapshotId{Value: id},
		SnapshotChain: []*syncpb.SnapshotId{{Value: id}},
		Snapshots:     []*syncpb.SnapshotRecord{rec},
	}
	resp, err := pb.svc.Push(pb.ctx, req)
	if err != nil {
		b.Fatal(err)
	}
	if !resp.GetAccepted() {
		b.Fatalf("push rejected: %v", resp.GetRejectionReason())
	}
	pb.tip = id
}

// BenchmarkPush measures the Push RPC (authorization, snapshot planning,
// object checks of every manifest entry, changed-path diff and persisting)
// of one snapshot whose 10k-entry manifest changes 10 files.
func BenchmarkPush(b *testing.B) {
	run := func(b *testing.B, objects objstore.Store) {
		pb := newPushBench(b, objects)
		b.ReportAllocs()
		b.ResetTimer()
		for i := 0; i < b.N; i++ {
			pb.push(b, (i+1)%2)
		}
	}
	b.Run("store=memory", func(b *testing.B) { run(b, newMemObjects()) })
	b.Run("store=disk", func(b *testing.B) { run(b, objstore.New(b.TempDir())) })
}

// BenchmarkCheckObjects measures step 5 of Push alone: decoding the new
// snapshot's manifest and checking that each of its blobs is stored, on
// top of the branch tip (parent=tip: unchanged blobs are trusted) and
// without a trusted parent (parent=none: every blob is checked, as for a
// first push).
func BenchmarkCheckObjects(b *testing.B) {
	run := func(b *testing.B, objects objstore.Store, onTip bool) {
		pb := newPushBench(b, objects)
		tree, err := pb.svc.d.Store.GetTree(pb.ctx, pb.treeID)
		if err != nil {
			b.Fatal(err)
		}
		snap := &domain.Snapshot{ID: newID(), TreeID: pb.treeID, ManifestHash: pb.manifest[1]}
		var frontier []string
		if onTip {
			snap.Parents, frontier = []string{pb.tip}, []string{pb.tip}
		}
		b.ReportAllocs()
		b.ResetTimer()
		for i := 0; i < b.N; i++ {
			pl := &pushPlan{
				tree:      tree,
				newTip:    snap.ID,
				records:   map[string]*domain.Snapshot{snap.ID: snap},
				newSnaps:  []*domain.Snapshot{snap},
				frontier:  frontier,
				manifests: map[hash.Hash]*syncpb.ManifestRecord{},
			}
			if err := pb.svc.checkObjects(pb.ctx, pl); err != nil {
				b.Fatal(err)
			}
		}
	}
	for _, onTip := range []bool{true, false} {
		parent := map[bool]string{true: "tip", false: "none"}[onTip]
		b.Run("store=memory/parent="+parent, func(b *testing.B) { run(b, newMemObjects(), onTip) })
		b.Run("store=disk/parent="+parent, func(b *testing.B) { run(b, objstore.New(b.TempDir()), onTip) })
	}
}

// BenchmarkNegotiate measures the pre-upload negotiation of a push: the
// client offers every object of the new snapshot (10k blobs + manifest).
func BenchmarkNegotiate(b *testing.B) {
	run := func(b *testing.B, objects objstore.Store) {
		pb := newPushBench(b, objects)
		m, _ := benchManifest(benchEntries, benchChanged, 1)
		req := &syncpb.NegotiateRequest{Have: &syncpb.HaveMessage{}}
		for _, e := range m.GetEntries() {
			req.Have.Hashes = append(req.Have.Hashes, e.GetBlobHash())
		}
		b.ReportAllocs()
		b.ResetTimer()
		for i := 0; i < b.N; i++ {
			plan, err := pb.svc.Negotiate(pb.ctx, req)
			if err != nil {
				b.Fatal(err)
			}
			if len(plan.GetObjectsToSend()) != 0 {
				b.Fatalf("%d objects missing", len(plan.GetObjectsToSend()))
			}
		}
	}
	b.Run("store=memory", func(b *testing.B) { run(b, newMemObjects()) })
	b.Run("store=disk", func(b *testing.B) { run(b, objstore.New(b.TempDir())) })
}
