package objstore

// Stored object encoding (docs/ENGINEERING_PLAN.md §14.6,
// crates/worktree-protocol/specs/storage/Compression.md).
//
// An object file holds either the raw content or the 4-byte magic "WTZ1"
// followed by one zstd frame of the content. Writers compress when the
// object is at least CompressMinSize bytes and compression saves at least
// 10 %; content that itself begins with the magic is always compressed so
// that a leading "WTZ1" is unambiguous. Readers accept both forms. Hashes
// are always over the uncompressed content, so object ids do not depend
// on the encoding and stores may mix raw and compressed files.

import (
	"bytes"
	"errors"
	"fmt"
	"sync"

	"github.com/klauspost/compress/zstd"
)

// CompressedMagic prefixes a zstd-compressed object file.
var CompressedMagic = [4]byte{'W', 'T', 'Z', '1'}

// CompressMinSize is the smallest object that writers compress (unless its
// content starts with CompressedMagic).
const CompressMinSize = 512

// maxDecodedSize bounds the memory one decoded object may use. Objects
// are at most 64 MiB on the wire; the margin covers locally built ones.
const maxDecodedSize = 1 << 30

// ErrCorrupt is returned by Get when a stored file is not a valid encoding
// of the object with the requested hash.
var ErrCorrupt = errors.New("objstore: stored object is corrupt")

var (
	codecOnce sync.Once
	encoder   *zstd.Encoder
	decoder   *zstd.Decoder
	codecErr  error
)

// codec returns the shared zstd encoder and decoder (both are safe for
// concurrent EncodeAll/DecodeAll).
func codec() (*zstd.Encoder, *zstd.Decoder, error) {
	codecOnce.Do(func() {
		encoder, codecErr = zstd.NewWriter(nil,
			zstd.WithEncoderLevel(zstd.SpeedDefault),
			zstd.WithEncoderConcurrency(1))
		if codecErr != nil {
			return
		}
		decoder, codecErr = zstd.NewReader(nil,
			zstd.WithDecoderConcurrency(0),
			zstd.WithDecoderMaxMemory(maxDecodedSize))
	})
	return encoder, decoder, codecErr
}

// hasMagic reports whether b starts with CompressedMagic.
func hasMagic(b []byte) bool {
	return len(b) >= len(CompressedMagic) && bytes.Equal(b[:len(CompressedMagic)], CompressedMagic[:])
}

// EncodeObject returns the stored form of content. With compress false
// only content that begins with the magic is compressed (that case is
// mandatory). The result may alias content.
func EncodeObject(content []byte, compress bool) ([]byte, error) {
	must := hasMagic(content)
	if !must && (!compress || len(content) < CompressMinSize) {
		return content, nil
	}
	enc, _, err := codec()
	if err != nil {
		return nil, fmt.Errorf("objstore: zstd: %w", err)
	}
	out := make([]byte, len(CompressedMagic), len(CompressedMagic)+len(content)/2+64)
	copy(out, CompressedMagic[:])
	out = enc.EncodeAll(content, out)
	// Keep it only when it saves at least 10 %.
	if !must && int64(len(out))*10 > int64(len(content))*9 {
		return content, nil
	}
	return out, nil
}

// DecodeObject returns the content of a stored object file: the zstd frame
// after a "WTZ1" prefix decompressed, anything else unchanged. Prefer
// DiskStore.Get, which also accepts legacy raw objects whose content
// happens to begin with the magic.
func DecodeObject(stored []byte) ([]byte, error) {
	if !hasMagic(stored) {
		return stored, nil
	}
	_, dec, err := codec()
	if err != nil {
		return nil, fmt.Errorf("objstore: zstd: %w", err)
	}
	out, err := dec.DecodeAll(stored[len(CompressedMagic):], nil)
	if err != nil {
		return nil, fmt.Errorf("%w: %v", ErrCorrupt, err)
	}
	return out, nil
}
