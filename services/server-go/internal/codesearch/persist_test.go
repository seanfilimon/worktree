package codesearch

import (
	"context"
	"encoding/binary"
	"errors"
	"hash/crc32"
	"io"
	"log/slog"
	"math/rand"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
)

// Persistent index, memory cap, branches and admin status (§15.9).

// restart replaces w.ix by a fresh Index over the same store, objects and
// index directory, as a server restart does.
func (w *world) restart(t *testing.T, o Options) {
	t.Helper()
	w.ix = New(Deps{Store: w.store, Objects: w.objs, Logger: slog.New(slog.NewTextHandler(io.Discard, nil))})
	w.ix.Configure(o)
}

func (w *world) dir(t *testing.T) string {
	t.Helper()
	if w.ix.opt.Dir == "" {
		t.Fatal("world without an index directory")
	}
	return w.ix.opt.Dir
}

// segFiles lists the segment files of a tree.
func segFiles(t *testing.T, dir, treeID string) []string {
	t.Helper()
	entries, err := os.ReadDir(filepath.Join(dir, treeDirName(treeID)))
	if errors.Is(err, os.ErrNotExist) {
		return nil
	}
	must(t, err)
	var out []string
	for _, e := range entries {
		out = append(out, e.Name())
	}
	return out
}

func TestIndexSurvivesRestart(t *testing.T) {
	w := newWorld(t)
	dir := w.dir(t)
	web := w.tree(t, "acme", "web")
	snap := w.push(t, web, "main", sample)
	tg := Target{TenantSlug: "acme", Tree: web}
	want := locs(w.search(t, Query{Pattern: "hello"}, tg))
	before, _ := w.ix.Status(web.ID)
	eq(t, w.ix.builds.Load(), int64(1))
	if files := segFiles(t, dir, web.ID); len(files) != 2 { // state.json + one segment
		t.Fatalf("index files: %v", files)
	}
	reads := w.objs.count(sample["README.md"])

	w.restart(t, Options{Dir: dir})
	eq(t, w.ix.Indexed(web.ID), true)
	if m, ok := w.ix.Status(web.ID); !ok || !m.BuiltAt.Equal(before.BuiltAt) || m.SnapshotID != snap {
		t.Fatalf("status after restart = %+v %v, want %+v", m, ok, before)
	}
	eq(t, locs(w.search(t, Query{Pattern: "hello"}, tg)), want)
	eq(t, []int64{w.ix.builds.Load(), w.ix.diskLoads.Load()}, []int64{0, 1})
	eq(t, w.objs.count(sample["README.md"]), reads) // no blob read again
	sym := w.symbols(t, SymbolQuery{Query: "reverse"}, tg)
	eq(t, symLocs(sym), []string{"web:src/util/str.go:4 function Reverse exact"})

	// A push after the restart rebuilds incrementally from the disk index.
	w.push(t, web, "main", map[string]string{"README.md": sample["README.md"], "new.txt": "hello new\n"})
	eq(t, len(w.search(t, Query{Pattern: "hello"}, tg).Hits), 3)
	eq(t, w.objs.count(sample["README.md"]), reads)
	m, _ := w.ix.Status(web.ID)
	eq(t, []int{m.Files, m.Reused}, []int{2, 1})
	// The old segment was swapped out and deleted.
	if files := segFiles(t, dir, web.ID); len(files) != 2 || !strings.HasSuffix(strings.Join(files, ","), stateFile) {
		t.Fatalf("index files after rebuild: %v", files)
	}
}

func TestCorruptAndMismatchedIndexesAreRebuilt(t *testing.T) {
	cases := []struct {
		name   string
		damage func(t *testing.T, treeDir string)
	}{
		{"flipped segment byte", func(t *testing.T, treeDir string) {
			p := onlySegment(t, treeDir)
			data, err := os.ReadFile(p)
			must(t, err)
			data[len(data)/2] ^= 0xff
			must(t, os.WriteFile(p, data, 0o644))
		}},
		{"truncated segment", func(t *testing.T, treeDir string) {
			p := onlySegment(t, treeDir)
			data, err := os.ReadFile(p)
			must(t, err)
			must(t, os.WriteFile(p, data[:len(data)/3], 0o644))
		}},
		{"segment format version", func(t *testing.T, treeDir string) {
			p := onlySegment(t, treeDir)
			data, err := os.ReadFile(p)
			must(t, err)
			binary.LittleEndian.PutUint32(data[4:8], FormatVersion+1)
			must(t, os.WriteFile(p, data, 0o644))
		}},
		{"state format version", func(t *testing.T, treeDir string) {
			p := filepath.Join(treeDir, stateFile)
			raw, err := os.ReadFile(p)
			must(t, err)
			must(t, os.WriteFile(p, []byte(strings.Replace(string(raw), `"format": 1`, `"format": 99`, 1)), 0o644))
		}},
		{"garbage state", func(t *testing.T, treeDir string) {
			must(t, os.WriteFile(filepath.Join(treeDir, stateFile), []byte("{not json"), 0o644))
		}},
		{"missing segment", func(t *testing.T, treeDir string) {
			must(t, os.Remove(onlySegment(t, treeDir)))
		}},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			w := newWorld(t)
			dir := w.dir(t)
			web := w.tree(t, "acme", "web")
			w.push(t, web, "main", sample)
			tg := Target{TenantSlug: "acme", Tree: web}
			want := locs(w.search(t, Query{Pattern: "package"}, tg))
			c.damage(t, filepath.Join(dir, treeDirName(web.ID)))

			w.restart(t, Options{Dir: dir})
			eq(t, locs(w.search(t, Query{Pattern: "package"}, tg)), want)
			eq(t, w.ix.builds.Load(), int64(1))
			// The rebuilt index is usable after the next restart.
			w.restart(t, Options{Dir: dir})
			eq(t, locs(w.search(t, Query{Pattern: "package"}, tg)), want)
			eq(t, w.ix.builds.Load(), int64(0))
		})
	}
}

func onlySegment(t *testing.T, treeDir string) string {
	t.Helper()
	matches, err := filepath.Glob(filepath.Join(treeDir, "*.seg"))
	must(t, err)
	if len(matches) != 1 {
		t.Fatalf("segments: %v", matches)
	}
	return matches[0]
}

func TestMemoryCapEvictsAndReloads(t *testing.T) {
	w := newWorld(t)
	dir := w.dir(t)
	w.restart(t, Options{Dir: dir, MemoryLimit: 1}) // every segment is over budget
	web := w.tree(t, "acme", "web")
	api := w.tree(t, "acme", "api")
	w.push(t, web, "main", map[string]string{"a.txt": "needle web\n"})
	w.push(t, api, "main", map[string]string{"a.txt": "needle api\n"})
	tw, ta := Target{TenantSlug: "acme", Tree: web}, Target{TenantSlug: "acme", Tree: api}
	eq(t, len(w.search(t, Query{Pattern: "needle"}, tw, ta).Hits), 2)
	st, err := w.ix.IndexStatus(context.Background())
	must(t, err)
	eq(t, []any{st.LoadedSegments, st.MemoryLimit, st.Builds, st.Evictions >= 1}, []any{1, int64(1), int64(2), true})
	// Evicted segments come back from disk, not from a rebuild.
	eq(t, len(w.search(t, Query{Pattern: "needle"}, tw, ta).Hits), 2)
	st, _ = w.ix.IndexStatus(context.Background())
	eq(t, []any{st.Builds, st.DiskLoads >= 1}, []any{int64(2), true})

	// Without a data directory an evicted segment is rebuilt.
	w2 := newWorld(t)
	w2.restart(t, Options{MemoryLimit: 1})
	web2 := w2.tree(t, "acme", "web")
	api2 := w2.tree(t, "acme", "api")
	w2.push(t, web2, "main", map[string]string{"a.txt": "needle web\n"})
	w2.push(t, api2, "main", map[string]string{"a.txt": "needle api\n"})
	targets := []Target{{TenantSlug: "acme", Tree: web2}, {TenantSlug: "acme", Tree: api2}}
	eq(t, len(w2.search(t, Query{Pattern: "needle"}, targets...).Hits), 2)
	eq(t, len(w2.search(t, Query{Pattern: "needle"}, targets...).Hits), 2)
	if b := w2.ix.builds.Load(); b < 3 {
		t.Fatalf("builds = %d, want rebuilds after eviction", b)
	}
}

func TestSegmentRoundTripAndFuzz(t *testing.T) {
	in := &segInput{
		stats:    segStats{Files: 2, Bytes: 9, Trigrams: 3, Symbols: 1, BuiltAt: t0},
		docs:     []segDoc{{path: "a.go", hash: hash.Sum([]byte("x")), content: []byte("func A()\n")}, {path: "b.txt", content: []byte("")}},
		binary:   []skipEntry{{path: "c.bin", hash: hash.Sum([]byte("bin"))}},
		syms:     []symbol{{Name: "A", Kind: KindFunction, Line: 1, Signature: "func A()", doc: 0}},
		postings: map[trigram][]uint32{makeTrigram('f', 'u', 'n'): {0}, makeTrigram('a', 'b', 'c'): {0, 1}, 0x10203: {1}},
	}
	data, err := encodeSegment(in)
	must(t, err)
	m := hash.Sum([]byte("manifest"))
	s, err := decodeSegment(m, data)
	must(t, err)
	eq(t, []any{s.manifest, s.stats, len(s.docs), string(s.docs[0].content), s.docs[1].path, s.binary[0].path, s.syms[0].Name, s.lnames[0]},
		[]any{m, in.stats, 2, "func A()\n", "b.txt", "c.bin", "A", "a"})
	eq(t, s.postings(makeTrigram('a', 'b', 'c')), []uint32{0, 1})
	eq(t, s.postings(makeTrigram('z', 'z', 'z')), []uint32(nil))

	rng := rand.New(rand.NewSource(1))
	for i := 0; i < 2000; i++ {
		bad := append([]byte(nil), data...)
		switch i % 3 {
		case 0:
			bad[rng.Intn(len(bad))] ^= byte(1 + rng.Intn(255))
		case 1:
			bad = bad[:rng.Intn(len(bad))]
		default:
			// Corrupt the body and fix the checksum: the structure checks
			// must still catch it (or decode something harmless).
			body := bad[8 : len(bad)-4]
			body[rng.Intn(len(body))] = byte(rng.Intn(256))
			binary.LittleEndian.PutUint32(bad[len(bad)-4:], crc(bad[:len(bad)-4]))
		}
		if s, err := decodeSegment(m, bad); err == nil {
			// A structurally valid result must be safe to query.
			for _, tr := range s.tri {
				for _, id := range s.postings(trigram(tr)) {
					_ = s.docs[id]
				}
			}
		} else if !errors.Is(err, errSegCorrupt) && !errors.Is(err, errSegFormat) {
			t.Fatalf("unexpected error %v", err)
		}
	}
}

func TestBranchSearch(t *testing.T) {
	w := newWorld(t)
	ctx := context.Background()
	web := w.tree(t, "acme", "web")
	w.push(t, web, "main", map[string]string{"a.txt": "shared needle\n", "m.txt": "main only needle\n"})
	w.push(t, web, "dev", map[string]string{"a.txt": "shared needle\n", "d.txt": "dev only needle\n"})
	tg := Target{TenantSlug: "acme", Tree: web}

	// Default: the default branch only.
	eq(t, locs(w.search(t, Query{Pattern: "needle"}, tg)), []string{"web:a.txt:1", "web:m.txt:1"})
	// One branch.
	res := w.search(t, Query{Pattern: "needle", Branch: "dev"}, tg)
	eq(t, locs(res), []string{"web:a.txt:1", "web:d.txt:1"})
	eq(t, res.Hits[0].Branch, "dev")
	eq(t, len(w.search(t, Query{Pattern: "needle", Branch: "nope"}, tg).Hits), 0)
	// All branches: identical files are reported once (on the default branch).
	res = w.search(t, Query{Pattern: "needle", AllBranches: true}, tg)
	eq(t, locs(res), []string{"web:a.txt:1", "web:m.txt:1", "web:d.txt:1"})
	eq(t, []string{res.Hits[0].Branch, res.Hits[1].Branch, res.Hits[2].Branch}, []string{"main", "main", "dev"})
	if _, err := w.ix.Search(ctx, Query{Pattern: "needle", Branch: "dev", AllBranches: true}, []Target{tg}, nil); !errors.Is(err, ErrInvalidQuery) {
		t.Fatalf("branch + all: %v", err)
	}

	// A push to an indexed non-default branch rebuilds it eagerly.
	snap := w.push(t, web, "dev", map[string]string{"d.txt": "dev changed needle\n"})
	must(t, w.ix.HandleEvent(ctx, domain.Event{Type: domain.EventBranchPushed, TreeID: web.ID, Payload: map[string]any{"branch": "dev"}}))
	st, ok := w.ix.TreeStatus(ctx, web)
	if !ok || len(st.Branches) != 2 || st.Branches[1].Branch != "dev" || st.Branches[1].SnapshotID != snap || st.Branches[1].Building {
		t.Fatalf("status after dev push: %+v", st)
	}
	// branch.deleted drops the branch (payload: domain.Branch).
	must(t, w.ix.HandleEvent(ctx, domain.Event{Type: domain.EventBranchDeleted, TreeID: web.ID, Payload: &domain.Branch{Name: "dev"}}))
	st, _ = w.ix.TreeStatus(ctx, web)
	eq(t, st.BranchCount, 1)
	if files := segFiles(t, w.dir(t), web.ID); len(files) != 2 {
		t.Fatalf("dev segment kept after branch.deleted: %v", files)
	}
}

func TestBranchesDedupAndCap(t *testing.T) {
	w := newWorld(t)
	w.restart(t, Options{Dir: w.dir(t), MaxBranches: 2})
	web := w.tree(t, "acme", "web")
	files := map[string]string{"a.txt": "needle\n"}
	w.push(t, web, "main", files)
	w.push(t, web, "copy", files) // same manifest as main
	tg := Target{TenantSlug: "acme", Tree: web}
	eq(t, locs(w.search(t, Query{Pattern: "needle", AllBranches: true}, tg)), []string{"web:a.txt:1"})
	st, _ := w.ix.TreeStatus(context.Background(), web)
	eq(t, []int{st.BranchCount, st.Segments}, []int{2, 1})
	eq(t, w.ix.builds.Load(), int64(1)) // deduplicated by manifest

	// With the cap at 2, all-branches covers the default branch and the
	// most recently updated other branch.
	time.Sleep(20 * time.Millisecond)
	w.push(t, web, "newest", map[string]string{"n.txt": "needle newest\n"})
	res := w.search(t, Query{Pattern: "needle", AllBranches: true}, tg)
	eq(t, locs(res), []string{"web:a.txt:1", "web:n.txt:1"})
	st, _ = w.ix.TreeStatus(context.Background(), web)
	eq(t, st.BranchCount, 2)
	names := []string{st.Branches[0].Branch, st.Branches[1].Branch}
	eq(t, names, []string{"main", "newest"})
}

func TestStatusAndRebuild(t *testing.T) {
	w := newWorld(t)
	ctx := context.Background()
	must(t, w.store.CreateTenant(ctx, &domain.Tenant{ID: "t-acme", Slug: "acme", Name: "Acme", CreatedAt: t0}))
	web := w.tree(t, "acme", "web")
	snap := w.push(t, web, "main", sample)
	tg := Target{TenantSlug: "acme", Tree: web}
	w.search(t, Query{Pattern: "hello"}, tg)

	st, err := w.ix.IndexStatus(ctx)
	must(t, err)
	eq(t, []any{st.FormatVersion, st.Persistent, st.MaxBranches, st.Builds, len(st.Trees)}, []any{FormatVersion, true, DefaultMaxBranches, int64(1), 1})
	tr := st.Trees[0]
	eq(t, []any{tr.TenantSlug, tr.Tree, tr.DefaultBranch, tr.State, tr.Files, tr.Symbols, tr.BranchCount, tr.Segments},
		[]any{"acme", "web", "main", "ready", 4, 2, 1, 1})
	if tr.DiskBytes <= 0 || tr.MemoryBytes <= 0 || tr.BuiltAt.IsZero() || tr.Branches[0].SnapshotID != snap || !tr.Branches[0].Loaded {
		t.Fatalf("tree status: %+v", tr)
	}

	reads := w.objs.count(sample["README.md"])
	rb, err := w.ix.Rebuild(ctx, web)
	must(t, err)
	eq(t, rb.Files, 4)
	eq(t, w.objs.count(sample["README.md"]), reads+1) // a rebuild re-reads every blob
	eq(t, w.ix.builds.Load(), int64(2))
	all, err := w.ix.RebuildAll(ctx)
	must(t, err)
	eq(t, []any{all.Rebuilt, len(all.Trees)}, []any{1, 1})

	// Indexes of trees deleted while the server was down are dropped.
	w.restart(t, Options{Dir: w.dir(t)})
	must(t, w.store.DeleteTree(ctx, web.ID))
	st, err = w.ix.IndexStatus(ctx)
	must(t, err)
	eq(t, len(st.Trees), 0)
	eq(t, len(segFiles(t, w.dir(t), web.ID)), 0)
}

func crc(b []byte) uint32 { return crc32.Checksum(b, crcTable) }
