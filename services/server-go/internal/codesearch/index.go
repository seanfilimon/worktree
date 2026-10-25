package codesearch

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"sort"
	"time"

	"google.golang.org/protobuf/proto"

	"github.com/seanfilimon/worktree/services/server-go/internal/content"
	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
)

// MaxFileSize is the largest file indexed (bytes); larger files are skipped.
const MaxFileSize = 1 << 20

// binarySniff is how many leading bytes are checked for a NUL byte.
const binarySniff = 8000

// doc is one indexed text file. Docs are immutable and shared between
// successive indexes of a tree while their content hash is unchanged.
type doc struct {
	path    string
	hash    hash.Hash
	content []byte
	tri     []trigram // distinct trigrams, sorted
}

// treeIndex is the immutable index of one snapshot of a tree's default
// branch.
type treeIndex struct {
	treeID     string
	branch     string
	snapshotID string // "" = the branch is missing or empty
	docs       []*doc // sorted by path
	postings   map[trigram][]uint32
	skipped    map[string]hash.Hash // binary files (path -> hash), not re-read on rebuilds
	meta       Meta
}

// Meta describes a built index; it is stored in the code_index_meta
// collection under the tree id.
type Meta struct {
	TreeID         string    `json:"tree_id"`
	Branch         string    `json:"branch"`
	SnapshotID     string    `json:"snapshot_id"`
	Files          int       `json:"files"`
	Bytes          int64     `json:"bytes"`
	Trigrams       int       `json:"trigrams"`
	SkippedBinary  int       `json:"skipped_binary"`
	SkippedLarge   int       `json:"skipped_large"`
	SkippedChunked int       `json:"skipped_chunked"`
	Reused         int       `json:"reused"` // files carried over from the previous index
	BuiltAt        time.Time `json:"built_at"`
	BuildMS        int64     `json:"build_ms"`
}

// isBinary reports whether data looks like a binary file.
func isBinary(data []byte) bool {
	return bytes.IndexByte(data[:min(len(data), binarySniff)], 0) >= 0
}

// fileTrigrams returns the distinct trigrams of data, sorted. scratch is a
// 2^24-bit set that must be all zero on entry and is all zero on return.
func fileTrigrams(data []byte, scratch []uint64) []trigram {
	if len(data) < 3 {
		return nil
	}
	var out []trigram
	for i := 0; i+3 <= len(data); i++ {
		t := makeTrigram(data[i], data[i+1], data[i+2])
		w, bit := t>>6, uint64(1)<<(t&63)
		if scratch[w]&bit != 0 {
			continue
		}
		scratch[w] |= bit
		out = append(out, t)
	}
	for _, t := range out {
		scratch[t>>6] = 0
	}
	sort.Slice(out, func(i, j int) bool { return out[i] < out[j] })
	return out
}

// build indexes tree's default branch, reusing the documents of prev
// whose content hash is unchanged.
func (ix *Index) build(ctx context.Context, tree *domain.Tree, prev *treeIndex) (*treeIndex, error) {
	start := time.Now()
	out := &treeIndex{
		treeID:   tree.ID,
		branch:   tree.DefaultBranch,
		postings: map[trigram][]uint32{},
		skipped:  map[string]hash.Hash{},
	}
	tip, err := ix.tip(ctx, tree)
	if err != nil {
		return nil, err
	}
	out.snapshotID = tip
	if tip != "" {
		if err := ix.fill(ctx, out, prev); err != nil {
			return nil, err
		}
	}
	out.meta.TreeID, out.meta.Branch, out.meta.SnapshotID = tree.ID, out.branch, out.snapshotID
	out.meta.Files = len(out.docs)
	out.meta.Trigrams = len(out.postings)
	out.meta.BuiltAt = time.Now().UTC()
	out.meta.BuildMS = time.Since(start).Milliseconds()
	return out, nil
}

// tip returns the default-branch tip of tree ("" when there is none).
func (ix *Index) tip(ctx context.Context, tree *domain.Tree) (string, error) {
	b, err := ix.d.Store.GetBranch(ctx, tree.ID, tree.DefaultBranch)
	if errors.Is(err, domain.ErrNotFound) {
		return "", nil
	}
	if err != nil {
		return "", err
	}
	return b.Tip, nil
}

func (ix *Index) fill(ctx context.Context, out, prev *treeIndex) error {
	snap, err := ix.d.Store.GetSnapshot(ctx, out.snapshotID)
	if err != nil {
		return fmt.Errorf("codesearch: snapshot %s: %w", out.snapshotID, err)
	}
	raw, err := ix.d.Objects.Get(ctx, snap.ManifestHash)
	if err != nil {
		return fmt.Errorf("codesearch: manifest of %s: %v", snap.ID, err)
	}
	var m syncpb.ManifestRecord
	if err := proto.Unmarshal(raw, &m); err != nil {
		return fmt.Errorf("codesearch: decode manifest of %s: %w", snap.ID, err)
	}
	reuse := map[string]*doc{}
	var reuseBinary map[string]hash.Hash
	if prev != nil {
		for _, d := range prev.docs {
			reuse[d.path] = d
		}
		reuseBinary = prev.skipped
	}
	entries := m.GetEntries()
	sort.Slice(entries, func(i, j int) bool { return entries[i].GetPath() < entries[j].GetPath() })
	var scratch []uint64
	for _, e := range entries {
		if err := ctx.Err(); err != nil {
			return err
		}
		switch {
		case e.GetChunked():
			out.meta.SkippedChunked++
			continue
		case e.GetSize() > MaxFileSize:
			out.meta.SkippedLarge++
			continue
		}
		h, err := content.EntryHash(e)
		if err != nil {
			return err
		}
		if d := reuse[e.GetPath()]; d != nil && d.hash == h {
			out.docs = append(out.docs, d)
			out.meta.Reused++
			out.meta.Bytes += int64(len(d.content))
			continue
		}
		if bh, ok := reuseBinary[e.GetPath()]; ok && bh == h {
			out.skipped[e.GetPath()] = h
			out.meta.SkippedBinary++
			continue
		}
		data, err := ix.d.Objects.Get(ctx, h)
		if err != nil {
			return fmt.Errorf("codesearch: blob of %s: %v", e.GetPath(), err)
		}
		if len(data) > MaxFileSize {
			out.meta.SkippedLarge++
			continue
		}
		if isBinary(data) {
			out.skipped[e.GetPath()] = h
			out.meta.SkippedBinary++
			continue
		}
		if scratch == nil {
			scratch = make([]uint64, 1<<24/64)
		}
		out.docs = append(out.docs, &doc{path: e.GetPath(), hash: h, content: data, tri: fileTrigrams(data, scratch)})
		out.meta.Bytes += int64(len(data))
	}
	for id, d := range out.docs {
		for _, t := range d.tri {
			out.postings[t] = append(out.postings[t], uint32(id))
		}
	}
	return nil
}

// candidates returns the ids (ascending, i.e. in path order) of the docs
// that satisfy q.
func (t *treeIndex) candidates(q *tquery) []uint32 {
	ids, all := t.eval(q)
	if !all {
		return ids
	}
	out := make([]uint32, len(t.docs))
	for i := range out {
		out[i] = uint32(i)
	}
	return out
}

// eval returns the matching doc ids, or all=true when q matches every doc.
func (t *treeIndex) eval(q *tquery) (ids []uint32, all bool) {
	switch q.op {
	case qAll:
		return nil, true
	case qNone:
		return nil, false
	case qAnd:
		var acc []uint32
		have := false
		for _, tr := range q.tri {
			p := t.postings[tr]
			if !have {
				acc, have = p, true
			} else {
				acc = intersect(acc, p)
			}
			if len(acc) == 0 {
				return nil, false
			}
		}
		for _, s := range q.sub {
			ids, all := t.eval(s)
			if all {
				continue
			}
			if !have {
				acc, have = ids, true
			} else {
				acc = intersect(acc, ids)
			}
			if len(acc) == 0 {
				return nil, false
			}
		}
		if !have {
			return nil, true
		}
		return acc, false
	case qOr:
		var acc []uint32
		for _, tr := range q.tri {
			acc = union(acc, t.postings[tr])
		}
		for _, s := range q.sub {
			ids, all := t.eval(s)
			if all {
				return nil, true
			}
			acc = union(acc, ids)
		}
		return acc, false
	}
	return nil, true
}

func intersect(a, b []uint32) []uint32 {
	out := make([]uint32, 0, min(len(a), len(b)))
	for i, j := 0, 0; i < len(a) && j < len(b); {
		switch {
		case a[i] < b[j]:
			i++
		case a[i] > b[j]:
			j++
		default:
			out = append(out, a[i])
			i++
			j++
		}
	}
	return out
}

func union(a, b []uint32) []uint32 {
	out := make([]uint32, 0, len(a)+len(b))
	i, j := 0, 0
	for i < len(a) && j < len(b) {
		switch {
		case a[i] < b[j]:
			out = append(out, a[i])
			i++
		case a[i] > b[j]:
			out = append(out, b[j])
			j++
		default:
			out = append(out, a[i])
			i++
			j++
		}
	}
	out = append(out, a[i:]...)
	return append(out, b[j:]...)
}
