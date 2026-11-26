package objstore

import (
	"bytes"
	"context"
	"fmt"
	"sync"
	"testing"

	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
)

// TestPutDeferredThenFlush: deferred objects are readable at once, their
// directories wait for Flush, and Flush leaves nothing pending.
func TestPutDeferredThenFlush(t *testing.T) {
	ctx := context.Background()
	s := New(t.TempDir())
	var hashes []hash.Hash
	for i := 0; i < 50; i++ {
		data := []byte(fmt.Sprintf("object %d", i))
		h := hash.Sum(data)
		stored, err := s.PutDeferred(ctx, h, data)
		if err != nil || !stored {
			t.Fatalf("PutDeferred %d: stored=%v err=%v", i, stored, err)
		}
		hashes = append(hashes, h)
		got, err := s.Get(ctx, h)
		if err != nil || !bytes.Equal(got, data) {
			t.Fatalf("Get before Flush: %v", err)
		}
	}
	if stored, err := s.PutDeferred(ctx, hashes[0], []byte("object 0")); err != nil || stored {
		t.Fatalf("second PutDeferred: stored=%v err=%v", stored, err)
	}
	s.dirtyMu.Lock()
	pending := len(s.dirty)
	s.dirtyMu.Unlock()
	if pending == 0 {
		t.Fatal("no directory was recorded for the flush")
	}
	if err := s.Flush(ctx); err != nil {
		t.Fatal(err)
	}
	s.dirtyMu.Lock()
	pending = len(s.dirty)
	s.dirtyMu.Unlock()
	if pending != 0 {
		t.Fatalf("%d directories still pending after Flush", pending)
	}
	if err := s.Flush(ctx); err != nil { // nothing to do
		t.Fatal(err)
	}
}

// TestPutIsDurableWithoutFlush: the plain Put and PutVerified flush
// themselves, so callers that know nothing about batches lose nothing.
func TestPutIsDurableWithoutFlush(t *testing.T) {
	ctx := context.Background()
	s := New(t.TempDir())
	data := []byte("durable")
	if err := s.Put(ctx, hash.Sum(data), data); err != nil {
		t.Fatal(err)
	}
	s.dirtyMu.Lock()
	defer s.dirtyMu.Unlock()
	if len(s.dirty) != 0 {
		t.Fatal("Put left directories unsynced")
	}
}

// TestConcurrentDeferredAndFlush exercises the dirty-set and flush locks
// under the race detector.
func TestConcurrentDeferredAndFlush(t *testing.T) {
	ctx := context.Background()
	s := New(t.TempDir())
	var wg sync.WaitGroup
	for w := 0; w < 8; w++ {
		wg.Add(1)
		go func(w int) {
			defer wg.Done()
			for i := 0; i < 40; i++ {
				data := []byte(fmt.Sprintf("w%d-%d", w, i))
				if _, err := s.PutDeferred(ctx, hash.Sum(data), data); err != nil {
					t.Error(err)
					return
				}
				if i%10 == 9 {
					if err := s.Flush(ctx); err != nil {
						t.Error(err)
						return
					}
				}
			}
		}(w)
	}
	wg.Wait()
	if err := s.Flush(ctx); err != nil {
		t.Fatal(err)
	}
	n, _, err := s.Stats(ctx)
	if err != nil || n != 320 {
		t.Fatalf("objects = %d, err = %v", n, err)
	}
}
