package syncsvc

// Pack format with binary deltas (docs/ENGINEERING_PLAN.md §16.1,
// crates/worktree-protocol/specs/sync/Packs.md). This is the Go twin of
// crates/worktree-protocol/src/pack/: both implementations write identical
// bytes for raw (uncompressed) packs and deltas, which the shared golden
// vectors in crates/worktree-protocol/testdata/pack/vectors.json check.
//
//	pack    = header entry* end trailer
//	header  = "WTPK" version:u8(=1) reserved:3×u8(=0)
//	entry   = type:u8 kind:u8 codec:u8 hash:32 [base:32 if type=DELTA]
//	          size:uvarint raw_len:uvarint pay_len:uvarint payload
//	end     = 0x00
//	trailer = BLAKE3(header .. end):32

import (
	"bufio"
	"bytes"
	"encoding/binary"
	"errors"
	"fmt"
	"io"

	"github.com/klauspost/compress/zstd"
	"github.com/zeebo/blake3"

	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
)

// Pack constants (see the package comment above).
const (
	packVersion   = 1
	packHeaderLen = 8
	packTrailer   = 32

	packTypeEnd   = 0
	packTypeWhole = 1
	packTypeDelta = 2

	packCodecRaw  = 0
	packCodecZstd = 1

	// PackMaxObject is the largest object a pack may carry.
	PackMaxObject = 64 << 20
	// packMaxPayload bounds decoded and encoded payloads.
	packMaxPayload = PackMaxObject + (1 << 20)
	// PackMaxDeltaDepth is the longest delta chain (an object the receiver
	// has is depth 0).
	PackMaxDeltaDepth = 8
	// packMaxDeltaBase: larger objects are never used as delta bases.
	packMaxDeltaBase = 16 << 20
	// PackMaxChunk is the largest PackChunk.data accepted or sent.
	PackMaxChunk = 4 << 20
	// packMinCompress: shorter payloads are stored raw.
	packMinCompress = 64

	deltaBlock = 16
	deltaPrime = 16777619

	deltaOpInsert = 0
	deltaOpCopy   = 1
)

var packMagic = [4]byte{'W', 'T', 'P', 'K'}

// Pack errors that make the whole pack unusable.
var (
	// errPackTruncated: the stream ended before the trailer (an
	// interrupted upload that can be resumed).
	errPackTruncated = errors.New("pack is truncated")
	// errPackChecksum: the trailer does not match the content.
	errPackChecksum = errors.New("pack checksum mismatch")
	// errPackCorrupt wraps framing errors.
	errPackCorrupt = errors.New("corrupt pack")
)

func packCorrupt(format string, args ...any) error {
	return fmt.Errorf("%w: %s", errPackCorrupt, fmt.Sprintf(format, args...))
}

// deltaPaysOff reports whether a delta saves at least 25 % of the target.
func deltaPaysOff(deltaLen, targetLen int) bool {
	return uint64(deltaLen)*4 <= uint64(targetLen)*3
}

// ---- deltas ----

// deltaFingerprint is Σ w[j]·P^(15-j) mod 2^32.
func deltaFingerprint(w []byte) uint32 {
	var h uint32
	for _, b := range w {
		h = h*deltaPrime + uint32(b)
	}
	return h
}

func appendUvarint(out []byte, v uint64) []byte { return binary.AppendUvarint(out, v) }

func deltaInsert(out, data []byte) []byte {
	out = append(out, deltaOpInsert)
	out = appendUvarint(out, uint64(len(data)))
	return append(out, data...)
}

func deltaCopy(out []byte, off, n int) []byte {
	out = append(out, deltaOpCopy)
	out = appendUvarint(out, uint64(off))
	return appendUvarint(out, uint64(n))
}

// encodeDelta encodes target against base with the deterministic
// copy/insert algorithm of Packs.md (identical to the Rust encoder).
func encodeDelta(base, target []byte) []byte {
	out := make([]byte, 0, 32)
	out = appendUvarint(out, uint64(len(base)))
	out = appendUvarint(out, uint64(len(target)))
	if len(base) < deltaBlock || len(target) < deltaBlock || len(base) > packMaxDeltaBase {
		if len(target) > 0 {
			out = deltaInsert(out, target)
		}
		return out
	}
	index := make(map[uint32]int, len(base)/deltaBlock)
	for i := 0; i+deltaBlock <= len(base); i += deltaBlock {
		fp := deltaFingerprint(base[i : i+deltaBlock])
		if _, ok := index[fp]; !ok {
			index[fp] = i
		}
	}
	outWeight := uint32(1)
	for i := 1; i < deltaBlock; i++ {
		outWeight *= deltaPrime
	}
	p, s := 0, 0
	h := deltaFingerprint(target[:deltaBlock])
	for p+deltaBlock <= len(target) {
		if o, ok := index[h]; ok && bytes.Equal(base[o:o+deltaBlock], target[p:p+deltaBlock]) {
			bp, bo := p, o
			for bp > s && bo > 0 && base[bo-1] == target[bp-1] {
				bp--
				bo--
			}
			e, eo := p+deltaBlock, o+deltaBlock
			for e < len(target) && eo < len(base) && target[e] == base[eo] {
				e++
				eo++
			}
			if bp > s {
				out = deltaInsert(out, target[s:bp])
			}
			out = deltaCopy(out, bo, e-bp)
			p, s = e, e
			if p+deltaBlock <= len(target) {
				h = deltaFingerprint(target[p : p+deltaBlock])
			}
			continue
		}
		if p+deltaBlock < len(target) {
			h = (h-uint32(target[p])*outWeight)*deltaPrime + uint32(target[p+deltaBlock])
		}
		p++
	}
	if s < len(target) {
		out = deltaInsert(out, target[s:])
	}
	return out
}

// applyDelta applies delta to base; the result is at most maxOut bytes.
func applyDelta(base, delta []byte, maxOut uint64) ([]byte, error) {
	bad := func(m string) error { return packCorrupt("delta: %s", m) }
	baseLen, n := binary.Uvarint(delta)
	if n <= 0 {
		return nil, bad("bad base size")
	}
	pos := n
	if baseLen != uint64(len(base)) {
		return nil, bad("base size mismatch")
	}
	targetLen, n := binary.Uvarint(delta[pos:])
	if n <= 0 {
		return nil, bad("bad target size")
	}
	pos += n
	if targetLen > maxOut {
		return nil, bad("target too large")
	}
	out := make([]byte, 0, targetLen)
	for pos < len(delta) {
		op := delta[pos]
		pos++
		switch op {
		case deltaOpInsert:
			l, n := binary.Uvarint(delta[pos:])
			if n <= 0 {
				return nil, bad("bad insert length")
			}
			pos += n
			if l == 0 || l > uint64(len(delta)-pos) || l > targetLen-uint64(len(out)) {
				return nil, bad("insert out of range")
			}
			out = append(out, delta[pos:pos+int(l)]...)
			pos += int(l)
		case deltaOpCopy:
			off, n := binary.Uvarint(delta[pos:])
			if n <= 0 {
				return nil, bad("bad copy offset")
			}
			pos += n
			l, n := binary.Uvarint(delta[pos:])
			if n <= 0 {
				return nil, bad("bad copy length")
			}
			pos += n
			end := off + l
			if l == 0 || end < off || end > uint64(len(base)) || l > targetLen-uint64(len(out)) {
				return nil, bad("copy out of range")
			}
			out = append(out, base[off:end]...)
		default:
			return nil, bad("unknown op")
		}
	}
	if uint64(len(out)) != targetLen {
		return nil, bad("target size mismatch")
	}
	return out, nil
}

// ---- zstd ----

var (
	packEncoder, _ = zstd.NewWriter(nil, zstd.WithEncoderLevel(zstd.SpeedDefault), zstd.WithEncoderConcurrency(1))
	packDecoder, _ = zstd.NewReader(nil, zstd.WithDecoderConcurrency(0), zstd.WithDecoderMaxMemory(packMaxPayload))
)

// ---- writer ----

// packStats describes what a packWriter produced.
type packStats struct {
	Entries     int
	Deltas      int
	ObjectBytes int64 // total size of the objects carried
	PackBytes   int64 // bytes of the pack
}

// packWriter writes a pack entry by entry.
type packWriter struct {
	w        io.Writer
	hasher   *blake3.Hasher
	compress bool
	deltas   bool
	depth    map[hash.Hash]uint8
	stats    packStats
	err      error
}

// newPackWriter writes the header; compress selects zstd payloads (off =
// raw, deterministic packs).
func newPackWriter(w io.Writer, compress bool) *packWriter {
	pw := &packWriter{w: w, hasher: blake3.New(), compress: compress, deltas: true, depth: make(map[hash.Hash]uint8)}
	pw.emit([]byte{packMagic[0], packMagic[1], packMagic[2], packMagic[3], packVersion, 0, 0, 0})
	return pw
}

func (pw *packWriter) emit(b []byte) {
	if pw.err != nil {
		return
	}
	if _, err := pw.w.Write(b); err != nil {
		pw.err = err
		return
	}
	_, _ = pw.hasher.Write(b)
	pw.stats.PackBytes += int64(len(b))
}

// has reports whether h was already written.
func (pw *packWriter) has(h hash.Hash) bool {
	_, ok := pw.depth[h]
	return ok
}

// depthOf returns the delta depth of an entry of this pack (0 for objects
// outside it).
func (pw *packWriter) depthOf(h hash.Hash) uint8 { return pw.depth[h] }

// add writes an object (kind = ObjectKind wire value), as a delta against
// base (an object the receiver has, or an earlier entry) when that saves
// at least 25 % and the chain stays within PackMaxDeltaDepth. It reports
// whether a delta was written; objects already in the pack are skipped.
func (pw *packWriter) add(h hash.Hash, kind uint8, data []byte, baseHash *hash.Hash, baseData []byte) (bool, error) {
	if pw.err != nil {
		return false, pw.err
	}
	if _, dup := pw.depth[h]; dup {
		return false, nil
	}
	pw.stats.Entries++
	pw.stats.ObjectBytes += int64(len(data))
	if baseHash != nil && pw.deltas && *baseHash != h {
		d := pw.depth[*baseHash]
		if d < PackMaxDeltaDepth && len(data) >= deltaBlock && len(baseData) <= packMaxDeltaBase {
			delta := encodeDelta(baseData, data)
			if deltaPaysOff(len(delta), len(data)) {
				pw.entry(packTypeDelta, kind, h, baseHash, len(data), delta)
				pw.depth[h] = d + 1
				pw.stats.Deltas++
				return true, pw.err
			}
		}
	}
	pw.entry(packTypeWhole, kind, h, nil, len(data), data)
	pw.depth[h] = 0
	return false, pw.err
}

func (pw *packWriter) entry(typ, kind uint8, h hash.Hash, base *hash.Hash, size int, raw []byte) {
	codec, payload := uint8(packCodecRaw), raw
	if pw.compress && len(raw) >= packMinCompress {
		if z := packEncoder.EncodeAll(raw, make([]byte, 0, len(raw)/2+64)); len(z) < len(raw) {
			codec, payload = packCodecZstd, z
		}
	}
	head := make([]byte, 0, 3+64+30)
	head = append(head, typ, kind, codec)
	head = append(head, h[:]...)
	if base != nil {
		head = append(head, base[:]...)
	}
	head = appendUvarint(head, uint64(size))
	head = appendUvarint(head, uint64(len(raw)))
	head = appendUvarint(head, uint64(len(payload)))
	pw.emit(head)
	pw.emit(payload)
}

// finish writes the end marker and trailer.
func (pw *packWriter) finish() (packStats, error) {
	pw.emit([]byte{packTypeEnd})
	if pw.err != nil {
		return pw.stats, pw.err
	}
	sum := pw.hasher.Sum(nil)
	if _, err := pw.w.Write(sum); err != nil {
		return pw.stats, err
	}
	pw.stats.PackBytes += int64(len(sum))
	return pw.stats, nil
}

// ---- reader ----

// packObject is one object read from a pack. Err (with Data nil) explains
// why the entry is unusable (missing base, bad delta, hash mismatch); such
// entries do not invalidate the rest of the pack.
type packObject struct {
	Hash hash.Hash
	Kind uint8
	Base *hash.Hash
	Data []byte
	Err  error
}

// packReader reads a pack, verifying object hashes, the delta depth and
// (after the end marker) the trailer.
type packReader struct {
	r        *bufio.Reader
	hasher   *blake3.Hasher
	consumed int64
	depth    map[hash.Hash]uint8
	done     bool
}

func packReadErr(err error) error {
	if errors.Is(err, io.EOF) || errors.Is(err, io.ErrUnexpectedEOF) {
		return errPackTruncated
	}
	return err
}

// newPackReader reads and checks the header.
func newPackReader(r io.Reader) (*packReader, error) {
	pr := &packReader{r: bufio.NewReaderSize(r, 256<<10), hasher: blake3.New(), depth: make(map[hash.Hash]uint8)}
	var head [packHeaderLen]byte
	if err := pr.fill(head[:]); err != nil {
		return nil, err
	}
	if !bytes.Equal(head[:4], packMagic[:]) {
		return nil, packCorrupt("not a pack (bad magic)")
	}
	if head[4] != packVersion {
		return nil, packCorrupt("unsupported pack version %d", head[4])
	}
	if head[5] != 0 || head[6] != 0 || head[7] != 0 {
		return nil, packCorrupt("reserved header bytes are not zero")
	}
	return pr, nil
}

func (pr *packReader) fill(b []byte) error {
	if _, err := io.ReadFull(pr.r, b); err != nil {
		return packReadErr(err)
	}
	_, _ = pr.hasher.Write(b)
	pr.consumed += int64(len(b))
	return nil
}

func (pr *packReader) byte() (byte, error) {
	var b [1]byte
	err := pr.fill(b[:])
	return b[0], err
}

func (pr *packReader) uvarint() (uint64, error) {
	var x uint64
	var s uint
	for i := 0; i < binary.MaxVarintLen64; i++ {
		b, err := pr.byte()
		if err != nil {
			return 0, err
		}
		if b < 0x80 {
			if i == binary.MaxVarintLen64-1 && b > 1 {
				break
			}
			return x | uint64(b)<<s, nil
		}
		x |= uint64(b&0x7f) << s
		s += 7
	}
	return 0, packCorrupt("varint overflows 64 bits")
}

// next returns the next object, or nil after the end marker (the trailer
// is then verified and nothing may follow). lookup returns the content of
// a delta base (an earlier object of the pack or one the receiver has);
// ok=false when unavailable.
func (pr *packReader) next(lookup func(hash.Hash) ([]byte, bool)) (*packObject, error) {
	if pr.done {
		return nil, nil
	}
	typ, err := pr.byte()
	if err != nil {
		return nil, err
	}
	if typ == packTypeEnd {
		return nil, pr.finishTrailer()
	}
	if typ != packTypeWhole && typ != packTypeDelta {
		return nil, packCorrupt("unknown entry type %d", typ)
	}
	kind, err := pr.byte()
	if err != nil {
		return nil, err
	}
	if kind > 3 {
		return nil, packCorrupt("unknown object kind %d", kind)
	}
	codec, err := pr.byte()
	if err != nil {
		return nil, err
	}
	if codec != packCodecRaw && codec != packCodecZstd {
		return nil, packCorrupt("unknown codec %d", codec)
	}
	obj := &packObject{Kind: kind}
	if err := pr.fill(obj.Hash[:]); err != nil {
		return nil, err
	}
	if typ == packTypeDelta {
		var b hash.Hash
		if err := pr.fill(b[:]); err != nil {
			return nil, err
		}
		obj.Base = &b
	}
	var vals [3]uint64
	for i := range vals {
		if vals[i], err = pr.uvarint(); err != nil {
			return nil, err
		}
	}
	size, rawLen, payLen := vals[0], vals[1], vals[2]
	switch {
	case size > PackMaxObject:
		return nil, packCorrupt("object larger than 64 MiB")
	case rawLen > packMaxPayload || payLen > packMaxPayload:
		return nil, packCorrupt("payload too large")
	case typ == packTypeWhole && rawLen != size:
		return nil, packCorrupt("whole entry size mismatch")
	case codec == packCodecRaw && payLen != rawLen:
		return nil, packCorrupt("raw payload length mismatch")
	}
	payload := make([]byte, payLen)
	if err := pr.fill(payload); err != nil {
		return nil, err
	}
	raw := payload
	if codec == packCodecZstd {
		out, err := packDecoder.DecodeAll(payload, make([]byte, 0, rawLen))
		if err != nil {
			return nil, packCorrupt("zstd: %v", err)
		}
		if uint64(len(out)) != rawLen {
			return nil, packCorrupt("zstd payload length mismatch")
		}
		raw = out
	}

	var depth uint8
	if obj.Base != nil {
		depth = pr.depth[*obj.Base] + 1
	}
	if depth > PackMaxDeltaDepth {
		return nil, packCorrupt("delta chain deeper than %d", PackMaxDeltaDepth)
	}
	pr.depth[obj.Hash] = depth

	data := raw
	if obj.Base != nil {
		baseData, ok := lookup(*obj.Base)
		if !ok {
			obj.Err = fmt.Errorf("delta base %s is not available", obj.Base.Hex())
			return obj, nil
		}
		if data, err = applyDelta(baseData, raw, size); err != nil {
			obj.Err = err
			return obj, nil
		}
	}
	switch {
	case uint64(len(data)) != size:
		obj.Err = errors.New("object size mismatch")
	case hash.Sum(data) != obj.Hash:
		obj.Err = errors.New("content does not match its hash")
	default:
		obj.Data = data
	}
	return obj, nil
}

func (pr *packReader) finishTrailer() error {
	sum := pr.hasher.Sum(nil)
	var trailer [packTrailer]byte
	if _, err := io.ReadFull(pr.r, trailer[:]); err != nil {
		return packReadErr(err)
	}
	pr.consumed += packTrailer
	if !bytes.Equal(sum, trailer[:]) {
		return errPackChecksum
	}
	if _, err := pr.r.ReadByte(); err == nil {
		return packCorrupt("data after the trailer")
	} else if !errors.Is(err, io.EOF) {
		return err
	}
	pr.done = true
	return nil
}
