package syncsvc

import (
	"container/list"
	"sync"

	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
)

// manifestCacheBytes bounds the memory of decoded manifests kept by a
// Service (estimated, see manifestCost).
const manifestCacheBytes = 64 << 20

// manifestCache is a small LRU of decoded, validated manifests keyed by
// their object hash. A push decodes the same parent and new manifests in
// several steps (object checks, changed paths, reachability) and clones
// and pulls decode the same history over and over; objects are immutable,
// so a decoded manifest stays valid for as long as the object exists.
//
// Cached records are shared: callers must treat them as read-only (no
// code in this package modifies a ManifestRecord it did not build).
type manifestCache struct {
	mu    sync.Mutex
	max   int64
	used  int64
	order *list.List // front = most recently used; values are *manifestItem
	items map[hash.Hash]*list.Element
}

type manifestItem struct {
	h    hash.Hash
	m    *syncpb.ManifestRecord
	cost int64
}

func newManifestCache(maxBytes int64) *manifestCache {
	return &manifestCache{max: maxBytes, order: list.New(), items: make(map[hash.Hash]*list.Element)}
}

// manifestCost estimates the memory of a decoded manifest from its
// encoded size (decoding expands entries into messages and strings).
func manifestCost(encoded int) int64 { return int64(encoded) * 4 }

func (c *manifestCache) get(h hash.Hash) (*syncpb.ManifestRecord, bool) {
	if c == nil {
		return nil, false
	}
	c.mu.Lock()
	defer c.mu.Unlock()
	el, ok := c.items[h]
	if !ok {
		return nil, false
	}
	c.order.MoveToFront(el)
	return el.Value.(*manifestItem).m, true
}

func (c *manifestCache) add(h hash.Hash, m *syncpb.ManifestRecord, encoded int) {
	if c == nil {
		return
	}
	cost := manifestCost(encoded)
	if cost > c.max/4 {
		return // one huge manifest must not flush everything else
	}
	c.mu.Lock()
	defer c.mu.Unlock()
	if _, ok := c.items[h]; ok {
		return
	}
	c.items[h] = c.order.PushFront(&manifestItem{h: h, m: m, cost: cost})
	c.used += cost
	for c.used > c.max {
		el := c.order.Back()
		it := el.Value.(*manifestItem)
		c.order.Remove(el)
		delete(c.items, it.h)
		c.used -= it.cost
	}
}
