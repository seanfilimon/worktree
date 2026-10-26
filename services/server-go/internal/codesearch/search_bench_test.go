package codesearch

// Benchmarks of code search queries (hardening, docs/ENGINEERING_PLAN.md
// §15.10) over a warm index of a 2,000-file tree (~6 MB of Go-like
// source). Run with
//
//	go test ./internal/codesearch -run '^$' -bench . -benchmem

import (
	"context"
	"fmt"
	"io"
	"log/slog"
	"strings"
	"testing"

	"google.golang.org/protobuf/proto"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
	"github.com/seanfilimon/worktree/services/server-go/internal/objstore"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
)

const (
	benchFiles     = 2_000
	benchFileLines = 100
)

// benchSource returns a ~3 KB Go-like file; every 200th file mentions the
// rare identifier "needleValue".
func benchSource(i int) []byte {
	var b strings.Builder
	fmt.Fprintf(&b, "package pkg%03d\n\nimport \"fmt\"\n\n", i/100)
	for l := 0; l < benchFileLines/5; l++ {
		fmt.Fprintf(&b, "// Handle%dx%d serves request %d of file %d.\n", i, l, l, i)
		fmt.Fprintf(&b, "func Handle%dx%dHandler(w Writer, r *Request) error {\n", i, l)
		fmt.Fprintf(&b, "\tfmt.Println(\"processing\", r.ID, %d)\n", l)
		if i%200 == 0 && l == 3 {
			b.WriteString("\treturn needleValue\n")
		} else {
			b.WriteString("\treturn nil\n")
		}
		b.WriteString("}\n")
	}
	return []byte(b.String())
}

type searchBench struct {
	ix     *Index
	target []Target
}

func newSearchBench(b *testing.B) *searchBench {
	b.Helper()
	ctx := context.Background()
	store := meta.NewMemory()
	objs := objstore.New(b.TempDir())
	ix := New(Deps{Store: store, Objects: objs, Logger: slog.New(slog.NewTextHandler(io.Discard, nil))})
	tree := &domain.Tree{ID: "tree-bench", TenantID: "t-bench", Name: "big", DefaultBranch: "main", Visibility: domain.VisibilityPrivate, CreatedAt: t0}
	must(b, store.CreateTree(ctx, tree))
	m := &syncpb.ManifestRecord{}
	for i := 0; i < benchFiles; i++ {
		data := benchSource(i)
		h := hash.Sum(data)
		must(b, objs.Put(ctx, h, data))
		m.Entries = append(m.Entries, &syncpb.ManifestEntry{
			Path: fmt.Sprintf("src/pkg%03d/file%04d.go", i/100, i), BlobHash: &syncpb.ContentHash{Value: h[:]}, Size: uint64(len(data)),
		})
	}
	raw, err := proto.Marshal(m)
	must(b, err)
	mh := hash.Sum(raw)
	must(b, objs.Put(ctx, mh, raw))
	must(b, store.PutSnapshot(ctx, &domain.Snapshot{ID: "snap-1", TreeID: tree.ID, BranchName: "main", ManifestHash: mh, Author: "ada", Message: "big", CreatedAt: t0}))
	must(b, store.CompareAndSwapBranchTip(ctx, tree.ID, "main", "", "snap-1"))
	sb := &searchBench{ix: ix, target: []Target{{TenantSlug: "bench", Tree: tree}}}
	// Warm the index so that the benchmarks measure queries only.
	if _, err := ix.Search(ctx, Query{Pattern: "package"}, sb.target, nil); err != nil {
		b.Fatal(err)
	}
	return sb
}

func BenchmarkSearch(b *testing.B) {
	sb := newSearchBench(b)
	cases := []struct {
		name string
		q    Query
	}{
		{"literal-rare", Query{Pattern: "needleValue", CaseSensitive: true}},
		{"literal-rare-fold", Query{Pattern: "needlevalue"}},
		{"literal-common", Query{Pattern: "processing", CaseSensitive: true}},
		{"literal-common-limit1000", Query{Pattern: "processing", CaseSensitive: true, Limit: MaxLimit}},
		{"regex", Query{Pattern: `func Handle1\d+x3Handler`, Regex: true, CaseSensitive: true}},
		{"regex-fold-path", Query{Pattern: `return needle\w+`, Regex: true, Path: "src/pkg01*/**"}},
		{"no-match", Query{Pattern: "zzzqqqxxx", CaseSensitive: true}},
	}
	for _, c := range cases {
		b.Run(c.name, func(b *testing.B) {
			ctx := context.Background()
			b.ReportAllocs()
			for i := 0; i < b.N; i++ {
				if _, err := sb.ix.Search(ctx, c.q, sb.target, nil); err != nil {
					b.Fatal(err)
				}
			}
		})
	}
}

// BenchmarkIndexBuild measures a full (cold) index build of the tree.
func BenchmarkIndexBuild(b *testing.B) {
	sb := newSearchBench(b)
	ctx := context.Background()
	tree := sb.target[0].Tree
	b.ReportAllocs()
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		if _, err := sb.ix.build(ctx, tree, nil); err != nil {
			b.Fatal(err)
		}
	}
}
