package objstore

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"io/fs"
	"os"
	"path/filepath"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
)

// Retry policy for transient filesystem errors (Windows sharing
// violations). The budget is generous because the holder of the
// conflicting handle is usually another goroutine finishing a read or a
// rename within microseconds, but anti-virus scanners can hold freshly
// written files for tens of milliseconds.
const (
	retryInitialDelay = time.Millisecond
	retryMaxDelay     = 50 * time.Millisecond
	retryBudget       = 2 * time.Second
)

// DiskStore is a filesystem-backed [Store].
//
// Objects live at <root>/objects/<2-hex>/<62-hex>. The objects/
// subtree and per-hash fanout directories are created lazily on
// first Put; the configured root is NOT created at construction so
// callers can decide when (and with what mode) to materialize it.
//
// DiskStore is safe for concurrent use, including concurrent Puts of the
// same object on Windows, where renaming onto an existing or open file
// fails with "Access is denied".
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

// objectsDir returns <root>/objects.
func (s *DiskStore) objectsDir() string {
	return filepath.Join(s.root, "objects")
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
	// On any path where the temp file was not renamed into place, ensure
	// it does not linger.
	renamed := false
	defer func() {
		if !renamed {
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

	renamed, err = s.install(ctx, tmpPath, s.objectPath(h), data)
	return err
}

// install renames tmpPath onto final. It reports whether the rename
// happened; (false, nil) means final already held exactly data (a
// concurrent Put of the same object won), in which case the caller must
// discard tmpPath.
//
// On Windows, MoveFileEx onto a destination that another goroutine has
// open or is concurrently replacing fails with ERROR_ACCESS_DENIED or
// ERROR_SHARING_VIOLATION. Because the store is content-addressed, a
// destination that already holds the expected bytes is success; otherwise
// transient errors are retried with exponential backoff.
func (s *DiskStore) install(ctx context.Context, tmpPath, final string, data []byte) (bool, error) {
	delay := retryInitialDelay
	deadline := time.Now().Add(retryBudget)
	for {
		err := os.Rename(tmpPath, final)
		if err == nil {
			return true, nil
		}
		if hasContent(final, data) {
			return false, nil
		}
		if !isTransientFSError(err) || time.Now().After(deadline) {
			return false, fmt.Errorf("objstore: rename to final path: %w", err)
		}
		if err := sleepCtx(ctx, delay); err != nil {
			return false, err
		}
		delay = min(delay*2, retryMaxDelay)
	}
}

// hasContent reports whether the file at path exists and holds exactly
// want. Any error (including a transient sharing violation) yields false.
func hasContent(path string, want []byte) bool {
	info, err := os.Stat(path)
	if err != nil || !info.Mode().IsRegular() || info.Size() != int64(len(want)) {
		return false
	}
	got, err := os.ReadFile(path)
	return err == nil && bytes.Equal(got, want)
}

// retryTransient runs op, retrying with backoff while it fails with a
// transient sharing violation and the retry budget is not exhausted.
func retryTransient(ctx context.Context, op func() error) error {
	delay := retryInitialDelay
	deadline := time.Now().Add(retryBudget)
	for {
		err := op()
		if err == nil || !isTransientFSError(err) || time.Now().After(deadline) {
			return err
		}
		if cerr := sleepCtx(ctx, delay); cerr != nil {
			return cerr
		}
		delay = min(delay*2, retryMaxDelay)
	}
}

// sleepCtx sleeps for d or until ctx is done, returning ctx.Err() in the
// latter case.
func sleepCtx(ctx context.Context, d time.Duration) error {
	t := time.NewTimer(d)
	defer t.Stop()
	select {
	case <-ctx.Done():
		return ctx.Err()
	case <-t.C:
		return nil
	}
}

// Get implements Store.Get.
func (s *DiskStore) Get(ctx context.Context, h hash.Hash) ([]byte, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	var data []byte
	err := retryTransient(ctx, func() error {
		var rerr error
		data, rerr = os.ReadFile(s.objectPath(h))
		return rerr
	})
	if err != nil {
		if errors.Is(err, fs.ErrNotExist) {
			return nil, ErrNotFound
		}
		if ctxErr := ctx.Err(); ctxErr != nil && errors.Is(err, ctxErr) {
			return nil, err
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
	err := retryTransient(ctx, func() error {
		_, serr := os.Stat(s.objectPath(h))
		return serr
	})
	if err == nil {
		return true, nil
	}
	if errors.Is(err, fs.ErrNotExist) {
		return false, nil
	}
	if ctxErr := ctx.Err(); ctxErr != nil && errors.Is(err, ctxErr) {
		return false, err
	}
	return false, fmt.Errorf("objstore: stat object: %w", err)
}

// Delete implements Store.Delete. Removing a non-existent object is
// a no-op and returns nil (idempotent delete).
func (s *DiskStore) Delete(ctx context.Context, h hash.Hash) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	err := retryTransient(ctx, func() error {
		return os.Remove(s.objectPath(h))
	})
	if err == nil || errors.Is(err, fs.ErrNotExist) {
		return nil
	}
	if ctxErr := ctx.Err(); ctxErr != nil && errors.Is(err, ctxErr) {
		return err
	}
	return fmt.Errorf("objstore: remove object: %w", err)
}

// Stats implements [StatStore]. It walks the object tree and returns the
// number of stored objects and their total size in bytes. In-flight
// temporary files and foreign files are ignored. Objects added or removed
// concurrently may or may not be counted.
func (s *DiskStore) Stats(ctx context.Context) (count, size int64, err error) {
	err = s.walk(ctx, func(_ hash.Hash, n int64) error {
		count++
		size += n
		return nil
	})
	if err != nil {
		return 0, 0, err
	}
	return count, size, nil
}

// List implements [ListStore]. It calls fn for every stored object in
// ascending hash order. Iteration stops at the first error returned by fn,
// which List returns unchanged. Objects added or removed concurrently may
// or may not be visited.
func (s *DiskStore) List(ctx context.Context, fn func(hash.Hash) error) error {
	return s.walk(ctx, func(h hash.Hash, _ int64) error { return fn(h) })
}

// walk visits every object file under <root>/objects in ascending hash
// order, passing its hash and size to fn. A missing objects directory is
// an empty store.
func (s *DiskStore) walk(ctx context.Context, fn func(h hash.Hash, size int64) error) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	fanouts, err := os.ReadDir(s.objectsDir())
	if err != nil {
		if errors.Is(err, fs.ErrNotExist) {
			return nil
		}
		return fmt.Errorf("objstore: read objects dir: %w", err)
	}
	// os.ReadDir returns entries sorted by name, and lowercase hex sorts
	// the same way as the raw bytes, so the walk is in hash order.
	for _, fan := range fanouts {
		if !fan.IsDir() || !isLowerHex(fan.Name(), 2) {
			continue
		}
		if err := ctx.Err(); err != nil {
			return err
		}
		dir := filepath.Join(s.objectsDir(), fan.Name())
		entries, err := os.ReadDir(dir)
		if err != nil {
			if errors.Is(err, fs.ErrNotExist) {
				continue // removed concurrently
			}
			return fmt.Errorf("objstore: read fanout dir: %w", err)
		}
		for _, e := range entries {
			if !e.Type().IsRegular() || !isLowerHex(e.Name(), hash.HexSize-2) {
				continue
			}
			h, err := hash.FromHex(fan.Name() + e.Name())
			if err != nil {
				continue
			}
			info, err := e.Info()
			if err != nil {
				if errors.Is(err, fs.ErrNotExist) {
					continue // deleted concurrently
				}
				return fmt.Errorf("objstore: stat object: %w", err)
			}
			if err := fn(h, info.Size()); err != nil {
				return err
			}
		}
	}
	return nil
}

// isLowerHex reports whether s is exactly n lowercase hex characters.
func isLowerHex(s string, n int) bool {
	if len(s) != n {
		return false
	}
	for i := 0; i < len(s); i++ {
		c := s[i]
		if (c < '0' || c > '9') && (c < 'a' || c > 'f') {
			return false
		}
	}
	return true
}

// Compile-time assertions that DiskStore satisfies the store interfaces.
var (
	_ Store     = (*DiskStore)(nil)
	_ StatStore = (*DiskStore)(nil)
	_ ListStore = (*DiskStore)(nil)
)
