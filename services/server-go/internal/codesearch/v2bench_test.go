package codesearch

import (
	"context"
	"fmt"
	"io"
	"log/slog"
	"math/rand"
	"path/filepath"
	"strings"
	"testing"

	"google.golang.org/protobuf/proto"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
	"github.com/seanfilimon/worktree/services/server-go/internal/objstore"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
)

// Benchmarks of the v2 index (§15.9) over a synthetic tree of
// v2BenchFiles source files (Go, Rust, TypeScript, Python; ~3 KiB each).
// Run with:
//
//	go test -run '^$' -bench V2 -benchtime 20x ./internal/codesearch
//
// The numbers recorded in specs/server/CodeSearch.md come from this file.

const v2BenchFiles = 3000

var v2Words = []string{"request", "response", "handler", "config", "buffer", "parse", "render", "client", "server",
	"session", "token", "cache", "index", "query", "result", "error", "stream", "update", "delete", "create"}

// v2BenchFile returns a deterministic source file.
func v2BenchFile(rng *rand.Rand, i int) (string, string) {
	w := func() string { return v2Words[rng.Intn(len(v2Words))] }
	title := func(s string) string { return strings.ToUpper(s[:1]) + s[1:] }
	var b strings.Builder
	switch i % 4 {
	case 0:
		fmt.Fprintf(&b, "package pkg%d\n\n", i%50)
		for f := 0; f < 12; f++ {
			fmt.Fprintf(&b, "// %s%s%d handles the %s %s.\nfunc %s%s%d(%s string) error {\n\tif %s == \"\" {\n\t\treturn errNo%s\n\t}\n\treturn nil\n}\n\n",
				title(w()), title(w()), i*100+f, w(), w(), title(w()), title(w()), i*100+f, w(), w(), title(w()))
		}
	case 1:
		fmt.Fprintf(&b, "pub struct %s%d {\n    %s: u64,\n}\n\nimpl %s%d {\n", title(w()), i, w(), title(w()), i)
		for f := 0; f < 12; f++ {
			fmt.Fprintf(&b, "    pub fn %s_%s_%d(&self, %s: &str) -> Result<(), Error> {\n        let %s = %s.len();\n        Ok(())\n    }\n\n",
				w(), w(), i*100+f, w(), w(), w())
		}
		b.WriteString("}\n")
	case 2:
		fmt.Fprintf(&b, "export interface %s%d {\n  %s: string;\n}\n\nexport class %sStore%d {\n", title(w()), i, w(), title(w()), i)
		for f := 0; f < 12; f++ {
			fmt.Fprintf(&b, "  async %s%s%d(%s: string): Promise<void> {\n    const %s = await fetch(%s);\n    return;\n  }\n\n",
				w(), title(w()), i*100+f, w(), w(), w())
		}
		b.WriteString("}\n")
	default:
		fmt.Fprintf(&b, "MAX_%s_%d = 10\n\nclass %s%d:\n", strings.ToUpper(w()), i, title(w()), i)
		for f := 0; f < 12; f++ {
			fmt.Fprintf(&b, "    def %s_%s_%d(self, %s):\n        %s = self.%s(%s)\n        return %s\n\n",
				w(), w(), i*100+f, w(), w(), w(), w(), w())
		}
	}
	ext := []string{".go", ".rs", ".ts", ".py"}[i%4]
	return fmt.Sprintf("src/m%02d/file%04d%s", i%40, i, ext), b.String()
}

type v2Bench struct {
	store *meta.Memory
	objs  objstore.Store
	tree  *domain.Tree
	dir   string
	bytes int64
}

func newV2Bench(b *testing.B) *v2Bench {
	b.Helper()
	ctx := context.Background()
	root := b.TempDir()
	e := &v2Bench{store: meta.NewMemory(), objs: objstore.New(filepath.Join(root, "objects")), dir: filepath.Join(root, "codeindex")}
	e.tree = &domain.Tree{ID: "tree-bench", TenantID: "t-bench", Name: "bench", DefaultBranch: "main", CreatedAt: t0}
	if err := e.store.CreateTree(ctx, e.tree); err != nil {
		b.Fatal(err)
	}
	rng := rand.New(rand.NewSource(42))
	m := &syncpb.ManifestRecord{}
	for i := 0; i < v2BenchFiles; i++ {
		p, c := v2BenchFile(rng, i)
		h := hash.Sum([]byte(c))
		if err := e.objs.Put(ctx, h, []byte(c)); err != nil {
			b.Fatal(err)
		}
		e.bytes += int64(len(c))
		m.Entries = append(m.Entries, &syncpb.ManifestEntry{Path: p, BlobHash: &syncpb.ContentHash{Value: h[:]}, Size: uint64(len(c))})
	}
	raw, err := proto.Marshal(m)
	if err != nil {
		b.Fatal(err)
	}
	mh := hash.Sum(raw)
	if err := e.objs.Put(ctx, mh, raw); err != nil {
		b.Fatal(err)
	}
	if err := e.store.PutSnapshot(ctx, &domain.Snapshot{ID: "bench-1", TreeID: e.tree.ID, BranchName: "main", ManifestHash: mh, CreatedAt: t0}); err != nil {
		b.Fatal(err)
	}
	if err := e.store.CompareAndSwapBranchTip(ctx, e.tree.ID, "main", "", "bench-1"); err != nil {
		b.Fatal(err)
	}
	return e
}

func (e *v2Bench) index(dir string) *Index {
	ix := New(Deps{Store: e.store, Objects: e.objs, Logger: slog.New(slog.NewTextHandler(io.Discard, nil))})
	ix.Configure(Options{Dir: dir})
	return ix
}

// BenchmarkV2IndexBuild: a cold build of the whole tree (blob reads,
// trigrams, symbols, encoding, segment write).
func BenchmarkV2IndexBuild(b *testing.B) {
	e := newV2Bench(b)
	ctx := context.Background()
	b.SetBytes(e.bytes)
	for b.Loop() {
		dir := filepath.Join(b.TempDir(), "idx")
		ix := e.index(dir)
		if _, err := ix.ensureBranch(ctx, e.tree, "main"); err != nil {
			b.Fatal(err)
		}
	}
	ix := e.index(filepath.Join(b.TempDir(), "idx"))
	v, _ := ix.ensureBranch(ctx, e.tree, "main")
	b.ReportMetric(float64(v.seg.stats.Files), "files")
	b.ReportMetric(float64(v.seg.stats.Symbols), "symbols")
	b.ReportMetric(float64(v.seg.encoded)/(1<<20), "segment-MiB")
	b.ReportMetric(float64(v.seg.size)/(1<<20), "memory-MiB")
}

// BenchmarkV2IndexRebuildCached: building the segment when every blob is
// already in a previous segment (no object-store reads): the CPU cost of
// trigrams, symbol extraction and encoding.
func BenchmarkV2IndexRebuildCached(b *testing.B) {
	e := newV2Bench(b)
	ctx := context.Background()
	ix := e.index(e.dir)
	v, err := ix.ensureBranch(ctx, e.tree, "main")
	if err != nil {
		b.Fatal(err)
	}
	snap, err := e.store.GetSnapshot(ctx, "bench-1")
	if err != nil {
		b.Fatal(err)
	}
	m, err := ix.readManifest(ctx, snap.ManifestHash)
	if err != nil {
		b.Fatal(err)
	}
	reuse := newReuseSet(v.seg)
	b.SetBytes(e.bytes)
	for b.Loop() {
		if _, err := ix.buildSegment(ctx, m, reuse); err != nil {
			b.Fatal(err)
		}
	}
}

// BenchmarkV2IndexLoad: loading the tree's index from disk after a restart.
func BenchmarkV2IndexLoad(b *testing.B) {
	e := newV2Bench(b)
	ctx := context.Background()
	if _, err := e.index(e.dir).ensureBranch(ctx, e.tree, "main"); err != nil {
		b.Fatal(err)
	}
	for b.Loop() {
		ix := e.index(e.dir)
		if _, err := ix.ensureBranch(ctx, e.tree, "main"); err != nil {
			b.Fatal(err)
		}
		if ix.builds.Load() != 0 {
			b.Fatal("rebuilt instead of loading")
		}
	}
}

func (e *v2Bench) warm(b *testing.B) (*Index, []Target) {
	b.Helper()
	ix := e.index(e.dir)
	if _, err := ix.ensureBranch(context.Background(), e.tree, "main"); err != nil {
		b.Fatal(err)
	}
	return ix, []Target{{TenantSlug: "bench", Tree: e.tree}}
}

func benchSearch(b *testing.B, q Query) {
	e := newV2Bench(b)
	ix, targets := e.warm(b)
	ctx := context.Background()
	hits := 0
	for b.Loop() {
		res, err := ix.Search(ctx, q, targets, nil)
		if err != nil {
			b.Fatal(err)
		}
		hits = len(res.Hits)
	}
	b.ReportMetric(float64(hits), "hits")
}

// Literal search for a rare identifier (selective trigram prefilter).
func BenchmarkV2SearchLiteralRare(b *testing.B) {
	benchSearch(b, Query{Pattern: "123405", Limit: 100})
}

// Literal search for a common word (many candidate files, limit 50).
func BenchmarkV2SearchLiteralCommon(b *testing.B) {
	benchSearch(b, Query{Pattern: "handler", Limit: 50})
}

// Regular expression with a trigram prefilter, limit 1000.
func BenchmarkV2SearchRegex(b *testing.B) {
	benchSearch(b, Query{Pattern: `func (Parse|Render)\w+\(`, Regex: true, CaseSensitive: true, Limit: 1000})
}

// Symbol search: prefix and substring ranking over every symbol.
func BenchmarkV2SearchSymbols(b *testing.B) {
	e := newV2Bench(b)
	ix, targets := e.warm(b)
	ctx := context.Background()
	n := 0
	for b.Loop() {
		res, err := ix.SearchSymbols(ctx, SymbolQuery{Query: "parse", Limit: 50}, targets, nil)
		if err != nil {
			b.Fatal(err)
		}
		n = len(res.Symbols)
	}
	b.ReportMetric(float64(n), "symbols")
}
