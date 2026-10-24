package objstore

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"testing"

	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
)

func TestConcurrent_SameHashWithReaders(t *testing.T) {
	store := newTestStore(t)
	ctx := context.Background()
	data := []byte(strings.Repeat("contended object ", 512))
	h := hash.Sum(data)
	const writers, readers = 64, 16

	var wg sync.WaitGroup
	errCh := make(chan error, writers+readers)
	start := make(chan struct{})
	for i := 0; i < writers; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			<-start
			if err := store.Put(ctx, h, data); err != nil {
				errCh <- err
			}
		}()
	}
	for i := 0; i < readers; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			<-start
			for j := 0; j < 20; j++ {
				got, err := store.Get(ctx, h)
				if errors.Is(err, ErrNotFound) {
					continue // not written yet
				}
				if err != nil {
					errCh <- err
					return
				}
				if hash.Sum(got) != h {
					errCh <- errors.New("reader observed partial or corrupt object")
					return
				}
			}
		}()
	}
	close(start)
	wg.Wait()
	close(errCh)
	for err := range errCh {
		t.Fatalf("concurrent op err: %v", err)
	}

	// Exactly the object, and no temp files left behind.
	entries, err := os.ReadDir(store.fanoutDir(h))
	if err != nil {
		t.Fatalf("ReadDir: %v", err)
	}
	for _, e := range entries {
		if strings.HasPrefix(e.Name(), "tmp-") {
			t.Fatalf("leftover temp file %s", e.Name())
		}
	}
	if len(entries) != 1 {
		t.Fatalf("fanout dir has %d entries, want 1", len(entries))
	}
}

func TestStatsAndList(t *testing.T) {
	store := newTestStore(t)
	ctx := context.Background()

	// Empty (the objects dir does not even exist yet).
	count, size, err := store.Stats(ctx)
	if err != nil || count != 0 || size != 0 {
		t.Fatalf("Stats on empty store = (%d, %d, %v), want (0, 0, nil)", count, size, err)
	}
	if err := store.List(ctx, func(hash.Hash) error {
		t.Fatal("List on empty store called fn")
		return nil
	}); err != nil {
		t.Fatalf("List on empty store: %v", err)
	}

	want := map[hash.Hash]int{}
	var wantBytes int64
	for i := 0; i < 50; i++ {
		data := []byte(strings.Repeat("y", i)) // includes the empty object
		h := hash.Sum(data)
		if err := store.Put(ctx, h, data); err != nil {
			t.Fatalf("Put: %v", err)
		}
		want[h] = len(data)
		wantBytes += int64(len(data))
	}
	// Foreign files and in-flight temp files must be ignored.
	oneByte := hash.Sum([]byte("y"))
	if err := os.WriteFile(filepath.Join(store.fanoutDir(oneByte), "tmp-123"), []byte("junk"), 0o644); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(store.Root(), "objects", "README"), []byte("junk"), 0o644); err != nil {
		t.Fatal(err)
	}
	if err := os.MkdirAll(filepath.Join(store.Root(), "objects", "zz"), 0o755); err != nil {
		t.Fatal(err)
	}

	count, size, err = store.Stats(ctx)
	if err != nil {
		t.Fatalf("Stats: %v", err)
	}
	if count != int64(len(want)) || size != wantBytes {
		t.Fatalf("Stats = (%d, %d), want (%d, %d)", count, size, len(want), wantBytes)
	}

	var listed []hash.Hash
	if err := store.List(ctx, func(h hash.Hash) error {
		listed = append(listed, h)
		return nil
	}); err != nil {
		t.Fatalf("List: %v", err)
	}
	if len(listed) != len(want) {
		t.Fatalf("List visited %d objects, want %d", len(listed), len(want))
	}
	for i, h := range listed {
		if _, ok := want[h]; !ok {
			t.Fatalf("List returned unknown hash %s", h)
		}
		if i > 0 && listed[i-1].Hex() >= h.Hex() {
			t.Fatalf("List not in ascending hash order at %d", i)
		}
	}

	// fn errors stop iteration and are returned unchanged.
	errStop := errors.New("stop")
	calls := 0
	err = store.List(ctx, func(hash.Hash) error {
		calls++
		if calls == 3 {
			return errStop
		}
		return nil
	})
	if err != errStop || calls != 3 {
		t.Fatalf("List with stopping fn: err = %v, calls = %d; want errStop after 3", err, calls)
	}

	// Canceled context.
	cctx, cancel := context.WithCancel(ctx)
	cancel()
	if _, _, err := store.Stats(cctx); !errors.Is(err, context.Canceled) {
		t.Fatalf("Stats with canceled ctx: %v", err)
	}
	if err := store.List(cctx, func(hash.Hash) error { return nil }); !errors.Is(err, context.Canceled) {
		t.Fatalf("List with canceled ctx: %v", err)
	}

	// Deleting is reflected in the stats.
	if err := store.Delete(ctx, oneByte); err != nil {
		t.Fatal(err)
	}
	count, size, err = store.Stats(ctx)
	if err != nil || count != int64(len(want)-1) || size != wantBytes-1 {
		t.Fatalf("Stats after delete = (%d, %d, %v), want (%d, %d)", count, size, err, len(want)-1, wantBytes-1)
	}
}

func TestInstall_ExistingDestinationWithSameContentIsSuccess(t *testing.T) {
	store := newTestStore(t)
	ctx := context.Background()
	data := []byte("pre-existing")
	h := hash.Sum(data)
	if err := store.Put(ctx, h, data); err != nil {
		t.Fatal(err)
	}
	// Simulate the losing side of a race: a temp file being installed over
	// an identical existing object while a reader holds it open (on
	// Windows this makes the rename fail with "Access is denied").
	f, err := os.Open(store.objectPath(h))
	if err != nil {
		t.Fatal(err)
	}
	defer f.Close()
	tmp := filepath.Join(store.fanoutDir(h), "tmp-race")
	if err := os.WriteFile(tmp, data, 0o644); err != nil {
		t.Fatal(err)
	}
	renamed, err := store.install(ctx, tmp, store.objectPath(h), data)
	if err != nil {
		t.Fatalf("install over identical existing object: %v", err)
	}
	if !renamed {
		_ = os.Remove(tmp)
	}
	got, err := store.Get(ctx, h)
	if err != nil || string(got) != string(data) {
		t.Fatalf("Get = (%q, %v)", got, err)
	}
}

func TestStoreOptionalInterfaces(t *testing.T) {
	var s Store = New(t.TempDir())
	if _, ok := s.(StatStore); !ok {
		t.Fatal("DiskStore does not implement StatStore")
	}
	if _, ok := s.(ListStore); !ok {
		t.Fatal("DiskStore does not implement ListStore")
	}
}
