package codesearch

import (
	"cmp"
	"encoding/binary"
	"encoding/json"
	"errors"
	"fmt"
	"hash/crc32"
	"slices"
	"sort"
	"strings"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
)

// Segments (docs/ENGINEERING_PLAN.md §15.9).
//
// A segment is the immutable index of one manifest: the text files it
// holds (path, blob hash, content), the binary files that were skipped
// (so that rebuilds do not read them again), the symbols extracted from
// the files and one posting list (sorted file ids) per trigram. Branches
// whose tips share a manifest share its segment.
//
// The same byte encoding is used on disk (<dir>/<tree>/<manifest>.seg) and
// in memory: a loaded segment keeps the encoded buffer and references file
// contents and posting lists inside it without copying. All integers are
// little-endian; "uv" is an unsigned varint and "str" is uv(len) + bytes.
//
//	magic "WTCS" | u32 FormatVersion
//	uv(len) stats JSON (segStats)
//	uv(nDocs)   { str path | [32]hash | uv(contentLen) }
//	uv(nBinary) { str path | [32]hash }
//	uv(nSyms)   { str name | u8 kind | uv(doc) | uv(line) | str container | str signature }
//	uv(nTri)    { [3]trigram | uv(postingLen) }   (trigrams ascending)
//	postings    concatenated; each list is uv(first id) uv(delta)...
//	contents    concatenated in doc order
//	u32 CRC-32C of every preceding byte
//
// A segment whose magic or version differs is a format mismatch, one whose
// checksum or structure is wrong is corrupt; both are discarded and the
// branch is rebuilt.

// FormatVersion is the version of the on-disk index format (segments and
// tree state files). Indexes written with another version are rebuilt.
const FormatVersion = 1

const segMagic = "WTCS"

var (
	errSegFormat  = errors.New("codesearch: index format version mismatch")
	errSegCorrupt = errors.New("codesearch: index segment is corrupt")
	crcTable      = crc32.MakeTable(crc32.Castagnoli)
)

// segStats describes the content of a segment.
type segStats struct {
	Files          int       `json:"files"`
	Bytes          int64     `json:"bytes"`
	Trigrams       int       `json:"trigrams"`
	Symbols        int       `json:"symbols"`
	SkippedBinary  int       `json:"skipped_binary"`
	SkippedLarge   int       `json:"skipped_large"`
	SkippedChunked int       `json:"skipped_chunked"`
	Reused         int       `json:"reused"`
	BuiltAt        time.Time `json:"built_at"`
	BuildMS        int64     `json:"build_ms"`
}

// segDoc is one indexed text file.
type segDoc struct {
	path    string
	hash    hash.Hash
	content []byte
}

// skipEntry is a binary file that was not indexed.
type skipEntry struct {
	path string
	hash hash.Hash
}

// symbol is one extracted definition; doc indexes segment.docs.
type symbol struct {
	Name      string
	Kind      SymbolKind
	Line      int
	Container string
	Signature string
	doc       uint32
}

// segment is a decoded, immutable index of one manifest.
type segment struct {
	manifest hash.Hash
	stats    segStats
	docs     []segDoc    // sorted by path
	binary   []skipEntry // sorted by path
	syms     []symbol    // sorted by lower-cased name, path, line
	lnames   []string    // lower-cased symbol names, parallel to syms
	tri      []uint32    // ascending trigrams
	triOff   []uint32    // len(tri)+1 offsets into post
	post     []byte
	size     int64 // approximate memory footprint
	encoded  int64 // length of the encoding (= file size)
}

// postings returns the doc ids (ascending) containing trigram t.
func (s *segment) postings(t trigram) []uint32 {
	i := sort.Search(len(s.tri), func(i int) bool { return s.tri[i] >= uint32(t) })
	if i == len(s.tri) || s.tri[i] != uint32(t) {
		return nil
	}
	buf := s.post[s.triOff[i]:s.triOff[i+1]]
	out := make([]uint32, 0, len(buf))
	var cur uint64
	for first := true; len(buf) > 0; first = false {
		v, n := binary.Uvarint(buf)
		if n <= 0 {
			break // validated at decode time; defensive
		}
		buf = buf[n:]
		if first {
			cur = v
		} else {
			cur += v
		}
		out = append(out, uint32(cur))
	}
	return out
}

// ---- encoding ----

// segInput is what a build hands to encodeSegment.
type segInput struct {
	stats    segStats
	docs     []segDoc // sorted by path
	binary   []skipEntry
	syms     []symbol
	postings map[trigram][]uint32
}

type encoder struct{ b []byte }

func (e *encoder) uv(v uint64)      { e.b = binary.AppendUvarint(e.b, v) }
func (e *encoder) str(s string)     { e.uv(uint64(len(s))); e.b = append(e.b, s...) }
func (e *encoder) raw(p []byte)     { e.b = append(e.b, p...) }
func (e *encoder) u32(v uint32)     { e.b = binary.LittleEndian.AppendUint32(e.b, v) }
func (e *encoder) hash(h hash.Hash) { e.b = append(e.b, h[:]...) }

// kindCode maps symbol kinds to their on-disk byte.
func kindCode(k SymbolKind) byte {
	for i, s := range SymbolKinds {
		if s == k {
			return byte(i)
		}
	}
	return 0
}

// encodeSegment returns the encoding of in. Symbols are sorted here.
func encodeSegment(in *segInput) ([]byte, error) {
	statsJSON, err := json.Marshal(in.stats)
	if err != nil {
		return nil, err
	}
	sortSymbols(in.syms, in.docs)
	size := 64 + len(statsJSON)
	for _, d := range in.docs {
		size += len(d.path) + 48 + len(d.content)
	}
	e := &encoder{b: make([]byte, 0, size)}
	e.raw([]byte(segMagic))
	e.u32(FormatVersion)
	e.uv(uint64(len(statsJSON)))
	e.raw(statsJSON)

	e.uv(uint64(len(in.docs)))
	for _, d := range in.docs {
		e.str(d.path)
		e.hash(d.hash)
		e.uv(uint64(len(d.content)))
	}
	e.uv(uint64(len(in.binary)))
	for _, s := range in.binary {
		e.str(s.path)
		e.hash(s.hash)
	}
	e.uv(uint64(len(in.syms)))
	for _, s := range in.syms {
		e.str(s.Name)
		e.b = append(e.b, kindCode(s.Kind))
		e.uv(uint64(s.doc))
		e.uv(uint64(s.Line))
		e.str(s.Container)
		e.str(s.Signature)
	}

	keys := make([]trigram, 0, len(in.postings))
	for t := range in.postings {
		keys = append(keys, t)
	}
	sort.Slice(keys, func(i, j int) bool { return keys[i] < keys[j] })
	lists := make([][]byte, len(keys))
	var scratch []byte
	for i, t := range keys {
		scratch = scratch[:0]
		prev := uint32(0)
		for j, id := range in.postings[t] {
			if j == 0 {
				scratch = binary.AppendUvarint(scratch, uint64(id))
			} else {
				scratch = binary.AppendUvarint(scratch, uint64(id-prev))
			}
			prev = id
		}
		lists[i] = append([]byte(nil), scratch...)
	}
	e.uv(uint64(len(keys)))
	for i, t := range keys {
		e.b = append(e.b, byte(t>>16), byte(t>>8), byte(t))
		e.uv(uint64(len(lists[i])))
	}
	for _, l := range lists {
		e.raw(l)
	}
	for _, d := range in.docs {
		e.raw(d.content)
	}
	e.u32(crc32.Checksum(e.b, crcTable))
	return e.b, nil
}

// sortSymbols orders symbols by lower-cased name, then path and line.
// Docs are in path order, so ordering by doc id orders by path.
func sortSymbols(syms []symbol, _ []segDoc) {
	type keyed struct {
		lname string
		sym   symbol
	}
	ks := make([]keyed, len(syms))
	for i, s := range syms {
		ks[i] = keyed{strings.ToLower(s.Name), s}
	}
	slices.SortFunc(ks, func(a, b keyed) int {
		if c := strings.Compare(a.lname, b.lname); c != 0 {
			return c
		}
		if a.sym.doc != b.sym.doc {
			return cmp.Compare(a.sym.doc, b.sym.doc)
		}
		if a.sym.Line != b.sym.Line {
			return cmp.Compare(a.sym.Line, b.sym.Line)
		}
		return strings.Compare(a.sym.Name, b.sym.Name)
	})
	for i := range ks {
		syms[i] = ks[i].sym
	}
}

// ---- decoding ----

type decoder struct {
	b   []byte
	off int
	err error
}

func (d *decoder) fail() {
	if d.err == nil {
		d.err = errSegCorrupt
	}
}

func (d *decoder) uv() uint64 {
	if d.err != nil {
		return 0
	}
	v, n := binary.Uvarint(d.b[d.off:])
	if n <= 0 {
		d.fail()
		return 0
	}
	d.off += n
	return v
}

// n reads a count or length that must fit in the remaining bytes.
func (d *decoder) n() int {
	v := d.uv()
	if v > uint64(len(d.b)-d.off) && v > 0 {
		// Counts of fixed-size records are bounded by the remaining bytes
		// too, so this catches absurd values early.
		d.fail()
		return 0
	}
	return int(v)
}

func (d *decoder) bytes(n int) []byte {
	if d.err != nil {
		return nil
	}
	if n < 0 || d.off+n > len(d.b) {
		d.fail()
		return nil
	}
	p := d.b[d.off : d.off+n : d.off+n]
	d.off += n
	return p
}

func (d *decoder) str() string { return string(d.bytes(d.n())) }

func (d *decoder) hash() hash.Hash {
	var h hash.Hash
	copy(h[:], d.bytes(hash.Size))
	return h
}

// decodeSegment parses an encoded segment. The returned segment references
// data, which must not be modified afterwards.
func decodeSegment(manifest hash.Hash, data []byte) (*segment, error) {
	if len(data) < len(segMagic)+8 || string(data[:4]) != segMagic {
		return nil, errSegFormat
	}
	if v := binary.LittleEndian.Uint32(data[4:8]); v != FormatVersion {
		return nil, fmt.Errorf("%w: segment version %d, want %d", errSegFormat, v, FormatVersion)
	}
	body := data[:len(data)-4]
	if crc32.Checksum(body, crcTable) != binary.LittleEndian.Uint32(data[len(data)-4:]) {
		return nil, fmt.Errorf("%w: checksum mismatch", errSegCorrupt)
	}
	d := &decoder{b: body, off: 8}
	s := &segment{manifest: manifest, encoded: int64(len(data))}
	if err := json.Unmarshal(d.bytes(d.n()), &s.stats); err != nil || d.err != nil {
		return nil, errSegCorrupt
	}
	nDocs := d.n()
	s.docs = make([]segDoc, 0, nDocs)
	lens := make([]int, 0, nDocs)
	for i := 0; i < nDocs && d.err == nil; i++ {
		p := d.str()
		h := d.hash()
		s.docs = append(s.docs, segDoc{path: p, hash: h})
		lens = append(lens, d.n())
	}
	nBin := d.n()
	s.binary = make([]skipEntry, 0, nBin)
	for i := 0; i < nBin && d.err == nil; i++ {
		p := d.str()
		s.binary = append(s.binary, skipEntry{path: p, hash: d.hash()})
	}
	nSyms := d.n()
	s.syms = make([]symbol, 0, nSyms)
	s.lnames = make([]string, 0, nSyms)
	for i := 0; i < nSyms && d.err == nil; i++ {
		var sym symbol
		sym.Name = d.str()
		k := d.bytes(1)
		if len(k) == 1 && int(k[0]) < len(SymbolKinds) {
			sym.Kind = SymbolKinds[k[0]]
		} else {
			d.fail()
		}
		doc := d.uv()
		if doc >= uint64(nDocs) {
			d.fail()
		}
		sym.doc = uint32(doc)
		sym.Line = int(d.uv())
		sym.Container = d.str()
		sym.Signature = d.str()
		s.syms = append(s.syms, sym)
		s.lnames = append(s.lnames, strings.ToLower(sym.Name))
	}
	nTri := d.n()
	s.tri = make([]uint32, 0, nTri)
	s.triOff = make([]uint32, 0, nTri+1)
	total := 0
	prev := int64(-1)
	for i := 0; i < nTri && d.err == nil; i++ {
		t := d.bytes(3)
		if len(t) < 3 {
			break
		}
		v := uint32(t[0])<<16 | uint32(t[1])<<8 | uint32(t[2])
		if int64(v) <= prev {
			d.fail()
		}
		prev = int64(v)
		s.tri = append(s.tri, v)
		s.triOff = append(s.triOff, uint32(total))
		total += d.n()
	}
	s.triOff = append(s.triOff, uint32(total))
	s.post = d.bytes(total)
	for i := range s.docs {
		if d.err != nil {
			break
		}
		s.docs[i].content = d.bytes(lens[i])
	}
	if d.err != nil {
		return nil, d.err
	}
	if d.off != len(body) {
		return nil, fmt.Errorf("%w: %d trailing bytes", errSegCorrupt, len(body)-d.off)
	}
	if err := s.validatePostings(); err != nil {
		return nil, err
	}
	s.size = s.memSize()
	return s, nil
}

// validatePostings checks that every posting list decodes to ascending ids
// of existing documents, so that queries never index out of range.
func (s *segment) validatePostings() error {
	for i := range s.tri {
		buf := s.post[s.triOff[i]:s.triOff[i+1]]
		var cur uint64
		for first := true; len(buf) > 0; first = false {
			v, n := binary.Uvarint(buf)
			if n <= 0 || (!first && v == 0) {
				return errSegCorrupt
			}
			buf = buf[n:]
			if first {
				cur = v
			} else {
				cur += v
			}
			if cur >= uint64(len(s.docs)) {
				return errSegCorrupt
			}
		}
	}
	return nil
}

// memSize estimates the memory a decoded segment holds.
func (s *segment) memSize() int64 {
	n := s.encoded + int64(len(s.tri))*8 + int64(cap(s.docs))*72 + int64(cap(s.binary))*56
	for _, d := range s.docs {
		n += int64(len(d.path))
	}
	for i, sym := range s.syms {
		n += 96 + int64(len(sym.Name)+len(sym.Container)+len(sym.Signature)+len(s.lnames[i]))
	}
	return n
}
