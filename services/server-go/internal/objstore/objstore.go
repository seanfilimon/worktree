// Package objstore implements a content-addressable byte store keyed
// by BLAKE3 hash.
//
// Objects are opaque byte sequences; the store performs no
// interpretation of their contents. Storage is shared across tenants
// (deduplicated by content) — per-tenant reference-graph isolation
// lives at a higher layer (WT-SRV-4).
//
// Per Server.md §14, on-disk layout is:
//
//	<storage_root>/objects/<2-hex>/<62-hex>
//
// The two-hex fanout caps per-directory entries at 256 and lines up
// with Git's well-trodden objects/<aa>/<bbb...> convention.
package objstore

import (
	"context"
	"errors"

	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
)

// Store is the abstract content-addressable byte store.
//
// All methods MUST be safe for concurrent use.
type Store interface {
	// Put writes data under h.
	//
	// If hash.Sum(data) != h, returns ErrHashMismatch and no bytes
	// are persisted. If an object with hash h already exists, Put is
	// a no-op and returns nil (content addressing makes this safe).
	Put(ctx context.Context, h hash.Hash, data []byte) error

	// Get returns the bytes previously stored under h, or
	// ErrNotFound if no such object exists.
	Get(ctx context.Context, h hash.Hash) ([]byte, error)

	// Has reports whether an object with hash h exists. Errors other
	// than not-found (e.g. permission denied on stat) are surfaced.
	Has(ctx context.Context, h hash.Hash) (bool, error)

	// Delete removes the object with hash h. Removing a non-existent
	// object is a no-op and returns nil.
	Delete(ctx context.Context, h hash.Hash) error
}

// VerifiedPutter is an optional interface implemented by stores that can
// persist data whose hash the caller has already verified, skipping the
// re-hash and reporting whether the object was newly stored.
type VerifiedPutter interface {
	// PutVerified stores data under h, which the caller guarantees is
	// hash.Sum(data). stored is false when the object already existed.
	PutVerified(ctx context.Context, h hash.Hash, data []byte) (stored bool, err error)
}

// PutVerified stores data (already verified to hash to h) in s with one
// existence check, through [VerifiedPutter] when s implements it. stored
// reports whether the object was newly written.
func PutVerified(ctx context.Context, s Store, h hash.Hash, data []byte) (stored bool, err error) {
	if vp, ok := s.(VerifiedPutter); ok {
		return vp.PutVerified(ctx, h, data)
	}
	exists, err := s.Has(ctx, h)
	if err != nil || exists {
		return false, err
	}
	return true, s.Put(ctx, h, data)
}

// BatchPutter is an optional interface implemented by stores that can
// share the expensive part of durability between many objects: a caller
// stores a batch with PutDeferred and calls Flush once before it tells
// anyone the objects are stored.
type BatchPutter interface {
	// PutDeferred is PutVerified, except that the object is only
	// guaranteed to survive a crash after the next successful Flush.
	PutDeferred(ctx context.Context, h hash.Hash, data []byte) (stored bool, err error)
	// Flush makes every object stored before the call durable.
	Flush(ctx context.Context) error
}

// PutDeferred stores verified data through [BatchPutter] when s implements
// it, else durably with [PutVerified]. Callers must call [Flush] before
// acknowledging the objects.
func PutDeferred(ctx context.Context, s Store, h hash.Hash, data []byte) (stored bool, err error) {
	if bp, ok := s.(BatchPutter); ok {
		return bp.PutDeferred(ctx, h, data)
	}
	return PutVerified(ctx, s, h, data)
}

// Flush completes the durability of objects stored with [PutDeferred].
func Flush(ctx context.Context, s Store) error {
	if bp, ok := s.(BatchPutter); ok {
		return bp.Flush(ctx)
	}
	return nil
}

// StatStore is an optional interface implemented by stores that can report
// aggregate usage (e.g. for the admin stats endpoint). Callers should
// type-assert a [Store] to StatStore and degrade gracefully if it is not
// implemented.
type StatStore interface {
	// Stats returns the number of stored objects and their total size in
	// bytes. The result is a point-in-time approximation when writes run
	// concurrently.
	Stats(ctx context.Context) (count, bytes int64, err error)
}

// ListStore is an optional interface implemented by stores that can
// enumerate their contents (e.g. for garbage collection or integrity
// checks).
type ListStore interface {
	// List calls fn once for every stored object, in ascending hash
	// order. If fn returns an error, iteration stops and List returns
	// that error unchanged.
	List(ctx context.Context, fn func(hash.Hash) error) error
}

// SizeStore is an optional interface implemented by stores that can report
// the size of one object without reading it.
type SizeStore interface {
	// Size returns the stored size of h in bytes, or ErrNotFound.
	Size(ctx context.Context, h hash.Hash) (int64, error)
}

// ObjectSize returns the size of h in s, using SizeStore when available
// (stored bytes: compressed objects report their compressed size, §14.6)
// and falling back to reading the object.
func ObjectSize(ctx context.Context, s Store, h hash.Hash) (int64, error) {
	if ss, ok := s.(SizeStore); ok {
		return ss.Size(ctx, h)
	}
	b, err := s.Get(ctx, h)
	if err != nil {
		return 0, err
	}
	return int64(len(b)), nil
}

// ErrNotFound is returned by Get when no object exists for the given
// hash. Callers should match with errors.Is.
var ErrNotFound = errors.New("objstore: object not found")

// ErrHashMismatch is returned by Put when the supplied data does not
// hash to the supplied hash. No bytes are persisted in this case.
var ErrHashMismatch = errors.New("objstore: data hash does not match expected")
