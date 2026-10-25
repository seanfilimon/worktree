// Package content reads file contents out of the object store, resolving
// chunked large files (docs/ENGINEERING_PLAN.md §13.2).
//
// A manifest entry with chunked=false names a BLOB holding the whole file.
// With chunked=true it names a CHUNK_LIST object (protobuf ChunkList) whose
// chunks are BLOBs; the file is their concatenation in order.
package content

import (
	"context"
	"errors"
	"fmt"

	"google.golang.org/protobuf/proto"

	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/objstore"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
)

// MaxFileSize bounds the size of a file reassembled in memory.
const MaxFileSize = 4 << 30

// ErrInvalidChunkList reports a malformed CHUNK_LIST object.
var ErrInvalidChunkList = errors.New("content: invalid chunk list")

// EntryHash returns the object hash a manifest entry points at.
func EntryHash(e *syncpb.ManifestEntry) (hash.Hash, error) {
	var h hash.Hash
	v := e.GetBlobHash().GetValue()
	if len(v) != hash.Size {
		return h, fmt.Errorf("content: entry %q has a malformed hash", e.GetPath())
	}
	copy(h[:], v)
	return h, nil
}

// DecodeChunkList decodes and validates a CHUNK_LIST object.
func DecodeChunkList(data []byte) (*syncpb.ChunkList, error) {
	cl := &syncpb.ChunkList{}
	if err := proto.Unmarshal(data, cl); err != nil {
		return nil, fmt.Errorf("%w: %v", ErrInvalidChunkList, err)
	}
	var total uint64
	for i, c := range cl.GetChunks() {
		if len(c.GetHash().GetValue()) != hash.Size {
			return nil, fmt.Errorf("%w: chunk %d has a malformed hash", ErrInvalidChunkList, i)
		}
		total += c.GetSize()
	}
	if total != cl.GetSize() {
		return nil, fmt.Errorf("%w: chunk sizes sum to %d, want %d", ErrInvalidChunkList, total, cl.GetSize())
	}
	return cl, nil
}

// ChunkHashes returns the chunk hashes of a chunk list, in order.
func ChunkHashes(cl *syncpb.ChunkList) []hash.Hash {
	out := make([]hash.Hash, len(cl.GetChunks()))
	for i, c := range cl.GetChunks() {
		copy(out[i][:], c.GetHash().GetValue())
	}
	return out
}

// EntryObjects returns every object needed to materialize the entry: the
// blob, or the chunk list followed by its chunks.
func EntryObjects(ctx context.Context, objs objstore.Store, e *syncpb.ManifestEntry) ([]hash.Hash, error) {
	h, err := EntryHash(e)
	if err != nil {
		return nil, err
	}
	if !e.GetChunked() {
		return []hash.Hash{h}, nil
	}
	data, err := objs.Get(ctx, h)
	if err != nil {
		return nil, err
	}
	cl, err := DecodeChunkList(data)
	if err != nil {
		return nil, err
	}
	return append([]hash.Hash{h}, ChunkHashes(cl)...), nil
}

// ReadEntry returns the complete file content of a manifest entry.
func ReadEntry(ctx context.Context, objs objstore.Store, e *syncpb.ManifestEntry) ([]byte, error) {
	h, err := EntryHash(e)
	if err != nil {
		return nil, err
	}
	data, err := objs.Get(ctx, h)
	if err != nil || !e.GetChunked() {
		return data, err
	}
	cl, err := DecodeChunkList(data)
	if err != nil {
		return nil, err
	}
	if cl.GetSize() > MaxFileSize {
		return nil, fmt.Errorf("content: %q is too large to read in memory (%d bytes)", e.GetPath(), cl.GetSize())
	}
	out := make([]byte, 0, cl.GetSize())
	for _, ch := range ChunkHashes(cl) {
		part, err := objs.Get(ctx, ch)
		if err != nil {
			return nil, fmt.Errorf("content: chunk %s of %q: %w", ch.Hex(), e.GetPath(), err)
		}
		out = append(out, part...)
	}
	if uint64(len(out)) != cl.GetSize() {
		return nil, fmt.Errorf("%w: %q reassembled to %d bytes, want %d", ErrInvalidChunkList, e.GetPath(), len(out), cl.GetSize())
	}
	return out, nil
}
