package codesearch

import (
	"bytes"
	"context"
	"fmt"
	"sort"
	"time"

	"google.golang.org/protobuf/proto"

	"github.com/seanfilimon/worktree/services/server-go/internal/content"
	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
)

// MaxFileSize is the largest file indexed (bytes); larger files are skipped.
const MaxFileSize = 1 << 20

// binarySniff is how many leading bytes are checked for a NUL byte.
const binarySniff = 8000

// Meta describes the index of one branch; the default branch's Meta is
// stored in the code_index_meta collection under the tree id.
type Meta struct {
	TreeID         string    `json:"tree_id"`
	Branch         string    `json:"branch"`
	SnapshotID     string    `json:"snapshot_id"`
	Manifest       string    `json:"manifest,omitempty"` // manifest hash (= segment) of the snapshot
	Files          int       `json:"files"`
	Bytes          int64     `json:"bytes"`
	Trigrams       int       `json:"trigrams"`
	Symbols        int       `json:"symbols"`
	SkippedBinary  int       `json:"skipped_binary"`
	SkippedLarge   int       `json:"skipped_large"`
	SkippedChunked int       `json:"skipped_chunked"`
	Reused         int       `json:"reused"` // files carried over from a previous index
	BuiltAt        time.Time `json:"built_at"`
	BuildMS        int64     `json:"build_ms"`
}

// metaOf combines a segment's statistics with branch information.
func metaOf(treeID, branch, snapshotID string, manifest hash.Hash, st segStats) Meta {
	m := Meta{TreeID: treeID, Branch: branch, SnapshotID: snapshotID,
		Files: st.Files, Bytes: st.Bytes, Trigrams: st.Trigrams, Symbols: st.Symbols,
		SkippedBinary: st.SkippedBinary, SkippedLarge: st.SkippedLarge, SkippedChunked: st.SkippedChunked,
		Reused: st.Reused, BuiltAt: st.BuiltAt, BuildMS: st.BuildMS}
	if !manifest.IsZero() {
		m.Manifest = manifest.Hex()
	}
	return m
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

// reuseSet holds the content of files of earlier segments of a tree, so
// that unchanged blobs are not read from the object store again.
type reuseSet struct {
	text   map[hash.Hash][]byte
	binary map[hash.Hash]bool
}

func newReuseSet(segs ...*segment) *reuseSet {
	r := &reuseSet{text: map[hash.Hash][]byte{}, binary: map[hash.Hash]bool{}}
	for _, s := range segs {
		if s == nil {
			continue
		}
		for _, d := range s.docs {
			r.text[d.hash] = d.content
		}
		for _, b := range s.binary {
			r.binary[b.hash] = true
		}
	}
	return r
}

// readManifest loads and decodes a manifest object.
func (ix *Index) readManifest(ctx context.Context, h hash.Hash) (*syncpb.ManifestRecord, error) {
	raw, err := ix.d.Objects.Get(ctx, h)
	if err != nil {
		return nil, fmt.Errorf("codesearch: manifest %s: %v", h, err)
	}
	var m syncpb.ManifestRecord
	if err := proto.Unmarshal(raw, &m); err != nil {
		return nil, fmt.Errorf("codesearch: decode manifest %s: %w", h, err)
	}
	return &m, nil
}

// buildSegment indexes the files of manifest m and returns the encoded
// segment. Blobs found in reuse are not read from the object store.
func (ix *Index) buildSegment(ctx context.Context, m *syncpb.ManifestRecord, reuse *reuseSet) ([]byte, error) {
	start := time.Now()
	in := &segInput{postings: map[trigram][]uint32{}}
	entries := append([]*syncpb.ManifestEntry(nil), m.GetEntries()...)
	sort.Slice(entries, func(i, j int) bool { return entries[i].GetPath() < entries[j].GetPath() })
	var scratch []uint64
	for _, e := range entries {
		if err := ctx.Err(); err != nil {
			return nil, err
		}
		switch {
		case e.GetChunked():
			in.stats.SkippedChunked++
			continue
		case e.GetSize() > MaxFileSize:
			in.stats.SkippedLarge++
			continue
		}
		h, err := content.EntryHash(e)
		if err != nil {
			return nil, err
		}
		if reuse.binary[h] {
			in.binary = append(in.binary, skipEntry{path: e.GetPath(), hash: h})
			in.stats.SkippedBinary++
			continue
		}
		data, ok := reuse.text[h]
		if ok {
			in.stats.Reused++
		} else {
			if data, err = ix.d.Objects.Get(ctx, h); err != nil {
				return nil, fmt.Errorf("codesearch: blob of %s: %v", e.GetPath(), err)
			}
			if len(data) > MaxFileSize {
				in.stats.SkippedLarge++
				continue
			}
			if isBinary(data) {
				in.binary = append(in.binary, skipEntry{path: e.GetPath(), hash: h})
				in.stats.SkippedBinary++
				continue
			}
		}
		if scratch == nil {
			scratch = make([]uint64, 1<<24/64)
		}
		id := uint32(len(in.docs))
		in.docs = append(in.docs, segDoc{path: e.GetPath(), hash: h, content: data})
		in.stats.Bytes += int64(len(data))
		for _, t := range fileTrigrams(data, scratch) {
			in.postings[t] = append(in.postings[t], id)
		}
		for _, s := range ExtractSymbols(e.GetPath(), data) {
			in.syms = append(in.syms, symbol{Name: s.Name, Kind: s.Kind, Line: s.Line, Container: s.Container, Signature: s.Signature, doc: id})
		}
	}
	in.stats.Files = len(in.docs)
	in.stats.Trigrams = len(in.postings)
	in.stats.Symbols = len(in.syms)
	in.stats.BuiltAt = time.Now().UTC()
	in.stats.BuildMS = time.Since(start).Milliseconds()
	return encodeSegment(in)
}

// candidates returns the ids (ascending, i.e. in path order) of the docs
// that satisfy q.
func (s *segment) candidates(q *tquery) []uint32 {
	ids, all := s.eval(q)
	if !all {
		return ids
	}
	out := make([]uint32, len(s.docs))
	for i := range out {
		out[i] = uint32(i)
	}
	return out
}

// eval returns the matching doc ids, or all=true when q matches every doc.
func (s *segment) eval(q *tquery) (ids []uint32, all bool) {
	switch q.op {
	case qAll:
		return nil, true
	case qNone:
		return nil, false
	case qAnd:
		var acc []uint32
		have := false
		for _, tr := range q.tri {
			p := s.postings(tr)
			if !have {
				acc, have = p, true
			} else {
				acc = intersect(acc, p)
			}
			if len(acc) == 0 {
				return nil, false
			}
		}
		for _, sub := range q.sub {
			ids, all := s.eval(sub)
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
			acc = union(acc, s.postings(tr))
		}
		for _, sub := range q.sub {
			ids, all := s.eval(sub)
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
