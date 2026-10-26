package codesearch

import (
	"container/list"
	"sync"

	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
)

// segKey identifies a segment: a tree's index of one manifest.
type segKey struct {
	tree     string
	manifest hash.Hash
}

// segCache keeps decoded segments in memory up to a byte budget, evicting
// the least recently used ones. Evicted segments stay on disk and are
// loaded again when a search needs them (without a data directory they
// are rebuilt). The most recently added segment is never evicted, so a
// single segment larger than the budget still works.
type segCache struct {
	mu        sync.Mutex
	limit     int64 // 0 = unlimited
	used      int64
	ll        *list.List // front = most recently used
	m         map[segKey]*list.Element
	evictions int64
}

type cacheEntry struct {
	key segKey
	seg *segment
}

func newSegCache(limit int64) *segCache {
	return &segCache{limit: limit, ll: list.New(), m: map[segKey]*list.Element{}}
}

func (c *segCache) setLimit(limit int64) {
	c.mu.Lock()
	defer c.mu.Unlock()
	c.limit = limit
	c.evictLocked(nil)
}

// get returns the cached segment and marks it recently used.
func (c *segCache) get(k segKey) *segment {
	c.mu.Lock()
	defer c.mu.Unlock()
	if el := c.m[k]; el != nil {
		c.ll.MoveToFront(el)
		return el.Value.(*cacheEntry).seg
	}
	return nil
}

// has reports whether k is cached without touching its recency.
func (c *segCache) has(k segKey) bool {
	c.mu.Lock()
	defer c.mu.Unlock()
	return c.m[k] != nil
}

// put adds (or replaces) a segment and evicts others beyond the limit.
func (c *segCache) put(k segKey, s *segment) {
	c.mu.Lock()
	defer c.mu.Unlock()
	if el := c.m[k]; el != nil {
		c.used -= el.Value.(*cacheEntry).seg.size
		el.Value.(*cacheEntry).seg = s
		c.used += s.size
		c.ll.MoveToFront(el)
	} else {
		c.m[k] = c.ll.PushFront(&cacheEntry{key: k, seg: s})
		c.used += s.size
	}
	c.evictLocked(c.m[k])
}

func (c *segCache) evictLocked(keep *list.Element) {
	for c.limit > 0 && c.used > c.limit {
		el := c.ll.Back()
		if el == nil || el == keep {
			return
		}
		e := el.Value.(*cacheEntry)
		c.ll.Remove(el)
		delete(c.m, e.key)
		c.used -= e.seg.size
		c.evictions++
	}
}

// remove drops one segment.
func (c *segCache) remove(k segKey) {
	c.mu.Lock()
	defer c.mu.Unlock()
	if el := c.m[k]; el != nil {
		c.ll.Remove(el)
		delete(c.m, k)
		c.used -= el.Value.(*cacheEntry).seg.size
	}
}

// removeTree drops every segment of a tree.
func (c *segCache) removeTree(tree string) {
	c.mu.Lock()
	defer c.mu.Unlock()
	for k, el := range c.m {
		if k.tree == tree {
			c.ll.Remove(el)
			delete(c.m, k)
			c.used -= el.Value.(*cacheEntry).seg.size
		}
	}
}

// treeUsage returns the cached bytes and segment count of a tree.
func (c *segCache) treeUsage(tree string) (bytes int64, n int) {
	c.mu.Lock()
	defer c.mu.Unlock()
	for k, el := range c.m {
		if k.tree == tree {
			bytes += el.Value.(*cacheEntry).seg.size
			n++
		}
	}
	return bytes, n
}

// stats returns the used bytes, limit, segment count and evictions.
func (c *segCache) stats() (used, limit int64, n int, evictions int64) {
	c.mu.Lock()
	defer c.mu.Unlock()
	return c.used, c.limit, len(c.m), c.evictions
}
