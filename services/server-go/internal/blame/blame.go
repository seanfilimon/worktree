// Package blame attributes every line of a file at a snapshot to the
// snapshot that last changed it (docs/ENGINEERING_PLAN.md §14.4). It is the
// server-side twin of the Rust SDK's Repository::blame and follows the same
// rules, so both produce the same attribution for the same history:
//
//   - The walk starts at the tip with all requested lines pending and moves
//     pending lines from a snapshot to its parents.
//   - A parent whose entry for the path is identical (same object, same
//     chunked flag) takes all pending lines unchanged, without a diff
//     (parents are tried in order, first parent first).
//   - Otherwise each parent in order (first parent first) takes the pending
//     lines that the line diff (textdiff.Matches) matches to one of its
//     lines, so a line of a merge snapshot goes to the first parent that
//     has it and lines brought in by a merged branch follow that branch.
//   - Lines no parent takes were introduced by the snapshot itself.
//
// Snapshots are processed newest first so lines reaching a snapshot along
// different paths are handled together; the walk ends when no line is
// pending. Renames are not followed: a parent without the path gets no
// lines, and neither does a parent whose version is binary. Blaming a
// binary file is an error. Chunked files are read through content.ReadEntry.
package blame

import (
	"container/heap"
	"context"
	"errors"
	"fmt"
	"sort"
	"strings"

	"google.golang.org/protobuf/proto"

	"github.com/seanfilimon/worktree/services/server-go/internal/content"
	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/objstore"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
	"github.com/seanfilimon/worktree/services/server-go/internal/textdiff"
)

// MaxFileSize bounds the size of every version of a blamed file.
const MaxFileSize = 32 << 20

// Errors reported by File. ErrNoFile wraps domain.ErrNotFound.
var (
	ErrNoFile   = fmt.Errorf("blame: file not found: %w", domain.ErrNotFound)
	ErrBinary   = errors.New("blame: binary file")
	ErrTooLarge = fmt.Errorf("blame: file is larger than %d bytes", MaxFileSize)
	ErrRange    = errors.New("blame: invalid line range")
	// ErrIncomplete reports a history with a dangling parent reference.
	ErrIncomplete = errors.New("blame: history is incomplete")
)

// Snapshots loads snapshot records; domain.MetaStore implements it.
type Snapshots interface {
	GetSnapshot(ctx context.Context, id string) (*domain.Snapshot, error)
}

// Blamer computes blames over a snapshot store and an object store.
type Blamer struct {
	snaps Snapshots
	objs  objstore.Store
}

// New returns a Blamer reading snapshots from snaps and objects from objs.
func New(snaps Snapshots, objs objstore.Store) *Blamer {
	return &Blamer{snaps: snaps, objs: objs}
}

// Options restricts a blame to the 1-based inclusive line range
// [Start, End]. Start 0 means 1, End 0 means the last line; End is clamped
// to the file length.
type Options struct {
	Start, End int
}

// origin is a snapshot waiting to hand its pending lines on.
type origin struct {
	snap  *domain.Snapshot
	entry *syncpb.ManifestEntry
	// lines are (0-based line index in this snapshot's version, 0-based
	// final line) pairs.
	lines [][2]int
}

// walk holds the caches of one File call.
type walk struct {
	b       *Blamer
	ctx     context.Context
	path    string
	entries map[hash.Hash]*syncpb.ManifestEntry // manifest hash -> entry (nil = absent)
	texts   map[textKey]*text
}

type textKey struct {
	h       hash.Hash
	chunked bool
}

// text is a decoded file version; lines is nil for binary content.
type text struct {
	lines  []string
	binary bool
}

// File blames path at tip.
func (b *Blamer) File(ctx context.Context, tip *domain.Snapshot, path string, o Options) ([]domain.BlameLine, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	w := &walk{b: b, ctx: ctx, path: path, entries: map[hash.Hash]*syncpb.ManifestEntry{}, texts: map[textKey]*text{}}
	entry, err := w.entry(tip)
	if err != nil {
		return nil, err
	}
	if entry == nil {
		return nil, fmt.Errorf("%w: %s in snapshot %s", ErrNoFile, path, tip.ID)
	}
	t, err := w.text(entry)
	if err != nil {
		return nil, err
	}
	if t.binary {
		return nil, fmt.Errorf("%w: %s", ErrBinary, path)
	}
	final := t.lines
	lo, hi := 0, len(final)
	if o.Start != 0 || o.End != 0 {
		start, end := max(o.Start, 1), o.End
		if end == 0 {
			end = len(final)
		}
		if o.Start < 0 || end < start {
			return nil, fmt.Errorf("%w: %d,%d", ErrRange, o.Start, o.End)
		}
		if start > len(final) {
			return nil, fmt.Errorf("%w: %s has only %d lines", ErrRange, path, len(final))
		}
		lo, hi = start-1, min(end, len(final))
	}

	owner := make([]*domain.Snapshot, hi-lo)
	pending := map[string]*origin{}
	q := &queue{}
	if hi > lo {
		lines := make([][2]int, 0, hi-lo)
		for i := lo; i < hi; i++ {
			lines = append(lines, [2]int{i, i})
		}
		pending[tip.ID] = &origin{snap: tip, entry: entry, lines: lines}
		heap.Push(q, tip)
	}
	for q.Len() > 0 {
		if err := ctx.Err(); err != nil {
			return nil, err
		}
		s := heap.Pop(q).(*domain.Snapshot)
		o, ok := pending[s.ID]
		if !ok {
			continue
		}
		delete(pending, s.ID)
		kept, err := w.passToParents(o, pending, q)
		if err != nil {
			return nil, err
		}
		for _, fin := range kept {
			owner[fin-lo] = o.snap
		}
	}

	out := make([]domain.BlameLine, 0, hi-lo)
	for i := lo; i < hi; i++ {
		s := owner[i-lo]
		if s == nil {
			return nil, fmt.Errorf("blame: line %d was not attributed", i+1)
		}
		out = append(out, domain.BlameLine{
			Line:       i + 1,
			Text:       stripEOL(final[i]),
			SnapshotID: s.ID,
			Author:     s.Author,
			Message:    subject(s.Message),
			At:         s.CreatedAt,
		})
	}
	return out, nil
}

// passToParents hands o's lines to its parents and returns the final line
// indices no parent took.
func (w *walk) passToParents(o *origin, pending map[string]*origin, q *queue) ([]int, error) {
	type parent struct {
		snap  *domain.Snapshot
		entry *syncpb.ManifestEntry
	}
	var parents []parent
	seen := map[string]bool{}
	for _, id := range o.snap.Parents {
		if seen[id] {
			continue
		}
		seen[id] = true
		p, err := w.b.snaps.GetSnapshot(w.ctx, id)
		if errors.Is(err, domain.ErrNotFound) {
			// A dangling parent is corruption (deliberately not ErrNotFound,
			// which callers would report as a missing file or revision).
			return nil, fmt.Errorf("%w: parent %s of snapshot %s is missing", ErrIncomplete, id, o.snap.ID)
		}
		if err != nil {
			return nil, err
		}
		e, err := w.entry(p)
		if err != nil {
			return nil, err
		}
		if e != nil {
			parents = append(parents, parent{p, e})
		}
	}

	send := func(p parent, lines [][2]int) {
		if len(lines) == 0 {
			return
		}
		if existing, ok := pending[p.snap.ID]; ok {
			existing.lines = append(existing.lines, lines...)
			return
		}
		pending[p.snap.ID] = &origin{snap: p.snap, entry: p.entry, lines: lines}
		heap.Push(q, p.snap)
	}

	// Unchanged in some parent: everything goes there, no diff needed.
	for _, p := range parents {
		if sameEntry(p.entry, o.entry) {
			send(p, o.lines)
			return nil, nil
		}
	}

	remaining := o.lines
	if len(parents) > 0 {
		cur, err := w.text(o.entry)
		if err != nil {
			return nil, err
		}
		for _, p := range parents {
			if len(remaining) == 0 {
				break
			}
			pt, err := w.text(p.entry)
			if err != nil {
				return nil, err
			}
			if pt.binary {
				continue
			}
			match := make([]int, len(cur.lines))
			for i := range match {
				match[i] = -1
			}
			for _, m := range textdiff.Matches(cur.lines, pt.lines) {
				match[m[0]] = m[1]
			}
			var passed, kept [][2]int
			for _, l := range remaining {
				if l[0] < len(match) && match[l[0]] >= 0 {
					passed = append(passed, [2]int{match[l[0]], l[1]})
				} else {
					kept = append(kept, l)
				}
			}
			send(p, passed)
			remaining = kept
		}
	}
	out := make([]int, len(remaining))
	for i, l := range remaining {
		out[i] = l[1]
	}
	return out, nil
}

// entry returns the path's manifest entry in s (nil if absent).
func (w *walk) entry(s *domain.Snapshot) (*syncpb.ManifestEntry, error) {
	if e, ok := w.entries[s.ManifestHash]; ok {
		return e, nil
	}
	raw, err := w.b.objs.Get(w.ctx, s.ManifestHash)
	if err != nil {
		// %v: a missing manifest of a stored snapshot is corruption, not a
		// client error.
		return nil, fmt.Errorf("blame: snapshot %s: load manifest %s: %v", s.ID, s.ManifestHash, err)
	}
	var m syncpb.ManifestRecord
	if err := proto.Unmarshal(raw, &m); err != nil {
		return nil, fmt.Errorf("blame: snapshot %s: decode manifest: %w", s.ID, err)
	}
	e := findEntry(m.GetEntries(), w.path)
	w.entries[s.ManifestHash] = e
	return e, nil
}

// text returns the decoded content of an entry.
func (w *walk) text(e *syncpb.ManifestEntry) (*text, error) {
	h, err := content.EntryHash(e)
	if err != nil {
		return nil, err
	}
	key := textKey{h, e.GetChunked()}
	if t, ok := w.texts[key]; ok {
		return t, nil
	}
	if e.GetSize() > MaxFileSize {
		return nil, fmt.Errorf("%w: %s", ErrTooLarge, e.GetPath())
	}
	data, err := content.ReadEntry(w.ctx, w.b.objs, e)
	if err != nil {
		return nil, fmt.Errorf("blame: content %s of %q: %v", h, e.GetPath(), err)
	}
	if len(data) > MaxFileSize {
		return nil, fmt.Errorf("%w: %s", ErrTooLarge, e.GetPath())
	}
	t := &text{binary: textdiff.IsBinary(data)}
	if !t.binary {
		t.lines = textdiff.Lines(data)
	}
	w.texts[key] = t
	return t, nil
}

func sameEntry(a, b *syncpb.ManifestEntry) bool {
	return a.GetChunked() == b.GetChunked() && string(a.GetBlobHash().GetValue()) == string(b.GetBlobHash().GetValue())
}

func findEntry(entries []*syncpb.ManifestEntry, p string) *syncpb.ManifestEntry {
	i := sort.Search(len(entries), func(i int) bool { return entries[i].GetPath() >= p })
	if i < len(entries) && entries[i].GetPath() == p {
		return entries[i]
	}
	// Manifests are specified as sorted, but tolerate unsorted ones.
	for _, e := range entries {
		if e.GetPath() == p {
			return e
		}
	}
	return nil
}

// stripEOL removes a trailing "\n" or "\r\n".
func stripEOL(line string) string {
	line = strings.TrimSuffix(line, "\n")
	return strings.TrimSuffix(line, "\r")
}

// subject is the first line of a message.
func subject(msg string) string {
	if i := strings.IndexByte(msg, '\n'); i >= 0 {
		msg = msg[:i]
	}
	return strings.TrimSuffix(msg, "\r")
}

// queue is a max-heap of snapshots by (created_at, id): newest first.
type queue struct{ items []*domain.Snapshot }

func (q *queue) Len() int { return len(q.items) }

func (q *queue) Less(i, j int) bool {
	a, b := q.items[i], q.items[j]
	if c := a.CreatedAt.Compare(b.CreatedAt); c != 0 {
		return c > 0
	}
	return a.ID > b.ID
}

func (q *queue) Swap(i, j int) { q.items[i], q.items[j] = q.items[j], q.items[i] }

func (q *queue) Push(x any) { q.items = append(q.items, x.(*domain.Snapshot)) }

func (q *queue) Pop() any {
	n := len(q.items)
	x := q.items[n-1]
	q.items[n-1] = nil
	q.items = q.items[:n-1]
	return x
}
