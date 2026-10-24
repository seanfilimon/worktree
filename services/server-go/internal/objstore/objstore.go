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

// ErrNotFound is returned by Get when no object exists for the given
// hash. Callers should match with errors.Is.
var ErrNotFound = errors.New("objstore: object not found")

// ErrHashMismatch is returned by Put when the supplied data does not
// hash to the supplied hash. No bytes are persisted in this case.
var ErrHashMismatch = errors.New("objstore: data hash does not match expected")
