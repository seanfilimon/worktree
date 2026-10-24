package meta

import (
	"container/heap"
	"context"
	"errors"
	"fmt"
	"strings"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// NewHistory returns a [domain.History] that walks snapshot ancestry stored
// in store.
//
// Semantics beyond the interface contract:
//
//   - The empty snapshot id "" denotes the empty history (an empty
//     branch). It is an ancestor of every snapshot, Log("") and
//     Missing("", ...) return empty slices, and "" entries in haves are
//     ignored.
//   - A tip/descendant that does not exist fails with domain.ErrNotFound.
//     An ancestor or have that does not exist is simply not reachable
//     (haves the server has never seen are expected during pulls).
//   - A parent reference that cannot be resolved while walking (a corrupt
//     or truncated history) fails with domain.ErrNotFound.
//
// All operations are O(V + E) in the part of the DAG they visit (plus a
// log factor for ordering), memoize snapshot lookups within a call, and,
// when store implements [SnapshotViewer], read the whole walk from one
// consistent view.
func NewHistory(store domain.MetaStore) domain.History {
	return &history{store: store}
}

type history struct {
	store domain.MetaStore
}

// lookup memoizes snapshot reads for the duration of one History call.
type lookup struct {
	ctx   context.Context
	get   func(id string) (*domain.Snapshot, error)
	cache map[string]*domain.Snapshot
	reads int
}

// ctxCheckEvery bounds how many store reads happen between context checks.
const ctxCheckEvery = 256

// snap returns the snapshot with the given id (an error wrapping
// domain.ErrNotFound if it does not exist).
func (l *lookup) snap(id string) (*domain.Snapshot, error) {
	if s, ok := l.cache[id]; ok {
		return s, nil
	}
	l.reads++
	if l.reads%ctxCheckEvery == 0 {
		if err := l.ctx.Err(); err != nil {
			return nil, err
		}
	}
	s, err := l.get(id)
	if err != nil {
		return nil, err
	}
	l.cache[id] = s
	return s, nil
}

// parent resolves a parent reference of child, reporting dangling
// references as corruption.
func (l *lookup) parent(child *domain.Snapshot, id string) (*domain.Snapshot, error) {
	p, err := l.snap(id)
	if err != nil && errors.Is(err, domain.ErrNotFound) {
		return nil, fmt.Errorf("meta: history: parent %q of snapshot %q is missing: %w", id, child.ID, domain.ErrNotFound)
	}
	return p, err
}

// with runs fn with a lookup bound to a single consistent view of the
// store when possible.
func (h *history) with(ctx context.Context, fn func(l *lookup) error) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	newLookup := func(get func(string) (*domain.Snapshot, error)) *lookup {
		return &lookup{ctx: ctx, get: get, cache: map[string]*domain.Snapshot{}}
	}
	if v, ok := h.store.(SnapshotViewer); ok {
		return v.ViewSnapshots(ctx, func(get func(string) (*domain.Snapshot, error)) error {
			return fn(newLookup(get))
		})
	}
	return fn(newLookup(func(id string) (*domain.Snapshot, error) { return h.store.GetSnapshot(ctx, id) }))
}

// IsAncestor implements domain.History with a breadth-first search from
// descendant that stops as soon as ancestor is found.
func (h *history) IsAncestor(ctx context.Context, ancestor, descendant string) (bool, error) {
	var found bool
	err := h.with(ctx, func(l *lookup) error {
		if descendant == "" {
			found = ancestor == ""
			return nil
		}
		d, err := l.snap(descendant)
		if err != nil {
			return err
		}
		if ancestor == "" || ancestor == descendant {
			found = true
			return nil
		}
		if _, err := l.snap(ancestor); err != nil {
			if errors.Is(err, domain.ErrNotFound) {
				return nil // unknown snapshots are nobody's ancestor
			}
			return err
		}
		seen := map[string]struct{}{d.ID: {}}
		queue := []*domain.Snapshot{d}
		for len(queue) > 0 {
			s := queue[0]
			queue = queue[1:]
			for _, pid := range s.Parents {
				if pid == ancestor {
					found = true
					return nil
				}
				if _, ok := seen[pid]; ok {
					continue
				}
				seen[pid] = struct{}{}
				p, err := l.parent(s, pid)
				if err != nil {
					return err
				}
				queue = append(queue, p)
			}
		}
		return nil
	})
	if err != nil {
		return false, err
	}
	return found, nil
}

// Log implements domain.History. It performs a best-first walk from tip
// using a max-heap keyed by (created_at, id), so a page of limit entries
// costs O(limit · parents · log) reads rather than a walk of the whole
// history. The result equals "all reachable snapshots sorted by
// (created_at, id) descending" whenever no snapshot is older than one of
// its parents (the normal case). With skewed clocks a snapshot can only be
// listed after the child through which it was reached, so the order is a
// best-effort date order (the same trade-off as git log's default order).
func (h *history) Log(ctx context.Context, tip string, limit int) ([]domain.Snapshot, error) {
	out := []domain.Snapshot{}
	err := h.with(ctx, func(l *lookup) error {
		if tip == "" {
			return nil
		}
		t, err := l.snap(tip)
		if err != nil {
			return err
		}
		pq := &snapHeap{newestFirst: true}
		heap.Push(pq, t)
		seen := map[string]struct{}{t.ID: {}}
		for pq.Len() > 0 {
			s := heap.Pop(pq).(*domain.Snapshot)
			out = append(out, *s)
			if limit > 0 && len(out) == limit {
				break
			}
			for _, pid := range s.Parents {
				if _, ok := seen[pid]; ok {
					continue
				}
				seen[pid] = struct{}{}
				p, err := l.parent(s, pid)
				if err != nil {
					return err
				}
				heap.Push(pq, p)
			}
		}
		return nil
	})
	if err != nil {
		return nil, err
	}
	return out, nil
}

// Missing implements domain.History.
//
// It computes H = ancestors of haves (inclusive), then M = snapshots
// reachable from tip without entering H, and finally orders M with Kahn's
// algorithm, breaking ties by (created_at, id) ascending so the output is
// deterministic, parents always precede children, and unrelated snapshots
// appear oldest first. Cost is O((|H| + |M|) log |M|).
func (h *history) Missing(ctx context.Context, tip string, haves []string) ([]domain.Snapshot, error) {
	out := []domain.Snapshot{}
	err := h.with(ctx, func(l *lookup) error {
		if tip == "" {
			return nil
		}
		t, err := l.snap(tip)
		if err != nil {
			return err
		}

		// H: everything the receiver already has.
		have := map[string]struct{}{}
		var queue []*domain.Snapshot
		for _, id := range haves {
			if id == "" {
				continue
			}
			if _, ok := have[id]; ok {
				continue
			}
			s, err := l.snap(id)
			if err != nil {
				if errors.Is(err, domain.ErrNotFound) {
					continue // the receiver has snapshots we have never seen
				}
				return err
			}
			have[id] = struct{}{}
			queue = append(queue, s)
		}
		for len(queue) > 0 {
			s := queue[len(queue)-1]
			queue = queue[:len(queue)-1]
			for _, pid := range s.Parents {
				if _, ok := have[pid]; ok {
					continue
				}
				have[pid] = struct{}{}
				p, err := l.parent(s, pid)
				if err != nil {
					return err
				}
				queue = append(queue, p)
			}
		}
		if _, ok := have[t.ID]; ok {
			return nil
		}

		// M: reachable from tip, minus H. Ancestors of a node in H are in
		// H, so H nodes are never expanded.
		missing := map[string]*domain.Snapshot{t.ID: t}
		queue = append(queue[:0], t)
		for len(queue) > 0 {
			s := queue[len(queue)-1]
			queue = queue[:len(queue)-1]
			for _, pid := range s.Parents {
				if _, ok := have[pid]; ok {
					continue
				}
				if _, ok := missing[pid]; ok {
					continue
				}
				p, err := l.parent(s, pid)
				if err != nil {
					return err
				}
				missing[pid] = p
				queue = append(queue, p)
			}
		}

		// Kahn's algorithm over the sub-DAG induced by M.
		indegree := make(map[string]int, len(missing))
		children := make(map[string][]*domain.Snapshot, len(missing))
		for id, s := range missing {
			for _, pid := range uniqueParents(s.Parents) {
				if _, ok := missing[pid]; ok {
					indegree[id]++
					children[pid] = append(children[pid], s)
				}
			}
		}
		pq := &snapHeap{}
		for id, s := range missing {
			if indegree[id] == 0 {
				pq.items = append(pq.items, s)
			}
		}
		heap.Init(pq)
		out = make([]domain.Snapshot, 0, len(missing))
		for pq.Len() > 0 {
			s := heap.Pop(pq).(*domain.Snapshot)
			out = append(out, *s)
			for _, c := range children[s.ID] {
				indegree[c.ID]--
				if indegree[c.ID] == 0 {
					heap.Push(pq, c)
				}
			}
		}
		if len(out) != len(missing) {
			return fmt.Errorf("meta: history: snapshot graph below %q contains a cycle: %w", tip, domain.ErrConflict)
		}
		return nil
	})
	if err != nil {
		return nil, err
	}
	return out, nil
}

// uniqueParents returns parents without duplicates (allocating only when
// duplicates are present, which is never the case for well-formed input).
func uniqueParents(parents []string) []string {
	if len(parents) < 2 {
		return parents
	}
	if len(parents) == 2 {
		if parents[0] == parents[1] {
			return parents[:1]
		}
		return parents
	}
	seen := make(map[string]struct{}, len(parents))
	out := make([]string, 0, len(parents))
	for _, p := range parents {
		if _, ok := seen[p]; !ok {
			seen[p] = struct{}{}
			out = append(out, p)
		}
	}
	return out
}

// snapHeap is a heap of snapshots ordered by (created_at, id): ascending
// by default, descending when newestFirst is set.
type snapHeap struct {
	items       []*domain.Snapshot
	newestFirst bool
}

func (h *snapHeap) Len() int { return len(h.items) }

func (h *snapHeap) Less(i, j int) bool {
	a, b := h.items[i], h.items[j]
	c := a.CreatedAt.Compare(b.CreatedAt)
	if c == 0 {
		c = strings.Compare(a.ID, b.ID)
	}
	if h.newestFirst {
		return c > 0
	}
	return c < 0
}

func (h *snapHeap) Swap(i, j int) { h.items[i], h.items[j] = h.items[j], h.items[i] }

func (h *snapHeap) Push(x any) { h.items = append(h.items, x.(*domain.Snapshot)) }

func (h *snapHeap) Pop() any {
	n := len(h.items)
	x := h.items[n-1]
	h.items[n-1] = nil
	h.items = h.items[:n-1]
	return x
}
