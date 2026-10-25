package merge3

import (
	"context"
	"errors"
	"fmt"
	"slices"
	"strings"
	"testing"

	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
)

// objects is an in-memory ObjectReader.
type objects map[hash.Hash][]byte

func (o objects) Get(_ context.Context, h hash.Hash) ([]byte, error) {
	d, ok := o[h]
	if !ok {
		return nil, fmt.Errorf("object %s: not found", h.Hex())
	}
	return d, nil
}

// file stores data and returns a manifest entry for it.
func (o objects) file(path, data string, exec bool) *syncpb.ManifestEntry {
	h := hash.Sum([]byte(data))
	o[h] = []byte(data)
	return &syncpb.ManifestEntry{Path: path, BlobHash: &syncpb.ContentHash{Value: h[:]}, Size: uint64(len(data)), Executable: exec}
}

func (o objects) chunked(path, data string) *syncpb.ManifestEntry {
	e := o.file(path, data, false)
	e.Chunked = true
	return e
}

func manifestOf(es ...*syncpb.ManifestEntry) *syncpb.ManifestRecord {
	m := &syncpb.ManifestRecord{Entries: slices.Clone(es)}
	slices.SortFunc(m.Entries, func(a, b *syncpb.ManifestEntry) int { return strings.Compare(a.Path, b.Path) })
	return m
}

// view renders a manifest as path → content for comparisons.
func (o objects) view(t *testing.T, m *syncpb.ManifestRecord) map[string]string {
	t.Helper()
	out := map[string]string{}
	prev := ""
	for i, e := range m.GetEntries() {
		if i > 0 && e.GetPath() <= prev {
			t.Fatalf("manifest not sorted: %q after %q", e.GetPath(), prev)
		}
		prev = e.GetPath()
		var h hash.Hash
		copy(h[:], e.GetBlobHash().GetValue())
		data, ok := o[h]
		if !ok {
			t.Fatalf("%s: object missing", e.GetPath())
		}
		if uint64(len(data)) != e.GetSize() {
			t.Fatalf("%s: size %d, content %d bytes", e.GetPath(), e.GetSize(), len(data))
		}
		v := string(data)
		if e.GetExecutable() {
			v += " [x]"
		}
		out[e.GetPath()] = v
	}
	return out
}

func run(t *testing.T, o objects, base, ours, theirs *syncpb.ManifestRecord) *Result {
	t.Helper()
	r, err := Merge(context.Background(), o, base, ours, theirs)
	if err != nil {
		t.Fatal(err)
	}
	for h, d := range r.Blobs {
		if hash.Sum(d) != h {
			t.Fatal("blob stored under the wrong hash")
		}
		o[h] = d
	}
	return r
}

func TestResolveTable(t *testing.T) {
	o := objects{}
	a := o.file("p", "a\n", false)
	ax := o.file("p", "a\n", true)
	b := o.file("p", "b\n", false)
	c := o.file("p", "c\n", false)
	tests := []struct {
		name          string
		base, ou, the *syncpb.ManifestEntry
		act           action
		keep          *syncpb.ManifestEntry
		kind          string
	}{
		{name: "deleted on both sides", base: a, act: actDelete},
		{name: "identical", base: a, ou: a, the: a, act: actKeep, keep: a},
		{name: "added identically", ou: b, the: b, act: actKeep, keep: b},
		{name: "added ours", ou: b, act: actKeep, keep: b},
		{name: "added theirs", the: b, act: actKeep, keep: b},
		{name: "add/add", ou: b, the: c, act: actConflict, kind: KindAddAdd},
		{name: "add/add mode", ou: a, the: ax, act: actConflict, kind: KindExecutable},
		{name: "deleted ours, unchanged theirs", base: a, the: a, act: actDelete},
		{name: "deleted theirs, unchanged ours", base: a, ou: a, act: actDelete},
		{name: "deleted ours, modified theirs", base: a, the: b, act: actConflict, kind: KindModifyDelete},
		{name: "modified ours, deleted theirs", base: a, ou: b, act: actConflict, kind: KindModifyDelete},
		{name: "mode change vs delete", base: a, ou: ax, act: actConflict, kind: KindModifyDelete},
		{name: "changed theirs", base: a, ou: a, the: b, act: actKeep, keep: b},
		{name: "changed ours", base: a, ou: b, the: a, act: actKeep, keep: b},
		{name: "same change", base: a, ou: b, the: b, act: actKeep, keep: b},
		{name: "both changed", base: a, ou: b, the: c, act: actMergeContent},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			r := resolve(tt.base, tt.ou, tt.the)
			if r.action != tt.act || r.kind != tt.kind {
				t.Fatalf("resolve = %+v, want action %d kind %q", r, tt.act, tt.kind)
			}
			if tt.keep != nil && !entryEqual(r.entry, tt.keep) {
				t.Fatalf("kept %v, want %v", r.entry, tt.keep)
			}
		})
	}
}

func TestResolveExecutableBitMergesIndependently(t *testing.T) {
	o := objects{}
	base := o.file("p", "a\n", false)
	modeOnly := o.file("p", "a\n", true)
	content := o.file("p", "b\n", false)
	// Ours toggles the bit, theirs changes the content: changed content with
	// the toggled bit, either way round.
	for _, sides := range [][2]*syncpb.ManifestEntry{{modeOnly, content}, {content, modeOnly}} {
		r := resolve(base, sides[0], sides[1])
		if r.action != actKeep || !sameContent(r.entry, content) || !r.entry.GetExecutable() {
			t.Fatalf("resolve = %+v", r)
		}
	}
	// The input entries are never mutated.
	if content.GetExecutable() || !modeOnly.GetExecutable() {
		t.Fatal("inputs modified")
	}
	eq(t, mergeExecutable(base, base, modeOnly), true)
	eq(t, mergeExecutable(base, modeOnly, base), true)
	eq(t, mergeExecutable(nil, base, modeOnly), false)
	eq(t, mergeExecutable(modeOnly, modeOnly, base), false)
}

func TestMergeManifests(t *testing.T) {
	o := objects{}
	base := manifestOf(
		o.file("keep.txt", "same\n", false),
		o.file("ours-edit.txt", "x\n", false),
		o.file("theirs-edit.txt", "y\n", false),
		o.file("both.txt", "1\n2\n3\n4\n5\n", false),
		o.file("gone-ours.txt", "g\n", false),
		o.file("gone-theirs.txt", "h\n", false),
		o.file("tool.sh", "echo hi\n", false),
	)
	ours := manifestOf(
		o.file("keep.txt", "same\n", false),
		o.file("ours-edit.txt", "X\n", false),
		o.file("theirs-edit.txt", "y\n", false),
		o.file("both.txt", "ONE\n2\n3\n4\n5\n", false),
		o.file("gone-theirs.txt", "h\n", false),
		o.file("tool.sh", "echo hi\n", true),
		o.file("new-ours.txt", "o\n", false),
	)
	theirs := manifestOf(
		o.file("keep.txt", "same\n", false),
		o.file("ours-edit.txt", "x\n", false),
		o.file("theirs-edit.txt", "Y\n", false),
		o.file("both.txt", "1\n2\n3\n4\nFIVE\n", false),
		o.file("gone-ours.txt", "g\n", false),
		o.file("tool.sh", "echo hello\n", false),
		o.file("new-theirs.txt", "t\n", false),
	)
	r := run(t, o, base, ours, theirs)
	if !r.Clean() {
		t.Fatalf("conflicts: %v", r.Conflicts)
	}
	eq(t, o.view(t, r.Manifest), map[string]string{
		"keep.txt":        "same\n",
		"ours-edit.txt":   "X\n",
		"theirs-edit.txt": "Y\n",
		"both.txt":        "ONE\n2\n3\n4\nFIVE\n",
		"tool.sh":         "echo hello\n [x]",
		"new-ours.txt":    "o\n",
		"new-theirs.txt":  "t\n",
	})
	if len(r.Blobs) != 1 {
		t.Fatalf("new blobs = %d, want 1 (both.txt)", len(r.Blobs))
	}

	// Merging is symmetric up to which side is "ours".
	r2 := run(t, o, base, theirs, ours)
	eq(t, o.view(t, r2.Manifest), o.view(t, r.Manifest))
}

func TestMergeConflicts(t *testing.T) {
	o := objects{}
	bin := func(path, s string) *syncpb.ManifestEntry { return o.file(path, "\x00"+s, false) }
	base := manifestOf(
		o.file("text.txt", "a\nb\nc\n", false),
		bin("img.png", "base"),
		o.chunked("big.iso", "base"),
		o.file("md.txt", "m\n", false),
		o.file("fine.txt", "f\n", false),
		o.file("dir", "was a file\n", false),
	)
	ours := manifestOf(
		o.file("text.txt", "a\nours\nc\n", false),
		bin("img.png", "ours"),
		o.chunked("big.iso", "ours"),
		o.file("md.txt", "M\n", false),
		o.file("fine.txt", "F\n", false),
		o.file("added.txt", "mine\n", false),
		o.file("mode.txt", "same\n", true),
		o.file("dir", "was a file\n", false),
	)
	theirs := manifestOf(
		o.file("text.txt", "a\ntheirs\nc\n", false),
		bin("img.png", "theirs"),
		o.chunked("big.iso", "theirs"),
		o.file("fine.txt", "f\n", false),
		o.file("added.txt", "yours\n", false),
		o.file("mode.txt", "same\n", false),
		o.file("dir/inner.txt", "now a directory\n", false),
	)
	r := run(t, o, base, ours, theirs)
	eq(t, r.Conflicts, []Conflict{
		{Path: "added.txt", Kind: KindAddAdd},
		{Path: "big.iso", Kind: KindChunked},
		{Path: "img.png", Kind: KindBinary},
		{Path: "md.txt", Kind: KindModifyDelete},
		{Path: "mode.txt", Kind: KindExecutable},
		{Path: "text.txt", Kind: KindContent},
	})
	if r.Clean() {
		t.Fatal("Clean() with conflicts")
	}
	v := o.view(t, r.Manifest)
	if v["fine.txt"] != "F\n" {
		t.Fatalf("clean path fine.txt = %q", v["fine.txt"])
	}
	// theirs replaced the unchanged file "dir" by a directory: no clash.
	if _, ok := v["dir"]; ok || v["dir/inner.txt"] != "now a directory\n" {
		t.Fatalf("file replaced by a directory: %v", v)
	}
}

func TestMergeFileDirectoryClash(t *testing.T) {
	o := objects{}
	base := manifestOf(o.file("other.txt", "x\n", false))
	ours := manifestOf(o.file("other.txt", "x\n", false), o.file("a", "file\n", false))
	theirs := manifestOf(o.file("other.txt", "x\n", false), o.file("a/b/c.txt", "nested\n", false), o.file("a/d.txt", "d\n", false))
	r := run(t, o, base, ours, theirs)
	eq(t, r.Conflicts, []Conflict{{Path: "a", Kind: KindFileDirectory}})
}

func TestMergeEmptyAndIdentity(t *testing.T) {
	o := objects{}
	r := run(t, o, nil, nil, nil)
	if !r.Clean() || len(r.Manifest.Entries) != 0 {
		t.Fatalf("empty merge = %+v", r)
	}
	m := manifestOf(o.file("a.txt", "a\n", false), o.file("b/c.txt", "c\n", true))
	other := manifestOf(o.file("a.txt", "A\n", false))
	for _, tc := range [][3]*syncpb.ManifestRecord{
		{m, m, m},     // nothing changed
		{m, m, other}, // only theirs changed → theirs
		{m, other, m}, // only ours changed → ours
		{nil, m, nil}, // only ours added everything
		{nil, nil, m}, // only theirs added everything
	} {
		r := run(t, o, tc[0], tc[1], tc[2])
		want := tc[1]
		if tc[1] == nil || (tc[0] != nil && o.view(t, tc[1])["a.txt"] == o.view(t, tc[0])["a.txt"]) {
			want = tc[2]
		}
		if !r.Clean() {
			t.Fatalf("conflicts: %v", r.Conflicts)
		}
		eq(t, o.view(t, r.Manifest), o.view(t, want))
	}
}

func TestMergeErrorsAndContext(t *testing.T) {
	o := objects{}
	base := manifestOf(o.file("a.txt", "a\n", false))
	ours := manifestOf(o.file("a.txt", "b\n", false))
	theirs := manifestOf(o.file("a.txt", "c\n", false))
	empty := objects{}
	if _, err := Merge(context.Background(), empty, base, ours, theirs); err == nil {
		t.Fatal("missing objects: no error")
	}
	bad := manifestOf(&syncpb.ManifestEntry{Path: "x", BlobHash: &syncpb.ContentHash{Value: []byte{1}}})
	if _, err := Merge(context.Background(), o, bad, ours, theirs); !errors.Is(err, ErrMalformed) {
		t.Fatalf("malformed: %v", err)
	}
	dup := &syncpb.ManifestRecord{Entries: []*syncpb.ManifestEntry{ours.Entries[0], ours.Entries[0]}}
	if _, err := Merge(context.Background(), o, base, dup, theirs); !errors.Is(err, ErrMalformed) {
		t.Fatalf("duplicate: %v", err)
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if _, err := Merge(ctx, o, base, ours, theirs); !errors.Is(err, context.Canceled) {
		t.Fatalf("canceled: %v", err)
	}
}

func TestMergeManyPathsStaysSorted(t *testing.T) {
	o := objects{}
	var base, ours, theirs []*syncpb.ManifestEntry
	for i := 0; i < 600; i++ {
		p := fmt.Sprintf("d%d/f%03d.txt", i%7, i)
		base = append(base, o.file(p, "base\n", false))
		switch i % 3 {
		case 0:
			ours = append(ours, o.file(p, "ours\n", false))
			theirs = append(theirs, o.file(p, "base\n", false))
		case 1:
			ours = append(ours, o.file(p, "base\n", false))
			theirs = append(theirs, o.file(p, "theirs\n", false))
		default:
			ours = append(ours, o.file(p, "base\n", false))
		}
	}
	r := run(t, o, manifestOf(base...), manifestOf(ours...), manifestOf(theirs...))
	if !r.Clean() {
		t.Fatal(r.Conflicts)
	}
	v := o.view(t, r.Manifest) // checks ordering
	eq(t, len(v), 400)
}
