package codesearch

import (
	"context"
	"errors"
	"fmt"
	"io"
	"log/slog"
	"reflect"
	"regexp"
	"regexp/syntax"
	"sort"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"google.golang.org/protobuf/proto"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/events"
	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
	"github.com/seanfilimon/worktree/services/server-go/internal/objstore"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
)

var t0 = time.Date(2026, 1, 2, 3, 4, 5, 0, time.UTC)

func must(t testing.TB, err error) {
	t.Helper()
	if err != nil {
		t.Fatal(err)
	}
}

// countingObjects counts Get calls per hash.
type countingObjects struct {
	objstore.Store
	mu   sync.Mutex
	gets map[hash.Hash]int
}

func (c *countingObjects) Get(ctx context.Context, h hash.Hash) ([]byte, error) {
	c.mu.Lock()
	c.gets[h]++
	c.mu.Unlock()
	return c.Store.Get(ctx, h)
}

func (c *countingObjects) count(data string) int {
	c.mu.Lock()
	defer c.mu.Unlock()
	return c.gets[hash.Sum([]byte(data))]
}

type world struct {
	store *meta.Memory
	objs  *countingObjects
	ix    *Index
	seq   int
}

func newWorld(t *testing.T) *world {
	t.Helper()
	w := &world{store: meta.NewMemory(), objs: &countingObjects{Store: objstore.New(t.TempDir()), gets: map[hash.Hash]int{}}}
	w.ix = New(Deps{Store: w.store, Objects: w.objs, Logger: slog.New(slog.NewTextHandler(io.Discard, nil))})
	return w
}

func (w *world) put(t *testing.T, data []byte) hash.Hash {
	t.Helper()
	h := hash.Sum(data)
	must(t, w.objs.Put(context.Background(), h, data))
	return h
}

func (w *world) tree(t *testing.T, tenant, name string) *domain.Tree {
	t.Helper()
	tr := &domain.Tree{ID: "tree-" + tenant + "-" + name, TenantID: "t-" + tenant, Name: name, DefaultBranch: "main", Visibility: domain.VisibilityPrivate, CreatedAt: t0}
	must(t, w.store.CreateTree(context.Background(), tr))
	return tr
}

// push stores a snapshot with files (and extra entries) as the tip of
// branch and returns its id.
func (w *world) push(t *testing.T, tree *domain.Tree, branch string, files map[string]string, extra ...*syncpb.ManifestEntry) string {
	t.Helper()
	ctx := context.Background()
	m := &syncpb.ManifestRecord{Entries: append([]*syncpb.ManifestEntry(nil), extra...)}
	for p, c := range files {
		h := w.put(t, []byte(c))
		m.Entries = append(m.Entries, &syncpb.ManifestEntry{Path: p, BlobHash: &syncpb.ContentHash{Value: h[:]}, Size: uint64(len(c))})
	}
	raw, err := proto.Marshal(m)
	must(t, err)
	mh := w.put(t, raw)
	w.seq++
	id := fmt.Sprintf("snap-%d", w.seq)
	old := ""
	if b, err := w.store.GetBranch(ctx, tree.ID, branch); err == nil {
		old = b.Tip
	}
	must(t, w.store.PutSnapshot(ctx, &domain.Snapshot{ID: id, TreeID: tree.ID, BranchName: branch, ManifestHash: mh, Author: "ada", Message: id, CreatedAt: t0}))
	must(t, w.store.CompareAndSwapBranchTip(ctx, tree.ID, branch, old, id))
	return id
}

func (w *world) search(t *testing.T, q Query, targets ...Target) *Result {
	t.Helper()
	res, err := w.ix.Search(context.Background(), q, targets, nil)
	must(t, err)
	return res
}

// locs renders hits as "tree:path:line".
func locs(res *Result) []string {
	out := []string{}
	for _, h := range res.Hits {
		out = append(out, fmt.Sprintf("%s:%s:%d", h.Tree, h.Path, h.Line))
	}
	return out
}

func eq(t *testing.T, got, want any) {
	t.Helper()
	if !reflect.DeepEqual(got, want) {
		t.Fatalf("got %#v\nwant %#v", got, want)
	}
}

var sample = map[string]string{
	"README.md":       "# Demo\nHello World\nhello again\n",
	"src/main.go":     "package main\n\nfunc main() {\n\tprintln(\"Hello\")\n}\n",
	"src/util/str.go": "package util\n\n// Reverse returns s reversed.\nfunc Reverse(s string) string {\n\treturn s\n}\n",
	"docs/guide.txt":  "line one\r\nKelvin: 300K\r\nend\r\n",
}

func TestLiteralSearch(t *testing.T) {
	w := newWorld(t)
	web := w.tree(t, "acme", "web")
	snap := w.push(t, web, "main", sample)
	tg := Target{TenantSlug: "acme", Tree: web}

	res := w.search(t, Query{Pattern: "hello"}, tg)
	eq(t, locs(res), []string{"web:README.md:2", "web:README.md:3", "web:src/main.go:4"})
	h := res.Hits[0]
	eq(t, []any{h.TenantSlug, h.Branch, h.SnapshotID, h.Text}, []any{"acme", "main", snap, "Hello World"})
	eq(t, res.SearchedTrees, 1)
	eq(t, res.Truncated, false)

	res = w.search(t, Query{Pattern: "Hello", CaseSensitive: true}, tg)
	eq(t, locs(res), []string{"web:README.md:2", "web:src/main.go:4"})

	// Case folding is Unicode-aware: "k" matches the Kelvin sign.
	res = w.search(t, Query{Pattern: "300k"}, tg)
	eq(t, locs(res), []string{"web:docs/guide.txt:2"})
	eq(t, res.Hits[0].Text, "Kelvin: 300K") // "\r" stripped

	// Regex metacharacters are literal.
	res = w.search(t, Query{Pattern: "main()"}, tg)
	eq(t, locs(res), []string{"web:src/main.go:3"})
	eq(t, w.search(t, Query{Pattern: "nothing-here"}, tg).Hits, []domain.CodeSearchHit{})
}

func TestContextLines(t *testing.T) {
	w := newWorld(t)
	web := w.tree(t, "acme", "web")
	w.push(t, web, "main", sample)
	tg := Target{TenantSlug: "acme", Tree: web}

	res := w.search(t, Query{Pattern: "println", Context: 2}, tg)
	eq(t, len(res.Hits), 1)
	eq(t, res.Hits[0].Before, []string{"", "func main() {"})
	eq(t, res.Hits[0].After, []string{"}"})

	res = w.search(t, Query{Pattern: "# Demo", Context: 1}, tg)
	eq(t, res.Hits[0].Before, []string(nil))
	eq(t, res.Hits[0].After, []string{"Hello World"})
}

func TestRegexSearch(t *testing.T) {
	w := newWorld(t)
	web := w.tree(t, "acme", "web")
	w.push(t, web, "main", sample)
	tg := Target{TenantSlug: "acme", Tree: web}
	cases := []struct {
		pattern string
		cs      bool
		want    []string
	}{
		{`^package \w+$`, false, []string{"web:src/main.go:1", "web:src/util/str.go:1"}},
		{`func (main|Reverse)\(`, true, []string{"web:src/main.go:3", "web:src/util/str.go:4"}},
		{`HELLO (world|again)`, false, []string{"web:README.md:2", "web:README.md:3"}},
		{`HELLO (world|again)`, true, []string{}},
		{`(?i)HELLO (world|again)`, true, []string{"web:README.md:2", "web:README.md:3"}},
		{`end$`, true, []string{"web:docs/guide.txt:3"}}, // CRLF line
		{`\d{3}`, true, []string{"web:docs/guide.txt:2"}},
		{`x*`, true, nil}, // matches every line
	}
	for _, c := range cases {
		res := w.search(t, Query{Pattern: c.pattern, Regex: true, CaseSensitive: c.cs, Limit: MaxLimit}, tg)
		if c.want == nil {
			if len(res.Hits) != 17 {
				t.Errorf("%q: %d hits, want every line (17)", c.pattern, len(res.Hits))
			}
			continue
		}
		if got := locs(res); !reflect.DeepEqual(got, c.want) {
			t.Errorf("%q (case=%v): got %v, want %v", c.pattern, c.cs, got, c.want)
		}
	}
}

func TestQueryValidation(t *testing.T) {
	w := newWorld(t)
	web := w.tree(t, "acme", "web")
	tg := Target{TenantSlug: "acme", Tree: web}
	for _, q := range []Query{
		{},
		{Pattern: "ab"},
		{Pattern: "a\nbc"},
		{Pattern: strings.Repeat("x", MaxPattern+1)},
		{Pattern: "(unclosed", Regex: true},
		{Pattern: "abc", Context: MaxContext + 1},
		{Pattern: "abc", Context: -1},
		{Pattern: "abc", Path: "src/[a"},
	} {
		_, err := w.ix.Search(context.Background(), q, []Target{tg}, nil)
		if !errors.Is(err, ErrInvalidQuery) || !errors.Is(err, domain.ErrInvalid) {
			t.Errorf("%+v: err = %v, want ErrInvalidQuery", q, err)
		}
	}
	// Short patterns are fine as regular expressions.
	if _, err := w.ix.Search(context.Background(), Query{Pattern: "a", Regex: true}, []Target{tg}, nil); err != nil {
		t.Fatal(err)
	}
}

func TestPathGlobs(t *testing.T) {
	cases := []struct {
		pattern, path string
		want          bool
	}{
		{"", "a/b.go", true},
		{"*.go", "main.go", true},
		{"*.go", "src/util/str.go", true}, // no slash: any depth
		{"*.go", "README.md", false},
		{"src", "src/util/str.go", true}, // directory covers its contents
		{"src/*.go", "src/main.go", true},
		{"src/*.go", "src/util/str.go", false},
		{"src/**/*.go", "src/util/str.go", true},
		{"**/util/*", "src/util/str.go", true},
		{"docs/", "docs/guide.txt", true},
		{"docs/", "src/docs.go", false},
	}
	for _, c := range cases {
		if got := MatchPath(c.pattern, c.path); got != c.want {
			t.Errorf("MatchPath(%q, %q) = %v, want %v", c.pattern, c.path, got, c.want)
		}
	}

	w := newWorld(t)
	web := w.tree(t, "acme", "web")
	w.push(t, web, "main", sample)
	tg := Target{TenantSlug: "acme", Tree: web}
	eq(t, locs(w.search(t, Query{Pattern: "package", Path: "src/util"}, tg)), []string{"web:src/util/str.go:1"})
	eq(t, locs(w.search(t, Query{Pattern: "hello", Path: "*.md"}, tg)), []string{"web:README.md:2", "web:README.md:3"})
}

func TestSkipsBinaryLargeAndChunked(t *testing.T) {
	w := newWorld(t)
	web := w.tree(t, "acme", "web")
	big := strings.Repeat("needle\n", MaxFileSize/7+1)
	chunked := &syncpb.ManifestEntry{Path: "huge.bin", Size: 10, Chunked: true}
	h := w.put(t, []byte("not a chunk list"))
	chunked.BlobHash = &syncpb.ContentHash{Value: h[:]}
	w.push(t, web, "main", map[string]string{
		"bin/blob.dat": "needle\x00\x01\x02",
		"big.txt":      big,
		"small.txt":    "a needle here\n",
	}, chunked)
	res := w.search(t, Query{Pattern: "needle"}, Target{TenantSlug: "acme", Tree: web})
	eq(t, locs(res), []string{"web:small.txt:1"})
	m, ok := w.ix.Status(web.ID)
	if !ok {
		t.Fatal("no status")
	}
	eq(t, []int{m.Files, m.SkippedBinary, m.SkippedLarge, m.SkippedChunked}, []int{1, 1, 1, 1})
	if w.objs.count(big) != 0 {
		t.Fatal("a file over MaxFileSize was read")
	}
	var stored Meta
	must(t, w.store.GetDoc(context.Background(), MetaCollection, web.ID, &stored))
	eq(t, stored.SnapshotID, m.SnapshotID)
	eq(t, stored.Files, 1)
}

func TestLazyAndIncrementalIndexing(t *testing.T) {
	w := newWorld(t)
	ctx := context.Background()
	web := w.tree(t, "acme", "web")
	w.push(t, web, "main", map[string]string{"a.txt": "alpha one\n", "b.txt": "beta one\n", "c.bin": "x\x00"})
	tg := Target{TenantSlug: "acme", Tree: web}

	// Nothing is indexed before the first search; pushes to trees that
	// were never searched are ignored.
	must(t, w.ix.HandleEvent(ctx, domain.Event{Type: domain.EventBranchPushed, TreeID: web.ID, Payload: map[string]any{"branch": "main"}}))
	if w.ix.Indexed(web.ID) || w.objs.count("alpha one\n") != 0 {
		t.Fatal("tree indexed before any search")
	}
	eq(t, len(w.search(t, Query{Pattern: "one"}, tg).Hits), 2)
	eq(t, w.objs.count("alpha one\n"), 1)

	// Repeated searches reuse the index.
	w.search(t, Query{Pattern: "one"}, tg)
	eq(t, w.objs.count("alpha one\n"), 1)

	// A push to another branch does not touch the index.
	w.push(t, web, "dev", map[string]string{"a.txt": "alpha dev\n"})
	must(t, w.ix.HandleEvent(ctx, domain.Event{Type: domain.EventBranchPushed, TreeID: web.ID, Payload: map[string]any{"branch": "dev"}}))
	eq(t, w.objs.count("alpha dev\n"), 0)

	// A push to the default branch rebuilds incrementally: unchanged files
	// (including the skipped binary) are not read again.
	snap := w.push(t, web, "main", map[string]string{"a.txt": "alpha one\n", "b.txt": "beta two\n", "c.bin": "x\x00", "d.txt": "delta one\n"})
	must(t, w.ix.HandleEvent(ctx, domain.Event{Type: domain.EventBranchPushed, TreeID: web.ID, Payload: map[string]any{"branch": "main"}}))
	m, _ := w.ix.Status(web.ID)
	eq(t, []any{m.SnapshotID, m.Files, m.Reused, m.SkippedBinary}, []any{snap, 3, 1, 1})
	eq(t, w.objs.count("alpha one\n"), 1)
	eq(t, w.objs.count("x\x00"), 1)
	eq(t, w.objs.count("beta two\n"), 1)
	eq(t, locs(w.search(t, Query{Pattern: "one"}, tg)), []string{"web:a.txt:1", "web:d.txt:1"})
	eq(t, locs(w.search(t, Query{Pattern: "two"}, tg)), []string{"web:b.txt:1"})

	// A missed event is caught up by the next search (stale tip check).
	w.push(t, web, "main", map[string]string{"a.txt": "alpha three\n"})
	eq(t, locs(w.search(t, Query{Pattern: "three"}, tg)), []string{"web:a.txt:1"})
	eq(t, w.search(t, Query{Pattern: "one"}, tg).Hits, []domain.CodeSearchHit{})

	// tree.deleted drops the index and its metadata.
	must(t, w.ix.HandleEvent(ctx, domain.Event{Type: domain.EventTreeDeleted, TreeID: web.ID}))
	if w.ix.Indexed(web.ID) {
		t.Fatal("index kept after tree.deleted")
	}
	var stored Meta
	if err := w.store.GetDoc(ctx, MetaCollection, web.ID, &stored); !errors.Is(err, domain.ErrNotFound) {
		t.Fatalf("metadata kept after tree.deleted: %v", err)
	}
}

func TestStartSubscribesToBus(t *testing.T) {
	w := newWorld(t)
	bus := events.NewBus()
	w.ix.d.Bus = bus
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	w.ix.Start(ctx)
	web := w.tree(t, "acme", "web")
	w.push(t, web, "main", map[string]string{"a.txt": "alpha one\n"})
	tg := Target{TenantSlug: "acme", Tree: web}
	w.search(t, Query{Pattern: "one"}, tg)

	snap := w.push(t, web, "main", map[string]string{"a.txt": "alpha two\n"})
	bus.Publish(domain.Event{Type: domain.EventBranchPushed, TreeID: web.ID, Payload: map[string]any{"branch": "main"}})
	waitFor(t, func() bool { m, _ := w.ix.Status(web.ID); return m.SnapshotID == snap })
	bus.Publish(domain.Event{Type: domain.EventTreeDeleted, TreeID: web.ID})
	waitFor(t, func() bool { return !w.ix.Indexed(web.ID) })
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

func TestEmptyAndMissingBranches(t *testing.T) {
	w := newWorld(t)
	web := w.tree(t, "acme", "web")
	res := w.search(t, Query{Pattern: "anything"}, Target{TenantSlug: "acme", Tree: web})
	eq(t, []any{len(res.Hits), res.SearchedTrees}, []any{0, 1})

	// A tree whose manifest is missing is skipped, not fatal.
	broken := w.tree(t, "acme", "broken")
	ctx := context.Background()
	must(t, w.store.PutSnapshot(ctx, &domain.Snapshot{ID: "dangling", TreeID: broken.ID, BranchName: "main", ManifestHash: hash.Sum([]byte("gone")), CreatedAt: t0}))
	must(t, w.store.CompareAndSwapBranchTip(ctx, broken.ID, "main", "", "dangling"))
	w.push(t, web, "main", map[string]string{"a.txt": "found it\n"})
	res = w.search(t, Query{Pattern: "found"}, Target{TenantSlug: "acme", Tree: broken}, Target{TenantSlug: "acme", Tree: web})
	eq(t, []any{locs(res), res.SearchedTrees}, []any{[]string{"web:a.txt:1"}, 1})
}

func TestLimitsOrderingAndBudget(t *testing.T) {
	w := newWorld(t)
	web := w.tree(t, "acme", "web")
	api := w.tree(t, "acme", "api")
	other := w.tree(t, "beta", "app")
	var lines strings.Builder
	for i := 0; i < 30; i++ {
		lines.WriteString("match line\n")
	}
	for _, tr := range []*domain.Tree{web, api, other} {
		w.push(t, tr, "main", map[string]string{"z.txt": "match z\n", "a.txt": lines.String()})
	}
	targets := []Target{{TenantSlug: "beta", Tree: other}, {TenantSlug: "acme", Tree: web}, {TenantSlug: "acme", Tree: api}}

	res := w.search(t, Query{Pattern: "match", Limit: 31}, targets...)
	eq(t, []any{len(res.Hits), res.Truncated, res.SearchedTrees}, []any{31, true, 2})
	eq(t, locs(res)[29:], []string{"api:a.txt:30", "api:z.txt:1"})

	res = w.search(t, Query{Pattern: "match", Limit: 93}, targets...)
	eq(t, []any{len(res.Hits), res.Truncated, res.SearchedTrees}, []any{93, false, 3})
	eq(t, res.Hits[92].TenantSlug, "beta")

	res = w.search(t, Query{Pattern: "match"}, targets...)
	eq(t, []any{len(res.Hits), res.Truncated}, []any{DefaultLimit, true})
	res = w.search(t, Query{Pattern: "match", Limit: 5000}, targets...)
	eq(t, len(res.Hits), 93) // clamped to MaxLimit, all found

	// An exhausted budget returns what was found, marked truncated.
	res = w.search(t, Query{Pattern: "match", Budget: time.Nanosecond}, targets...)
	eq(t, res.Truncated, true)
}

func TestReadFilter(t *testing.T) {
	w := newWorld(t)
	web := w.tree(t, "acme", "web")
	files := map[string]string{}
	for i := 0; i < 2*filterBatch+3; i++ {
		files[fmt.Sprintf("f%03d.txt", i)] = "secret token\n"
	}
	files["secret/key.txt"] = "secret token\n"
	w.push(t, web, "main", files)
	var calls atomic.Int32
	var seen []string
	filter := func(_ context.Context, tree *domain.Tree, paths []string) (func(string) bool, error) {
		calls.Add(1)
		if tree.ID != web.ID {
			t.Errorf("filter called for %s", tree.ID)
		}
		seen = append(seen, paths...)
		return func(p string) bool { return strings.HasPrefix(p, "secret/") || p == "f001.txt" }, nil
	}
	res, err := w.ix.Search(context.Background(), Query{Pattern: "token", Limit: MaxLimit}, []Target{{TenantSlug: "acme", Tree: web}}, filter)
	must(t, err)
	eq(t, len(res.Hits), 2*filterBatch+2)
	for _, h := range res.Hits {
		if h.Path == "f001.txt" || strings.HasPrefix(h.Path, "secret/") {
			t.Fatalf("restricted hit %s returned", h.Path)
		}
	}
	eq(t, int(calls.Load()), 3) // batched
	eq(t, len(seen), 2*filterBatch+4)

	// Restricted hits do not count towards the limit.
	res, err = w.ix.Search(context.Background(), Query{Pattern: "token", Limit: 2}, []Target{{TenantSlug: "acme", Tree: web}}, filter)
	must(t, err)
	eq(t, locs(res), []string{"web:f000.txt:1", "web:f002.txt:1"})
	eq(t, res.Truncated, true)

	// Filter errors fail the search.
	boom := errors.New("boom")
	_, err = w.ix.Search(context.Background(), Query{Pattern: "token"}, []Target{{TenantSlug: "acme", Tree: web}},
		func(context.Context, *domain.Tree, []string) (func(string) bool, error) { return nil, boom })
	if !errors.Is(err, boom) {
		t.Fatalf("err = %v, want boom", err)
	}
}

func TestLongLinesClipped(t *testing.T) {
	w := newWorld(t)
	web := w.tree(t, "acme", "web")
	w.push(t, web, "main", map[string]string{"min.js": strings.Repeat("é", MaxLineLen) + "needle\n"})
	res := w.search(t, Query{Pattern: "needle"}, Target{TenantSlug: "acme", Tree: web})
	eq(t, len(res.Hits), 1)
	if n := len(res.Hits[0].Text); n > MaxLineLen || n < MaxLineLen-1 {
		t.Fatalf("text is %d bytes", n)
	}
	if !strings.HasPrefix(res.Hits[0].Text, "éé") || strings.ContainsRune(res.Hits[0].Text, '�') {
		t.Fatal("clipped inside a character")
	}
}

// ---- trigram prefilter ----

func trigramsOf(ss ...string) []trigram {
	var out []trigram
	for _, s := range ss {
		out = append(out, stringTrigrams(s)...)
	}
	return out
}

func TestRegexpQueryShapes(t *testing.T) {
	parse := func(expr string, flags syntax.Flags) *tquery {
		re, err := syntax.Parse(expr, syntax.Perl|flags)
		must(t, err)
		return regexpQuery(re)
	}
	if q := parse(`abcdef`, 0); q.op != qAnd || !reflect.DeepEqual(q.tri, trigramsOf("abcdef")) {
		t.Fatalf("literal: %+v", q)
	}
	if q := parse(`ab`, 0); q.op != qAll {
		t.Fatalf("short literal: %+v", q)
	}
	if q := parse(`.*`, 0); q.op != qAll {
		t.Fatalf(".*: %+v", q)
	}
	// Alternation inside a concatenation yields exact strings.
	q := parse(`foo(bar|baz)`, 0)
	if q.op != qOr || len(q.sub) != 2 {
		t.Fatalf("foo(bar|baz): %+v", q)
	}
	// Case folding of "k" adds the Kelvin sign alternative.
	q = parse(`ok!`, syntax.FoldCase)
	if q.op != qOr || !reflect.DeepEqual(q.tri, trigramsOf("ok!")) || len(q.sub) != 1 {
		t.Fatalf("(?i)ok!: %+v", q)
	}
	if q := parse(`[^a]`, 0); q.op != qAll {
		t.Fatalf("[^a]: %+v", q)
	}
	if q := parse(`[^\x00-\x{10FFFF}]abc`, 0); q.op != qNone {
		t.Fatalf("empty class: %+v", q)
	}
}

// TestPrefilterHasNoFalseNegatives compares prefiltered search with a
// brute-force scan over many patterns.
func TestPrefilterHasNoFalseNegatives(t *testing.T) {
	w := newWorld(t)
	web := w.tree(t, "acme", "web")
	corpus := map[string]string{
		"a.txt": "Hello World\nfoobar\nfoo-baz\nKELVIN 5K\n",
		"b.txt": "hello\tworld\nABCabc\nstraße STRASSE\n",
		"c.txt": "x=1;y=22;z=333\nfunc Foo() {}\nreturn nil\n",
		"d.txt": "ſtate\nσς Σ\nfoobaz\n",
		"e.txt": "\n\nabc\n",
	}
	w.push(t, web, "main", corpus)
	tg := Target{TenantSlug: "acme", Tree: web}
	patterns := []string{
		`hello`, `HELLO world`, `foo(bar|baz)`, `fo+ba[rz]`, `foo.?baz`, `^abc$`, `\d{2,}`,
		`kelvin \d.`, `5k`, `stat`, `STATE`, `σ`, `Σς`, `func \w+\(\)`, `x=1;y`, `(abc)+`,
		`a|b`, `wor(ld)?`, `[A-C]{3}`, `return\s+nil`, `^$`, `\x{212A}`, `strasse`, `[sS]tra`,
	}
	for _, p := range patterns {
		for _, cs := range []bool{false, true} {
			expr := p
			if !cs {
				expr = "(?i)" + p
			}
			re := regexp.MustCompile(expr)
			var want []string
			names := make([]string, 0, len(corpus))
			for n := range corpus {
				names = append(names, n)
			}
			sort.Strings(names)
			for _, n := range names {
				for i, line := range splitLines([]byte(corpus[n])) {
					if re.Match(line) {
						want = append(want, fmt.Sprintf("web:%s:%d", n, i+1))
					}
				}
			}
			if want == nil {
				want = []string{}
			}
			res := w.search(t, Query{Pattern: p, Regex: true, CaseSensitive: cs, Limit: MaxLimit}, tg)
			if got := locs(res); !reflect.DeepEqual(got, want) {
				t.Errorf("%q case=%v: got %v, want %v", p, cs, got, want)
			}
			if len([]rune(p)) >= MinLiteral && !strings.ContainsAny(p, `\.?+*()[]{}|^$`) {
				lit := w.search(t, Query{Pattern: p, CaseSensitive: cs, Limit: MaxLimit}, tg)
				if got := locs(lit); !reflect.DeepEqual(got, want) {
					t.Errorf("literal %q case=%v: got %v, want %v", p, cs, got, want)
				}
			}
		}
	}
}
