package httpapi

// Tests of POST /api/v1/admin/fsck (docs/ENGINEERING_PLAN.md §14.6).

import (
	"context"
	"os"
	"path/filepath"
	"slices"
	"testing"

	"google.golang.org/protobuf/proto"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/objstore"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
)

// newFsckFixture is the standard fixture with its objects copied into a
// (compressing) disk store.
func newFsckFixture(t *testing.T) (*fixture, *objstore.DiskStore) {
	t.Helper()
	disk := objstore.New(t.TempDir())
	f := newFixture(t, func(d *Deps) {
		fo := d.Objects.(*fakeObjects)
		for h, data := range fo.objs {
			must(t, disk.Put(context.Background(), h, data))
		}
		d.Objects = disk
	})
	return f, disk
}

func objectFile(disk *objstore.DiskStore, h hash.Hash) string {
	hex := h.Hex()
	return filepath.Join(disk.Root(), "objects", hex[:2], hex[2:])
}

func runFsckAs(t *testing.T, f *fixture, as string) fsckReport {
	t.Helper()
	rec := f.do(t, "POST", "/api/v1/admin/fsck", as, nil)
	if rec.Code != 200 {
		t.Fatalf("fsck: %d %s", rec.Code, rec.Body.String())
	}
	return decode[fsckReport](t, rec)
}

func TestFsckHealthyStore(t *testing.T) {
	f, disk := newFsckFixture(t)
	count, _, err := disk.Stats(context.Background())
	must(t, err)
	rep := runFsckAs(t, f, "admin")
	if rep.ObjectsChecked != count || len(rep.Corrupt) != 0 || len(rep.Missing) != 0 ||
		len(rep.MissingSnapshots) != 0 || rep.SnapshotsChecked != 2 || rep.DurationMS < 0 {
		t.Fatalf("healthy report = %+v (objects %d)", rep, count)
	}
	// Only server admins may run it.
	if rec := f.do(t, "POST", "/api/v1/admin/fsck", "alice", nil); rec.Code != 403 {
		t.Fatalf("owner: %d", rec.Code)
	}
	if rec := f.do(t, "POST", "/api/v1/admin/fsck", "", nil); rec.Code != 401 {
		t.Fatalf("anonymous: %d", rec.Code)
	}
}

func TestFsckDetectsCorruptionAndMissingObjects(t *testing.T) {
	f, disk := newFsckFixture(t)
	ctx := context.Background()

	// Corrupt the first README (snap-1) and lose src/main.go (snap-2).
	readme := hash.Sum([]byte("# web\n"))
	must(t, os.WriteFile(objectFile(disk, readme), []byte("tampered"), 0o644))
	mainGo := hash.Sum([]byte("package main\n\nfunc main() {}\n"))
	must(t, disk.Delete(ctx, mainGo))

	// A chunked file whose second chunk was never stored, on its own branch.
	c1, c2 := []byte("chunk one"), []byte("chunk two")
	h1, h2 := hash.Sum(c1), hash.Sum(c2)
	must(t, disk.Put(ctx, h1, c1))
	cl, err := proto.Marshal(&syncpb.ChunkList{Size: 18, Chunks: []*syncpb.Chunk{
		{Hash: &syncpb.ContentHash{Value: h1[:]}, Size: 9},
		{Hash: &syncpb.ContentHash{Value: h2[:]}, Size: 9},
	}})
	must(t, err)
	hcl := hash.Sum(cl)
	must(t, disk.Put(ctx, hcl, cl))
	raw, err := proto.Marshal(&syncpb.ManifestRecord{Entries: []*syncpb.ManifestEntry{
		{Path: "big.bin", BlobHash: &syncpb.ContentHash{Value: hcl[:]}, Size: 18, Chunked: true},
	}})
	must(t, err)
	mh := hash.Sum(raw)
	must(t, disk.Put(ctx, mh, raw))
	f.store.snapshots["snap-big"] = domain.Snapshot{ID: "snap-big", TreeID: f.web.ID, ManifestHash: mh, CreatedAt: t0}
	must(t, f.store.PutBranch(ctx, &domain.Branch{TreeID: f.web.ID, Name: "big", Tip: "snap-big", CreatedAt: t0, UpdatedAt: t0}))
	// A branch pointing at a snapshot record that does not exist.
	must(t, f.store.PutBranch(ctx, &domain.Branch{TreeID: f.web.ID, Name: "broken", Tip: "snap-nope", CreatedAt: t0, UpdatedAt: t0}))
	// A snapshot whose manifest is a corrupt (undecodable) object.
	junk := []byte("\xff\xff\xff not a manifest")
	hj := hash.Sum(junk)
	must(t, disk.Put(ctx, hj, junk))
	f.store.snapshots["snap-junk"] = domain.Snapshot{ID: "snap-junk", TreeID: f.web.ID, ManifestHash: hj, CreatedAt: t0}
	f.store.tags[[2]string{f.web.ID, "junk"}] = domain.Tag{TreeID: f.web.ID, Name: "junk", Target: "snap-junk", CreatedAt: t0}

	rep := runFsckAs(t, f, "admin")
	wantCorrupt := []string{readme.Hex(), hj.Hex()}
	slices.Sort(wantCorrupt)
	if !slices.Equal(rep.Corrupt, wantCorrupt) {
		t.Fatalf("corrupt = %v, want %v", rep.Corrupt, wantCorrupt)
	}
	missing := map[string]string{}
	for _, m := range rep.Missing {
		missing[m.Object] = m.Snapshot
	}
	if len(missing) != 2 || missing[mainGo.Hex()] != "snap-2" || missing[h2.Hex()] != "snap-big" {
		t.Fatalf("missing = %+v", rep.Missing)
	}
	if !slices.Equal(rep.MissingSnapshots, []string{"snap-nope"}) {
		t.Fatalf("missing snapshots = %v", rep.MissingSnapshots)
	}
	if rep.SnapshotsChecked != 4 {
		t.Fatalf("snapshots checked = %d", rep.SnapshotsChecked)
	}
}

func TestFsckWithoutObjectStore(t *testing.T) {
	f := newFixture(t, func(d *Deps) { d.Objects = nil })
	if rec := f.do(t, "POST", "/api/v1/admin/fsck", "admin", nil); rec.Code != 501 {
		t.Fatalf("no object store: %d", rec.Code)
	}
}
