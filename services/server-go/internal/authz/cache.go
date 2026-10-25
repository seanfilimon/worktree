package authz

import (
	"context"
	"errors"
	"sync"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// requestCache memoizes store lookups for the lifetime of one request.
//
// Every Evaluate / Authorize call uses one: the cache attached to the
// context with WithRequestCache when there is one, else a fresh cache for
// that call alone. Scoping the cache to a request (instead of a time-based
// cache shared by all requests) means authorization never acts on stale
// data — a membership, role or policy change is visible to the very next
// request — while a request that authorizes several times (a push checks
// tree:write, branch:push and branch protection) reads each account,
// membership, grant list and policy list from the store only once.
//
// Successful results and ErrNotFound are cached; other errors are not.
// Cached values are shared between lookups and must not be modified.
type requestCache struct {
	mu sync.Mutex
	m  map[string]cacheEntry
}

type cacheEntry struct {
	v   any
	err error
}

type cacheCtxKey struct{}

// WithRequestCache returns a context whose authorization lookups are
// memoized until the context is discarded. Use it around a unit of work
// that authorizes several times with the same principal and resources (one
// RPC or HTTP request). Calling it on a context that already carries a
// cache returns ctx unchanged.
func WithRequestCache(ctx context.Context) context.Context {
	if cacheFrom(ctx) != nil {
		return ctx
	}
	return context.WithValue(ctx, cacheCtxKey{}, newRequestCache())
}

func newRequestCache() *requestCache { return &requestCache{m: map[string]cacheEntry{}} }

func cacheFrom(ctx context.Context) *requestCache {
	c, _ := ctx.Value(cacheCtxKey{}).(*requestCache)
	return c
}

// cached returns the memoized result for key, calling load on a miss.
// Concurrent misses for the same key may both call load; that is harmless.
func cached[T any](c *requestCache, key string, load func() (T, error)) (T, error) {
	if c == nil {
		return load()
	}
	c.mu.Lock()
	e, ok := c.m[key]
	c.mu.Unlock()
	if ok {
		v, _ := e.v.(T)
		return v, e.err
	}
	v, err := load()
	if err == nil || errors.Is(err, domain.ErrNotFound) {
		c.mu.Lock()
		c.m[key] = cacheEntry{v: v, err: err}
		c.mu.Unlock()
	}
	return v, err
}
