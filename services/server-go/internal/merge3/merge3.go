// Package merge3 implements server-side three-way merges of manifests and
// text files for merge requests (docs/ENGINEERING_PLAN.md §14.1,
// specs/server/Merging.md).
//
// The semantics follow the Rust SDK's merge engine
// (crates/worktree-sdk/src/merge) so a merge made by the server equals the
// one `wt merge` would make locally:
//
//   - Manifests are merged path by path: a path unchanged on one side takes
//     the other side, identical changes are taken once, a deletion against
//     an unchanged path deletes it. Content and the executable bit merge
//     independently.
//   - When both sides changed a text file it is merged line-wise with diff3
//     (Text); overlapping edits are a "content" conflict.
//   - Binary files, chunked (large) files, add/add with different content or
//     different executable bits, modify/delete, and a file that the merge
//     would place where the other side has a directory are conflicts. The
//     server never writes conflict markers: a conflicting merge is refused
//     and resolved locally.
//
// Everything here is a pure function over an ObjectReader; nothing is
// written. Merge returns the merged manifest plus the new blobs the caller
// must store.
package merge3

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"slices"
	"strings"

	"github.com/seanfilimon/worktree/services/server-go/internal/content"
	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
	"github.com/seanfilimon/worktree/services/server-go/internal/textdiff"
)

// Conflict kinds. The first four match the Rust SDK's ConflictKind
// serialization.
const (
	KindContent       = "content"        // both sides changed overlapping lines of a text file
	KindModifyDelete  = "modify_delete"  // one side modified, the other deleted
	KindAddAdd        = "add_add"        // both sides added the path with different content
	KindBinary        = "binary"         // a binary file changed on both sides
	KindChunked       = "chunked"        // a chunked (large) file changed on both sides
	KindExecutable    = "executable"     // both sides added the same content with different executable bits
	KindFileDirectory = "file_directory" // the merged tree would hold a file and a directory at one path
)

// Conflict is one path the merge cannot resolve.
type Conflict struct {
	Path string `json:"path"`
	Kind string `json:"kind"`
}

// ObjectReader reads stored objects (objstore.Store implements it).
type ObjectReader interface {
	Get(ctx context.Context, h hash.Hash) ([]byte, error)
}

// Result is the outcome of Merge.
type Result struct {
	// Manifest is the merged manifest, entries sorted by path. Conflicting
	// paths keep ours' entry (theirs' when ours deleted it); it is only
	// meaningful when the merge is clean.
	Manifest *syncpb.ManifestRecord
	// Conflicts lists the unresolvable paths, sorted by path.
	Conflicts []Conflict
	// Blobs holds the content of text files merged line-wise, keyed by
	// hash. They are not in the object store yet.
	Blobs map[hash.Hash][]byte
}

// Clean reports whether the merge has no conflicts.
func (r *Result) Clean() bool { return len(r.Conflicts) == 0 }

// ErrMalformed reports a manifest entry without a valid hash.
var ErrMalformed = errors.New("merge3: malformed manifest entry")

// Merge three-way merges the manifests ours and theirs against base (nil
// manifests are empty). Text files changed on both sides are read through
// objs and merged line-wise. Errors are operational (unreadable objects,
// malformed entries) or the context's; conflicts are reported in the
// result.
func Merge(ctx context.Context, objs ObjectReader, base, ours, theirs *syncpb.ManifestRecord) (*Result, error) {
	bm, err := byPath(base)
	if err != nil {
		return nil, err
	}
	om, err := byPath(ours)
	if err != nil {
		return nil, err
	}
	tm, err := byPath(theirs)
	if err != nil {
		return nil, err
	}
	paths := make([]string, 0, len(om)+len(tm))
	for p := range om {
		paths = append(paths, p)
	}
	for p := range tm {
		if _, ok := om[p]; !ok {
			paths = append(paths, p)
		}
	}
	slices.Sort(paths)

	res := &Result{Manifest: &syncpb.ManifestRecord{}, Conflicts: []Conflict{}, Blobs: map[hash.Hash][]byte{}}
	conflict := func(path, kind string, keep *syncpb.ManifestEntry) {
		res.Conflicts = append(res.Conflicts, Conflict{Path: path, Kind: kind})
		if keep != nil {
			res.Manifest.Entries = append(res.Manifest.Entries, keep)
		}
	}
	for i, p := range paths {
		if i%256 == 0 {
			if err := ctx.Err(); err != nil {
				return nil, err
			}
		}
		b, o, t := bm[p], om[p], tm[p]
		r := resolve(b, o, t)
		switch r.action {
		case actKeep:
			res.Manifest.Entries = append(res.Manifest.Entries, r.entry)
		case actDelete:
		case actConflict:
			conflict(p, r.kind, firstNonNil(o, t))
		case actMergeContent:
			e, kind, err := mergeContent(ctx, objs, b, o, t, res.Blobs)
			if err != nil {
				return nil, err
			}
			if kind != "" {
				conflict(p, kind, o)
				continue
			}
			res.Manifest.Entries = append(res.Manifest.Entries, e)
		}
	}
	res.Conflicts = append(res.Conflicts, fileDirectoryClashes(res.Manifest.Entries)...)
	slices.SortStableFunc(res.Conflicts, func(a, b Conflict) int { return strings.Compare(a.Path, b.Path) })
	res.Conflicts = slices.CompactFunc(res.Conflicts, func(a, b Conflict) bool { return a.Path == b.Path })
	return res, nil
}

// mergeContent merges a text file both sides changed. It returns the merged
// entry, or the conflict kind.
func mergeContent(ctx context.Context, objs ObjectReader, b, o, t *syncpb.ManifestEntry, blobs map[hash.Hash][]byte) (*syncpb.ManifestEntry, string, error) {
	if b.GetChunked() || o.GetChunked() || t.GetChunked() {
		return nil, KindChunked, nil
	}
	read := func(e *syncpb.ManifestEntry) ([]byte, error) {
		h, err := content.EntryHash(e)
		if err != nil {
			return nil, fmt.Errorf("%w: %v", ErrMalformed, err)
		}
		data, err := objs.Get(ctx, h)
		if err != nil {
			return nil, fmt.Errorf("merge3: read %s of %q: %w", h.Hex(), e.GetPath(), err)
		}
		return data, nil
	}
	var sides [3][]byte
	for i, e := range []*syncpb.ManifestEntry{b, o, t} {
		data, err := read(e)
		if err != nil {
			return nil, "", err
		}
		if textdiff.IsBinary(data) {
			return nil, KindBinary, nil
		}
		sides[i] = data
	}
	r := Text(string(sides[0]), string(sides[1]), string(sides[2]), DefaultLabels())
	if !r.Clean() {
		return nil, KindContent, nil
	}
	data := []byte(r.Merged)
	h := hash.Sum(data)
	if !bytes.Equal(h[:], o.GetBlobHash().GetValue()) && !bytes.Equal(h[:], t.GetBlobHash().GetValue()) {
		blobs[h] = data
	}
	return &syncpb.ManifestEntry{
		Path:       o.GetPath(),
		BlobHash:   &syncpb.ContentHash{Value: h[:]},
		Size:       uint64(len(data)),
		Executable: mergeExecutable(b, o, t),
	}, "", nil
}

// action is what resolve decided for one path.
type action int

const (
	actKeep         action = iota // keep entry (ours, theirs or a combination)
	actDelete                     // drop the path
	actMergeContent               // both changed a file's content: merge it
	actConflict                   // unresolvable (kind)
)

type resolution struct {
	action action
	entry  *syncpb.ManifestEntry // actKeep
	kind   string                // actConflict
}

// resolve is the per-path resolution table (the Rust SDK's resolve_path,
// plus the executable-bit add/add clash). o or t may be nil (absent); the
// path is present in at least one of them.
func resolve(b, o, t *syncpb.ManifestEntry) resolution {
	keep := func(e *syncpb.ManifestEntry) resolution { return resolution{action: actKeep, entry: e} }
	clash := func(kind string) resolution { return resolution{action: actConflict, kind: kind} }
	switch {
	case o == nil && t == nil:
		return resolution{action: actDelete}
	case o != nil && t != nil && entryEqual(o, t):
		return keep(o)
	case b == nil:
		switch {
		case t == nil:
			return keep(o) // added on our side only
		case o == nil:
			return keep(t) // added on their side only
		case sameContent(o, t):
			return clash(KindExecutable) // same content, different mode
		default:
			return clash(KindAddAdd)
		}
	case t == nil:
		if entryEqual(o, b) {
			return resolution{action: actDelete}
		}
		return clash(KindModifyDelete)
	case o == nil:
		if entryEqual(t, b) {
			return resolution{action: actDelete}
		}
		return clash(KindModifyDelete)
	}
	// Present everywhere.
	exec := mergeExecutable(b, o, t)
	switch {
	case entryEqual(o, b):
		return keep(t)
	case entryEqual(t, b):
		return keep(o)
	case sameContent(o, t) || sameContent(t, b):
		// Content is ours'; only the executable bit can differ.
		return keep(withExecutable(o, exec))
	case sameContent(o, b):
		return keep(withExecutable(t, exec))
	default:
		return resolution{action: actMergeContent}
	}
}

// mergeExecutable merges the executable bit: a side that changed it
// relative to base wins; without a base, or when both agree, ours is kept.
func mergeExecutable(b, o, t *syncpb.ManifestEntry) bool {
	if b != nil && o.GetExecutable() == b.GetExecutable() {
		return t.GetExecutable()
	}
	return o.GetExecutable()
}

func withExecutable(e *syncpb.ManifestEntry, exec bool) *syncpb.ManifestEntry {
	if e.GetExecutable() == exec {
		return e
	}
	return &syncpb.ManifestEntry{
		Path:       e.GetPath(),
		BlobHash:   &syncpb.ContentHash{Value: slices.Clone(e.GetBlobHash().GetValue())},
		Size:       e.GetSize(),
		Executable: exec,
		Chunked:    e.GetChunked(),
	}
}

// sameContent reports whether two entries name the same object.
func sameContent(a, b *syncpb.ManifestEntry) bool {
	return a.GetChunked() == b.GetChunked() && bytes.Equal(a.GetBlobHash().GetValue(), b.GetBlobHash().GetValue())
}

// entryEqual compares every field of two entries.
func entryEqual(a, b *syncpb.ManifestEntry) bool {
	return a.GetPath() == b.GetPath() && sameContent(a, b) &&
		a.GetSize() == b.GetSize() && a.GetExecutable() == b.GetExecutable()
}

func firstNonNil(es ...*syncpb.ManifestEntry) *syncpb.ManifestEntry {
	for _, e := range es {
		if e != nil {
			return e
		}
	}
	return nil
}

// byPath indexes a manifest by path, validating hashes and uniqueness.
func byPath(m *syncpb.ManifestRecord) (map[string]*syncpb.ManifestEntry, error) {
	out := make(map[string]*syncpb.ManifestEntry, len(m.GetEntries()))
	for _, e := range m.GetEntries() {
		if e.GetPath() == "" || len(e.GetBlobHash().GetValue()) != hash.Size {
			return nil, fmt.Errorf("%w: %q", ErrMalformed, e.GetPath())
		}
		if _, dup := out[e.GetPath()]; dup {
			return nil, fmt.Errorf("%w: %q is listed twice", ErrMalformed, e.GetPath())
		}
		out[e.GetPath()] = e
	}
	return out, nil
}

// fileDirectoryClashes reports files of the merged manifest that are also a
// directory prefix of another merged path ("a" next to "a/b"). entries are
// sorted by path.
func fileDirectoryClashes(entries []*syncpb.ManifestEntry) []Conflict {
	files := make(map[string]struct{}, len(entries))
	for _, e := range entries {
		files[e.GetPath()] = struct{}{}
	}
	var out []Conflict
	reported := map[string]bool{}
	for _, e := range entries {
		p := e.GetPath()
		for i := strings.IndexByte(p, '/'); i >= 0; i = nextSlash(p, i) {
			dir := p[:i]
			if _, ok := files[dir]; ok && !reported[dir] {
				reported[dir] = true
				out = append(out, Conflict{Path: dir, Kind: KindFileDirectory})
			}
		}
	}
	return out
}

// nextSlash returns the index of the next '/' in p after position i, or -1.
func nextSlash(p string, i int) int {
	j := strings.IndexByte(p[i+1:], '/')
	if j < 0 {
		return -1
	}
	return i + 1 + j
}
