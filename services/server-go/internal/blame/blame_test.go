package blame

import (
	"context"
	"errors"
	"fmt"
	"sort"
	"strings"
	"testing"
	"time"

	"google.golang.org/protobuf/proto"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
	"github.com/seanfilimon/worktree/services/server-go/internal/objstore"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
)

var t0 = time.Date(2026, 1, 2, 3, 4, 5, 0, time.UTC)

// fixture builds snapshot histories in a memory store and a disk object
// store.
type fixture struct {
	t     *testing.T
	ctx   context.Context
	store *meta.Memory
	objs  *objstore.DiskStore
	n     int
	snaps map[string]*domain.Snapshot
}

func newFixture(t *testing.T) *fixture {
	return &fixture{t: t, ctx: context.Background(), store: meta.NewMemory(), objs: objstore.New(t.TempDir()), snaps: map[string]*domain.Snapshot{}}
}

func (f *fixture) put(data []byte) hash.Hash {
	f.t.Helper()
	h := hash.Sum(data)
	if err := f.objs.Put(f.ctx, h, data); err != nil {
		f.t.Fatal(err)
	}
	return h
}

func (f *fixture) blobEntry(path string, data []byte) *syncpb.ManifestEntry {
	h := f.put(data)
	return &syncpb.ManifestEntry{Path: path, BlobHash: &syncpb.ContentHash{Value: h[:]}, Size: uint64(len(data))}
}

// chunkedEntry stores data as a CHUNK_LIST of parts of at most n bytes.
func (f *fixture) chunkedEntry(path string, data []byte, n int) *syncpb.ManifestEntry {
	f.t.Helper()
	cl := &syncpb.ChunkList{Size: uint64(len(data))}
	for off := 0; off < len(data); off += n {
		part := data[off:min(off+n, len(data))]
		h := f.put(part)
		cl.Chunks = append(cl.Chunks, &syncpb.Chunk{Hash: &syncpb.ContentHash{Value: h[:]}, Size: uint64(len(part))})
	}
	raw, err := proto.Marshal(cl)
	if err != nil {
		f.t.Fatal(err)
	}
	h := f.put(raw)
	return &syncpb.ManifestEntry{Path: path, BlobHash: &syncpb.ContentHash{Value: h[:]}, Size: uint64(len(data)), Chunked: true}
}

// snap stores a snapshot named name with the given files and parents.
func (f *fixture) snap(name string, files map[string]string, parents ...string) *domain.Snapshot {
	return f.snapEntries(name, f.entries(files), parents...)
}

func (f *fixture) entries(files map[string]string) []*syncpb.ManifestEntry {
	var es []*syncpb.ManifestEntry
	for p, c := range files {
		es = append(es, f.blobEntry(p, []byte(c)))
	}
	return es
}

func (f *fixture) snapEntries(name string, entries []*syncpb.ManifestEntry, parents ...string) *domain.Snapshot {
	f.t.Helper()
	sort.Slice(entries, func(i, j int) bool { return entries[i].GetPath() < entries[j].GetPath() })
	raw, err := proto.Marshal(&syncpb.ManifestRecord{Entries: entries})
	if err != nil {
		f.t.Fatal(err)
	}
	f.n++
	s := &domain.Snapshot{
		ID: "snap-" + name, TreeID: "tree", BranchName: "main", Parents: parents,
		ManifestHash: f.put(raw), Author: "Author " + name + " <" + name + "@example.com>",
		Message: name + "\n\nbody", CreatedAt: t0.Add(time.Duration(f.n) * time.Minute),
	}
	if err := f.store.PutSnapshot(f.ctx, s); err != nil {
		f.t.Fatal(err)
	}
	f.snaps[name] = s
	return s
}

func (f *fixture) blame(tip *domain.Snapshot, path string, o Options) []domain.BlameLine {
	f.t.Helper()
	lines, err := New(f.store, f.objs).File(f.ctx, tip, path, o)
	if err != nil {
		f.t.Fatalf("blame %s at %s: %v", path, tip.ID, err)
	}
	return lines
}

// owners renders lines as "text:snapshot" pairs.
func owners(lines []domain.BlameLine) string {
	parts := make([]string, len(lines))
	for i, l := range lines {
		parts[i] = fmt.Sprintf("%d:%s:%s", l.Line, l.Text, strings.TrimPrefix(l.SnapshotID, "snap-"))
	}
	return strings.Join(parts, " ")
}

func want(t *testing.T, got []domain.BlameLine, w string) {
	t.Helper()
	if g := owners(got); g != w {
		t.Fatalf("blame =\n  %s\nwant\n  %s", g, w)
	}
}

func TestLinearEditsInsertionsDeletions(t *testing.T) {
	f := newFixture(t)
	f.snap("s1", map[string]string{"f.txt": "a\nb\nc\nd\n"})
	f.snap("s2", map[string]string{"f.txt": "a\nB\nc\nd\n"}, "snap-s1")
	f.snap("s3", map[string]string{"f.txt": "a\nB\nnew1\nnew2\nc\nd\n", "other": "x"}, "snap-s2")
	f.snap("s4", map[string]string{"f.txt": "a\nB\nnew1\nnew2\nd\n", "other": "y"}, "snap-s3")
	s5 := f.snap("s5", map[string]string{"f.txt": "a\nB\nnew1\nnew2\nd\n", "other": "z"}, "snap-s4")
	got := f.blame(s5, "f.txt", Options{})
	want(t, got, "1:a:s1 2:B:s2 3:new1:s3 4:new2:s3 5:d:s1")
	l := got[0]
	if l.Author != "Author s1 <s1@example.com>" || l.Message != "s1" || !l.At.Equal(f.snaps["s1"].CreatedAt) {
		t.Fatalf("metadata = %+v", l)
	}
	want(t, f.blame(f.snaps["s2"], "f.txt", Options{}), "1:a:s1 2:B:s2 3:c:s1 4:d:s1")
}

// TestSharedScenario is the shared Go/Rust scenario (keep in sync with
// crates/worktree-sdk/tests/blame.rs
// `shared_scenario_merge_follows_the_merged_branch`):
//
//	S1 base     a.txt = one two three
//	S2 feature  (branch from S1) one two three feature
//	S3 top      (main) zero one two three
//	S4 merge    feature into main: zero one two three feature
//	S5 edit     zero one TWO feature
//	blame a.txt at S5 = zero:S3 one:S1 TWO:S5 feature:S2
func TestSharedScenario(t *testing.T) {
	f := newFixture(t)
	f.snap("base", map[string]string{"a.txt": "one\ntwo\nthree\n"})
	f.snap("feature", map[string]string{"a.txt": "one\ntwo\nthree\nfeature\n"}, "snap-base")
	f.snap("top", map[string]string{"a.txt": "zero\none\ntwo\nthree\n"}, "snap-base")
	merge := f.snap("merge", map[string]string{"a.txt": "zero\none\ntwo\nthree\nfeature\n"}, "snap-top", "snap-feature")
	edit := f.snap("edit", map[string]string{"a.txt": "zero\none\nTWO\nfeature\n"}, "snap-merge")
	want(t, f.blame(edit, "a.txt", Options{}), "1:zero:top 2:one:base 3:TWO:edit 4:feature:feature")
	// At the merge itself every line belongs to a parent's history.
	want(t, f.blame(merge, "a.txt", Options{}), "1:zero:top 2:one:base 3:two:base 4:three:base 5:feature:feature")
}

func TestMergeResolutionAndIdenticalParent(t *testing.T) {
	f := newFixture(t)
	f.snap("base", map[string]string{"m.txt": "1\n2\n3\n"})
	f.snap("feat", map[string]string{"m.txt": "1\nfeature\n3\n"}, "snap-base")
	f.snap("main", map[string]string{"m.txt": "1\nmain\n3\n"}, "snap-base")
	resolved := f.snap("merge", map[string]string{"m.txt": "1\nresolved\n3\n"}, "snap-main", "snap-feat")
	want(t, f.blame(resolved, "m.txt", Options{}), "1:1:base 2:resolved:merge 3:3:base")
	// A merge taking the second parent's version unchanged passes all lines
	// to that parent.
	theirs := f.snap("theirs", map[string]string{"m.txt": "1\nfeature\n3\n"}, "snap-main", "snap-feat")
	want(t, f.blame(theirs, "m.txt", Options{}), "1:1:base 2:feature:feat 3:3:base")
}

func TestRenamesAreNotFollowed(t *testing.T) {
	f := newFixture(t)
	f.snap("create", map[string]string{"old.txt": "p\nq\n"})
	s2 := f.snap("rename", map[string]string{"new.txt": "p\nq\n"}, "snap-create")
	want(t, f.blame(s2, "new.txt", Options{}), "1:p:rename 2:q:rename")
	if _, err := New(f.store, f.objs).File(f.ctx, s2, "old.txt", Options{}); !errors.Is(err, ErrNoFile) || !errors.Is(err, domain.ErrNotFound) {
		t.Fatalf("old path at HEAD: %v", err)
	}
}

func TestRanges(t *testing.T) {
	f := newFixture(t)
	f.snap("s1", map[string]string{"r.txt": "1\n2\n3\n4\n5\n"})
	s2 := f.snap("s2", map[string]string{"r.txt": "1\n2\nthree\n4\n5\n"}, "snap-s1")
	want(t, f.blame(s2, "r.txt", Options{Start: 2, End: 3}), "2:2:s1 3:three:s2")
	want(t, f.blame(s2, "r.txt", Options{Start: 4, End: 99}), "4:4:s1 5:5:s1")
	want(t, f.blame(s2, "r.txt", Options{Start: 5}), "5:5:s1")
	for _, o := range []Options{{Start: 3, End: 2}, {Start: 6, End: 7}, {Start: -1, End: 2}} {
		if _, err := New(f.store, f.objs).File(f.ctx, s2, "r.txt", o); !errors.Is(err, ErrRange) {
			t.Fatalf("%+v: %v", o, err)
		}
	}
}

func TestBinaryEmptyAndLineEndings(t *testing.T) {
	f := newFixture(t)
	s1 := f.snap("s1", map[string]string{"bin": "\x00\x01", "empty": "", "crlf": "a\r\nb"})
	if _, err := New(f.store, f.objs).File(f.ctx, s1, "bin", Options{}); !errors.Is(err, ErrBinary) {
		t.Fatalf("binary: %v", err)
	}
	if got := f.blame(s1, "empty", Options{}); len(got) != 0 {
		t.Fatalf("empty: %+v", got)
	}
	want(t, f.blame(s1, "crlf", Options{}), "1:a:s1 2:b:s1")
	// A version that was binary in the parent gives the parent no lines.
	s2 := f.snap("s2", map[string]string{"bin": "a\n"}, "snap-s1")
	want(t, f.blame(s2, "bin", Options{}), "1:a:s2")
}

func TestChunkedFiles(t *testing.T) {
	f := newFixture(t)
	var sb strings.Builder
	for i := 0; i < 200; i++ {
		fmt.Fprintf(&sb, "line %03d\n", i)
	}
	text := sb.String()
	f.snapEntries("s1", []*syncpb.ManifestEntry{f.chunkedEntry("big", []byte(text), 300)})
	edited := strings.Replace(text, "line 010\n", "changed\n", 1)
	s2 := f.snapEntries("s2", []*syncpb.ManifestEntry{f.chunkedEntry("big", []byte(edited), 300)}, "snap-s1")
	want(t, f.blame(s2, "big", Options{Start: 10, End: 12}), "10:line 009:s1 11:changed:s2 12:line 011:s1")
}

func TestMissingParentIsAnError(t *testing.T) {
	f := newFixture(t)
	s := f.snap("orphan", map[string]string{"a": "x\n"}, "snap-nope")
	if _, err := New(f.store, f.objs).File(f.ctx, s, "a", Options{}); !errors.Is(err, ErrIncomplete) || errors.Is(err, domain.ErrNotFound) {
		t.Fatalf("missing parent: %v", err)
	}
}

func TestCancelledContext(t *testing.T) {
	f := newFixture(t)
	f.snap("s1", map[string]string{"a": "x\n"})
	s2 := f.snap("s2", map[string]string{"a": "x\ny\n"}, "snap-s1")
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if _, err := New(f.store, f.objs).File(ctx, s2, "a", Options{}); !errors.Is(err, context.Canceled) {
		t.Fatalf("cancelled: %v", err)
	}
}

func TestLongHistoryStopsWhenAttributed(t *testing.T) {
	f := newFixture(t)
	prev := f.snap("s0", map[string]string{"a": "keep\n", "n": "0"})
	for i := 1; i <= 100; i++ {
		prev = f.snap(fmt.Sprint("s", i), map[string]string{"a": "keep\n", "n": fmt.Sprint(i)}, prev.ID)
	}
	tip := f.snap("tip", map[string]string{"a": "keep\nnew\n"}, prev.ID)
	want(t, f.blame(tip, "a", Options{Start: 2}), "2:new:tip")
	want(t, f.blame(tip, "a", Options{}), "1:keep:s0 2:new:tip")
}
