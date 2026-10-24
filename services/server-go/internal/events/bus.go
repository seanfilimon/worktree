// Package events implements domain.EventBus: an in-process, non-blocking
// publish/subscribe bus used to fan server activity (pushes, staged
// snapshots, merge requests, ...) out to SSE clients and other in-process
// consumers.
//
// Design:
//
//   - Every subscriber owns a buffered channel. Publish performs a
//     non-blocking send to each matching subscriber; when a subscriber's
//     buffer is full the event is dropped for that subscriber only and the
//     drop is counted (see Stats). Publish therefore never blocks, no matter
//     how slow a consumer is.
//   - Subscribers are selected by domain.EventFilter (TenantID, TreeID,
//     Types; empty fields match anything).
//   - The cancel func returned by Subscribe is idempotent: it unregisters the
//     subscriber and closes its channel exactly once.
//   - An optional ring buffer keeps the most recent events so that newly
//     connected clients can be shown recent activity (see Recent).
//
// All methods are safe for concurrent use.
package events

import (
	"sync"
	"sync/atomic"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

const (
	// DefaultBuffer is the per-subscriber channel capacity used by NewBus.
	DefaultBuffer = 64
	// DefaultHistory is the number of recent events retained by NewBus for
	// Recent.
	DefaultHistory = 200
)

// Options configures a Bus. The zero value selects the defaults.
type Options struct {
	// Buffer is the capacity of each subscriber channel. Values <= 0 select
	// DefaultBuffer.
	Buffer int
	// History is the number of most recent events kept for Recent. 0 selects
	// DefaultHistory; a negative value disables the history ring entirely.
	History int
}

// Stats is a point-in-time snapshot of bus counters.
type Stats struct {
	// Published counts calls to Publish.
	Published uint64 `json:"published"`
	// Delivered counts successful (subscriber, event) deliveries.
	Delivered uint64 `json:"delivered"`
	// Dropped counts (subscriber, event) pairs that matched but were
	// discarded because the subscriber's buffer was full.
	Dropped uint64 `json:"dropped"`
	// Subscribers is the number of currently registered subscribers.
	Subscribers int `json:"subscribers"`
}

// Bus is an in-process pub/sub bus implementing domain.EventBus. Construct it
// with NewBus, NewBusWithBuffer or NewBusWithOptions; the zero value is also
// usable and behaves like NewBus().
type Bus struct {
	buffer  int
	history int // < 0 disables the ring

	// mu guards subs, nextID and closed. Publish holds it in read mode while
	// sending (sends are non-blocking), cancel/Close hold it in write mode
	// while closing channels, so a send can never hit a closed channel.
	mu     sync.RWMutex
	subs   map[uint64]*subscriber
	nextID uint64
	closed bool

	// ringMu guards the history ring, independent of mu so that recording
	// history does not serialize concurrent publishers against each other's
	// fan-out.
	ringMu   sync.Mutex
	ring     []domain.Event // fixed capacity once allocated
	ringNext int            // index of the next slot to write
	ringLen  int            // number of valid entries

	published atomic.Uint64
	delivered atomic.Uint64
	dropped   atomic.Uint64
}

// subscriber is one registered consumer.
type subscriber struct {
	ch     chan domain.Event
	filter matcher
}

// matcher is a pre-processed domain.EventFilter.
type matcher struct {
	tenantID string
	treeID   string
	types    map[string]struct{} // nil = any type
}

func newMatcher(f domain.EventFilter) matcher {
	m := matcher{tenantID: f.TenantID, treeID: f.TreeID}
	for _, t := range f.Types {
		if t == "" {
			continue
		}
		if m.types == nil {
			m.types = make(map[string]struct{}, len(f.Types))
		}
		m.types[t] = struct{}{}
	}
	return m
}

func (m matcher) match(e *domain.Event) bool {
	if m.tenantID != "" && m.tenantID != e.TenantID {
		return false
	}
	if m.treeID != "" && m.treeID != e.TreeID {
		return false
	}
	if m.types != nil {
		if _, ok := m.types[e.Type]; !ok {
			return false
		}
	}
	return true
}

// Compile-time interface check.
var _ domain.EventBus = (*Bus)(nil)

// NewBus returns a bus with DefaultBuffer per-subscriber capacity and a
// DefaultHistory-event history ring.
func NewBus() *Bus { return NewBusWithOptions(Options{}) }

// NewBusWithBuffer returns a bus whose subscriber channels have the given
// capacity (<= 0 selects DefaultBuffer) and a DefaultHistory-event ring.
func NewBusWithBuffer(buffer int) *Bus { return NewBusWithOptions(Options{Buffer: buffer}) }

// NewBusWithOptions returns a bus configured by opts.
func NewBusWithOptions(opts Options) *Bus {
	b := &Bus{buffer: opts.Buffer, history: opts.History}
	if b.buffer <= 0 {
		b.buffer = DefaultBuffer
	}
	if b.history == 0 {
		b.history = DefaultHistory
	}
	return b
}

func (b *Bus) bufferSize() int {
	if b.buffer <= 0 {
		return DefaultBuffer
	}
	return b.buffer
}

func (b *Bus) historySize() int {
	switch {
	case b.history < 0:
		return 0
	case b.history == 0:
		return DefaultHistory
	default:
		return b.history
	}
}

// Publish delivers e to every matching subscriber without blocking. If e.At
// is zero it is set to the current UTC time. Events are dropped (and counted)
// for subscribers whose buffer is full. e.Payload is shared by all
// subscribers and the history ring and must be treated as read-only.
func (b *Bus) Publish(e domain.Event) {
	if e.At.IsZero() {
		e.At = time.Now().UTC()
	}
	b.published.Add(1)
	b.record(e)

	var delivered, dropped uint64
	b.mu.RLock()
	for _, s := range b.subs {
		if !s.filter.match(&e) {
			continue
		}
		select {
		case s.ch <- e:
			delivered++
		default:
			dropped++
		}
	}
	b.mu.RUnlock()

	if delivered > 0 {
		b.delivered.Add(delivered)
	}
	if dropped > 0 {
		b.dropped.Add(dropped)
	}
}

// Subscribe registers a subscriber for events matching f. The returned
// channel is closed when cancel is called (or the bus is closed); cancel is
// idempotent and safe to call from any goroutine. Callers must keep draining
// the channel or call cancel; a full channel only causes drops, never blocks
// publishers. Subscribing to a closed bus returns an already-closed channel.
func (b *Bus) Subscribe(f domain.EventFilter) (<-chan domain.Event, func()) {
	s := &subscriber{
		ch:     make(chan domain.Event, b.bufferSize()),
		filter: newMatcher(f),
	}

	b.mu.Lock()
	if b.closed {
		b.mu.Unlock()
		close(s.ch)
		return s.ch, func() {}
	}
	if b.subs == nil {
		b.subs = make(map[uint64]*subscriber)
	}
	id := b.nextID
	b.nextID++
	b.subs[id] = s
	b.mu.Unlock()

	cancel := func() {
		b.mu.Lock()
		defer b.mu.Unlock()
		// Presence in the map is the "still open" flag: whoever removes the
		// entry (cancel or Close) closes the channel, exactly once.
		if cur, ok := b.subs[id]; ok && cur == s {
			delete(b.subs, id)
			close(s.ch)
		}
	}
	return s.ch, cancel
}

// Close unregisters every subscriber and closes their channels. Subsequent
// Subscribe calls return closed channels; Publish keeps recording history
// but has no one to deliver to. Close is idempotent.
func (b *Bus) Close() {
	b.mu.Lock()
	defer b.mu.Unlock()
	if b.closed {
		return
	}
	b.closed = true
	for id, s := range b.subs {
		delete(b.subs, id)
		close(s.ch)
	}
}

// Stats returns the current counters.
func (b *Bus) Stats() Stats {
	b.mu.RLock()
	n := len(b.subs)
	b.mu.RUnlock()
	return Stats{
		Published:   b.published.Load(),
		Delivered:   b.delivered.Load(),
		Dropped:     b.dropped.Load(),
		Subscribers: n,
	}
}

// record appends e to the history ring (if enabled).
func (b *Bus) record(e domain.Event) {
	size := b.historySize()
	if size == 0 {
		return
	}
	b.ringMu.Lock()
	if b.ring == nil {
		b.ring = make([]domain.Event, size)
	}
	b.ring[b.ringNext] = e
	b.ringNext = (b.ringNext + 1) % len(b.ring)
	if b.ringLen < len(b.ring) {
		b.ringLen++
	}
	b.ringMu.Unlock()
}

// Recent returns up to n of the most recent retained events matching f, in
// chronological (publish) order, oldest first, so callers can replay them
// before streaming live events. n <= 0 returns every retained match. The
// result is a fresh slice (never nil); payloads are shared and read-only.
func (b *Bus) Recent(f domain.EventFilter, n int) []domain.Event {
	m := newMatcher(f)
	b.ringMu.Lock()
	defer b.ringMu.Unlock()

	limit := b.ringLen
	if n > 0 && n < limit {
		limit = n
	}
	out := make([]domain.Event, 0, limit)
	if b.ringLen == 0 || limit == 0 {
		return out
	}
	// Walk newest -> oldest collecting matches, then reverse.
	size := len(b.ring)
	for i := 0; i < b.ringLen && len(out) < limit; i++ {
		idx := (b.ringNext - 1 - i + size) % size
		if m.match(&b.ring[idx]) {
			out = append(out, b.ring[idx])
		}
	}
	for i, j := 0, len(out)-1; i < j; i, j = i+1, j-1 {
		out[i], out[j] = out[j], out[i]
	}
	return out
}
