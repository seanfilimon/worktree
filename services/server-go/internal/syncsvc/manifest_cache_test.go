package syncsvc

import (
	"context"
	"errors"
	"testing"

	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/objstore"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
)

// TestLoadManifestCachesButHonorsDeletes: a cached manifest is served
// without reading the object again, but an object removed from the store
// (garbage collection) is ErrNotFound even while its decode is cached.
func TestLoadManifestCachesButHonorsDeletes(t *testing.T) {
	ctx := context.Background()
	objs := newMemObjects()
	s := New(Deps{Objects: objs})
	m, _ := benchManifest(50, 0, 0)
	data := marshalManifest(t, m)
	h := hash.Sum(data)
	if err := objs.Put(ctx, h, data); err != nil {
		t.Fatal(err)
	}
	first, err := s.loadManifest(ctx, h)
	if err != nil {
		t.Fatal(err)
	}
	second, err := s.loadManifest(ctx, h)
	if err != nil {
		t.Fatal(err)
	}
	if first != second {
		t.Fatal("second load was not served from the cache")
	}
	if len(second.GetEntries()) != 50 {
		t.Fatalf("entries = %d", len(second.GetEntries()))
	}
	if err := objs.Delete(ctx, h); err != nil {
		t.Fatal(err)
	}
	if _, err := s.loadManifest(ctx, h); !errors.Is(err, objstore.ErrNotFound) {
		t.Fatalf("deleted manifest: err = %v, want ErrNotFound", err)
	}
}

// TestManifestCacheEvictsByCost keeps the estimated size within bounds,
// least recently used first, and never admits one oversized manifest.
func TestManifestCacheEvictsByCost(t *testing.T) {
	c := newManifestCache(manifestCost(1000))
	h := func(i byte) hash.Hash { return hash.Sum([]byte{i}) }
	rec := &syncpb.ManifestRecord{}
	c.add(h(1), rec, 240)
	c.add(h(2), rec, 240)
	c.add(h(3), rec, 240)
	if _, ok := c.get(h(1)); !ok { // 1 becomes most recently used
		t.Fatal("1 missing")
	}
	c.add(h(4), rec, 240)
	c.add(h(5), rec, 240)
	if _, ok := c.get(h(2)); ok {
		t.Fatal("least recently used entry 2 was not evicted")
	}
	for _, i := range []byte{1, 4, 5} {
		if _, ok := c.get(h(i)); !ok {
			t.Fatalf("%d evicted", i)
		}
	}
	c.add(h(9), rec, 300) // > max/4
	if _, ok := c.get(h(9)); ok {
		t.Fatal("oversized manifest admitted")
	}
	if c.used > c.max {
		t.Fatalf("used %d > max %d", c.used, c.max)
	}
}

// TestHasAllKeepsOrder: concurrent existence checks answer per position.
func TestHasAllKeepsOrder(t *testing.T) {
	ctx := context.Background()
	objs := newMemObjects()
	s := New(Deps{Objects: objs})
	var hashes []hash.Hash
	for i := 0; i < 500; i++ {
		data := []byte{byte(i), byte(i >> 8), 7}
		h := hash.Sum(data)
		hashes = append(hashes, h)
		if i%3 == 0 {
			if err := objs.Put(ctx, h, data); err != nil {
				t.Fatal(err)
			}
		}
	}
	got, err := s.hasAll(ctx, hashes)
	if err != nil {
		t.Fatal(err)
	}
	for i, ok := range got {
		if ok != (i%3 == 0) {
			t.Fatalf("hash %d: present = %v", i, ok)
		}
	}
}
