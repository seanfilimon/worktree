package objstore

import (
	"context"
	"errors"
	"fmt"
	"io/fs"
	"os"
	"path/filepath"

	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
)

// DiskStore is a filesystem-backed [Store].
//
// Objects live at <root>/objects/<2-hex>/<62-hex>. The objects/
// subtree and per-hash fanout directories are created lazily on
// first Put; the configured root is NOT created at construction so
// callers can decide when (and with what mode) to materialize it.
type DiskStore struct {
	root string
}

// New constructs a DiskStore rooted at root.
//
// root may be relative; it is used as supplied for path joining.
// Callers that want absolute-path safety should resolve before
// passing it in.
func New(root string) *DiskStore {
	return &DiskStore{root: root}
}

// Root returns the configured root directory.
func (s *DiskStore) Root() string {
	return s.root
}

// objectPath returns <root>/objects/<2-hex>/<62-hex>.
func (s *DiskStore) objectPath(h hash.Hash) string {
	hex := h.Hex()
	return filepath.Join(s.root, "objects", hex[:2], hex[2:])
}

// fanoutDir returns <root>/objects/<2-hex>.
func (s *DiskStore) fanoutDir(h hash.Hash) string {
	return filepath.Join(s.root, "objects", h.Hex()[:2])
}

// Put implements Store.Put.
func (s *DiskStore) Put(ctx context.Context, h hash.Hash, data []byte) error {
	if err := ctx.Err(); err != nil {
		return err
	}

	if hash.Sum(data) != h {
		return ErrHashMismatch
	}

	// Content-addressed idempotency: if the object already exists, no work to do.
	if exists, err := s.Has(ctx, h); err != nil {
		return err
	} else if exists {
		return nil
	}

	fanout := s.fanoutDir(h)
	if err := os.MkdirAll(fanout, 0o755); err != nil {
		return fmt.Errorf("objstore: mkdir fanout dir: %w", err)
	}

	tmp, err := os.CreateTemp(fanout, "tmp-*")
	if err != nil {
		return fmt.Errorf("objstore: create temp file: %w", err)
	}
	tmpPath := tmp.Name()
	// On any error path, ensure the temp file does not linger.
	// os.Remove on a path that no longer exists (post-rename) is a
	// harmless no-op apart from the returned err, which is ignored.
	removed := false
	defer func() {
		if !removed {
			_ = os.Remove(tmpPath)
		}
	}()

	if _, err := tmp.Write(data); err != nil {
		_ = tmp.Close()
		return fmt.Errorf("objstore: write temp file: %w", err)
	}
	if err := tmp.Sync(); err != nil {
		_ = tmp.Close()
		return fmt.Errorf("objstore: sync temp file: %w", err)
	}
	if err := tmp.Close(); err != nil {
		return fmt.Errorf("objstore: close temp file: %w", err)
	}

	final := s.objectPath(h)
	if err := os.Rename(tmpPath, final); err != nil {
		return fmt.Errorf("objstore: rename to final path: %w", err)
	}
	removed = true
	return nil
}

// Get implements Store.Get.
func (s *DiskStore) Get(ctx context.Context, h hash.Hash) ([]byte, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	data, err := os.ReadFile(s.objectPath(h))
	if err != nil {
		if errors.Is(err, fs.ErrNotExist) {
			return nil, ErrNotFound
		}
		return nil, fmt.Errorf("objstore: read object: %w", err)
	}
	return data, nil
}

// Has implements Store.Has.
func (s *DiskStore) Has(ctx context.Context, h hash.Hash) (bool, error) {
	if err := ctx.Err(); err != nil {
		return false, err
	}
	_, err := os.Stat(s.objectPath(h))
	if err == nil {
		return true, nil
	}
	if errors.Is(err, fs.ErrNotExist) {
		return false, nil
	}
	return false, fmt.Errorf("objstore: stat object: %w", err)
}

// Delete implements Store.Delete. Removing a non-existent object is
// a no-op and returns nil (idempotent delete).
func (s *DiskStore) Delete(ctx context.Context, h hash.Hash) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	err := os.Remove(s.objectPath(h))
	if err == nil {
		return nil
	}
	if errors.Is(err, fs.ErrNotExist) {
		return nil
	}
	return fmt.Errorf("objstore: remove object: %w", err)
}

// Compile-time assertion that DiskStore satisfies Store.
var _ Store = (*DiskStore)(nil)
