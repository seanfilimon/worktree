package objstore

import (
	"context"
	"crypto/rand"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"testing"

	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
)

func newTestStore(t *testing.T) *DiskStore {
	t.Helper()
	return New(t.TempDir())
}

func TestPutGet_Roundtrip(t *testing.T) {
	store := newTestStore(t)
	ctx := context.Background()
	data := []byte("hello world")
	h := hash.Sum(data)

	if err := store.Put(ctx, h, data); err != nil {
		t.Fatalf("Put: %v", err)
	}
	got, err := store.Get(ctx, h)
	if err != nil {
		t.Fatalf("Get: %v", err)
	}
	if string(got) != string(data) {
		t.Fatalf("Get returned %q, want %q", got, data)
	}
}

func TestGet_UnknownHashReturnsErrNotFound(t *testing.T) {
	store := newTestStore(t)
	_, err := store.Get(context.Background(), hash.Sum([]byte("does not exist")))
	if !errors.Is(err, ErrNotFound) {
		t.Fatalf("Get on unknown hash: err = %v, want errors.Is ErrNotFound", err)
	}
}

func TestPut_HashMismatchRejected(t *testing.T) {
	store := newTestStore(t)
	ctx := context.Background()
	data := []byte("payload")
	wrongHash := hash.Sum([]byte("not-the-payload"))

	err := store.Put(ctx, wrongHash, data)
	if !errors.Is(err, ErrHashMismatch) {
		t.Fatalf("Put with mismatched hash: err = %v, want errors.Is ErrHashMismatch", err)
	}

	// Verify nothing was persisted at the wrong-hash path.
	if exists, _ := store.Has(ctx, wrongHash); exists {
		t.Fatal("Put with mismatched hash persisted bytes anyway")
	}

	// Verify no stray tmp files in the fanout dir (it may not even exist yet).
	fanout := store.fanoutDir(wrongHash)
	entries, err := os.ReadDir(fanout)
	if err == nil {
		for _, e := range entries {
			if strings.HasPrefix(e.Name(), "tmp-") {
				t.Fatalf("found leftover tmp file after mismatch: %s", e.Name())
			}
		}
	}
}

func TestPut_Idempotent(t *testing.T) {
	store := newTestStore(t)
	ctx := context.Background()
	data := []byte("idempotent")
	h := hash.Sum(data)

	for i := 0; i < 3; i++ {
		if err := store.Put(ctx, h, data); err != nil {
			t.Fatalf("Put #%d: %v", i, err)
		}
	}
	got, err := store.Get(ctx, h)
	if err != nil {
		t.Fatalf("Get after repeated Puts: %v", err)
	}
	if string(got) != string(data) {
		t.Fatalf("Get returned %q, want %q", got, data)
	}
}

func TestPut_CreatesFanoutDir(t *testing.T) {
	store := newTestStore(t)
	ctx := context.Background()
	data := []byte("fanout creation")
	h := hash.Sum(data)
	fanout := store.fanoutDir(h)

	if _, err := os.Stat(fanout); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("fanout dir exists before Put: stat err = %v (want NotExist)", err)
	}
	if err := store.Put(ctx, h, data); err != nil {
		t.Fatalf("Put: %v", err)
	}
	info, err := os.Stat(fanout)
	if err != nil {
		t.Fatalf("fanout dir missing after Put: %v", err)
	}
	if !info.IsDir() {
		t.Fatalf("expected %s to be a directory", fanout)
	}
}

func TestPut_OnDiskPathMatchesSpec(t *testing.T) {
	store := newTestStore(t)
	ctx := context.Background()
	data := []byte("path layout")
	h := hash.Sum(data)
	if err := store.Put(ctx, h, data); err != nil {
		t.Fatalf("Put: %v", err)
	}
	hex := h.Hex()
	want := filepath.Join(store.Root(), "objects", hex[:2], hex[2:])
	if _, err := os.Stat(want); err != nil {
		t.Fatalf("expected object at %s, stat err = %v", want, err)
	}
}

func TestHas(t *testing.T) {
	store := newTestStore(t)
	ctx := context.Background()
	data := []byte("has")
	h := hash.Sum(data)

	if exists, err := store.Has(ctx, h); err != nil || exists {
		t.Fatalf("Has before Put: (%v, %v), want (false, nil)", exists, err)
	}
	if err := store.Put(ctx, h, data); err != nil {
		t.Fatalf("Put: %v", err)
	}
	if exists, err := store.Has(ctx, h); err != nil || !exists {
		t.Fatalf("Has after Put: (%v, %v), want (true, nil)", exists, err)
	}
	if err := store.Delete(ctx, h); err != nil {
		t.Fatalf("Delete: %v", err)
	}
	if exists, err := store.Has(ctx, h); err != nil || exists {
		t.Fatalf("Has after Delete: (%v, %v), want (false, nil)", exists, err)
	}
}

func TestDelete_Idempotent(t *testing.T) {
	store := newTestStore(t)
	ctx := context.Background()
	h := hash.Sum([]byte("never put"))

	if err := store.Delete(ctx, h); err != nil {
		t.Fatalf("Delete on missing hash should be no-op, got err = %v", err)
	}
}

func TestDelete_ExistingObjectRemoved(t *testing.T) {
	store := newTestStore(t)
	ctx := context.Background()
	data := []byte("to be deleted")
	h := hash.Sum(data)

	if err := store.Put(ctx, h, data); err != nil {
		t.Fatalf("Put: %v", err)
	}
	if err := store.Delete(ctx, h); err != nil {
		t.Fatalf("Delete: %v", err)
	}
	_, err := store.Get(ctx, h)
	if !errors.Is(err, ErrNotFound) {
		t.Fatalf("Get after Delete: err = %v, want errors.Is ErrNotFound", err)
	}
}

func TestPut_EmptyData(t *testing.T) {
	store := newTestStore(t)
	ctx := context.Background()
	h := hash.Sum(nil)

	if err := store.Put(ctx, h, []byte{}); err != nil {
		t.Fatalf("Put empty: %v", err)
	}
	got, err := store.Get(ctx, h)
	if err != nil {
		t.Fatalf("Get empty: %v", err)
	}
	if len(got) != 0 {
		t.Fatalf("Get empty returned len = %d, want 0", len(got))
	}
	if exists, err := store.Has(ctx, h); err != nil || !exists {
		t.Fatalf("Has on empty: (%v, %v), want (true, nil)", exists, err)
	}
}

func TestPut_LargeData_10MiB(t *testing.T) {
	store := newTestStore(t)
	ctx := context.Background()

	data := make([]byte, 10*1024*1024) // 10 MiB
	if _, err := rand.Read(data); err != nil {
		t.Fatalf("rand.Read: %v", err)
	}
	h := hash.Sum(data)

	if err := store.Put(ctx, h, data); err != nil {
		t.Fatalf("Put 10MiB: %v", err)
	}
	got, err := store.Get(ctx, h)
	if err != nil {
		t.Fatalf("Get 10MiB: %v", err)
	}
	if len(got) != len(data) {
		t.Fatalf("Get returned %d bytes, want %d", len(got), len(data))
	}
	// Compare via hash (faster than bytewise on 10 MiB and equally strong).
	if hash.Sum(got) != h {
		t.Fatal("Get returned bytes that hash differently from the original")
	}
}

func TestConcurrent_DifferentHashes(t *testing.T) {
	store := newTestStore(t)
	ctx := context.Background()
	const n = 64

	type kv struct {
		h    hash.Hash
		data []byte
	}
	pairs := make([]kv, n)
	for i := range pairs {
		pairs[i].data = []byte(strings.Repeat("x", i+1))
		pairs[i].h = hash.Sum(pairs[i].data)
	}

	var wg sync.WaitGroup
	errCh := make(chan error, n)
	for i := range pairs {
		wg.Add(1)
		go func(p kv) {
			defer wg.Done()
			if err := store.Put(ctx, p.h, p.data); err != nil {
				errCh <- err
			}
		}(pairs[i])
	}
	wg.Wait()
	close(errCh)
	for err := range errCh {
		t.Fatalf("concurrent Put err: %v", err)
	}

	for _, p := range pairs {
		got, err := store.Get(ctx, p.h)
		if err != nil {
			t.Fatalf("Get after concurrent Put: %v", err)
		}
		if string(got) != string(p.data) {
			t.Fatalf("Get mismatch for hash %s: got %q, want %q", p.h, got, p.data)
		}
	}
}

func TestConcurrent_SameHash(t *testing.T) {
	store := newTestStore(t)
	ctx := context.Background()
	data := []byte("racy")
	h := hash.Sum(data)
	const n = 32

	var wg sync.WaitGroup
	errCh := make(chan error, n)
	for i := 0; i < n; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			if err := store.Put(ctx, h, data); err != nil {
				errCh <- err
			}
		}()
	}
	wg.Wait()
	close(errCh)
	for err := range errCh {
		t.Fatalf("concurrent same-hash Put err: %v", err)
	}

	got, err := store.Get(ctx, h)
	if err != nil {
		t.Fatalf("Get after concurrent same-hash Put: %v", err)
	}
	if string(got) != string(data) {
		t.Fatalf("Get returned %q, want %q", got, data)
	}
}

func TestPut_CanceledContextRejected(t *testing.T) {
	store := newTestStore(t)
	ctx, cancel := context.WithCancel(context.Background())
	cancel()

	data := []byte("never written")
	h := hash.Sum(data)
	err := store.Put(ctx, h, data)
	if !errors.Is(err, context.Canceled) {
		t.Fatalf("Put with canceled ctx: err = %v, want context.Canceled", err)
	}
	if exists, _ := store.Has(context.Background(), h); exists {
		t.Fatal("Put on canceled ctx persisted bytes anyway")
	}
}

func TestNew_DoesNotCreateRoot(t *testing.T) {
	// Use a subdirectory of TempDir that doesn't exist yet.
	parent := t.TempDir()
	root := filepath.Join(parent, "does-not-exist-yet")
	store := New(root)

	if _, err := os.Stat(root); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("New created root prematurely (stat err = %v)", err)
	}
	// First Put should create everything on demand.
	data := []byte("create on demand")
	h := hash.Sum(data)
	if err := store.Put(context.Background(), h, data); err != nil {
		t.Fatalf("Put on fresh root: %v", err)
	}
	if _, err := os.Stat(root); err != nil {
		t.Fatalf("root not created after Put: %v", err)
	}
}
