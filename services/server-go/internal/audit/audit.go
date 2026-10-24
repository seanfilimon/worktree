// Package audit implements domain.AuditLog on top of a domain.MetaStore.
//
// Every security-relevant action on the server (pushes, logins, membership
// and license changes, ...) is recorded as a domain.AuditEvent. Recording
// never fails the caller: storage errors are logged with slog and the
// request proceeds. Before an event is persisted, Log normalizes it:
//
//   - ID is set to a time-sortable UUIDv7 string when empty, so that
//     lexical order of IDs follows recording order;
//   - At is set to the current UTC time when zero (and converted to UTC
//     otherwise);
//   - Actor defaults to "anonymous" when empty;
//   - RemoteAddr defaults to the address attached to the context with
//     WithRemoteAddr, or the gRPC peer address;
//   - Details values whose keys look sensitive (password, token, secret,
//     authorization, api key, ...) are replaced with "[REDACTED]".
//
// By default events are written synchronously. WithAsync enables a bounded
// buffer drained by a background writer; call Close to flush it on
// shutdown.
package audit

import (
	"context"
	"errors"
	"fmt"
	"log/slog"
	"sync"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// Query limits.
const (
	// DefaultQueryLimit is used when AuditFilter.Limit is zero or negative.
	DefaultQueryLimit = 100
	// MaxQueryLimit caps AuditFilter.Limit.
	MaxQueryLimit = 1000
)

// Well-known actor names.
const (
	ActorAnonymous = "anonymous"
	ActorSystem    = "system"
)

// DefaultWriteTimeout bounds a single store append.
const DefaultWriteTimeout = 10 * time.Second

// ErrNoStore is returned by Query when the Log was built without a store.
var ErrNoStore = errors.New("audit: no store configured")

// Option configures a Log.
type Option func(*Log)

// WithAsync makes Record enqueue events on a buffered channel of the given
// capacity, written by a single background goroutine. When the buffer is
// full Record falls back to a synchronous write (back-pressure rather than
// silently dropping audit records). A non-positive buffer keeps the default
// synchronous behaviour. Call Close to drain the buffer.
func WithAsync(buffer int) Option {
	return func(l *Log) {
		if buffer > 0 {
			l.buffer = buffer
		}
	}
}

// WithClock overrides the time source (useful in tests).
func WithClock(now func() time.Time) Option {
	return func(l *Log) {
		if now != nil {
			l.now = now
		}
	}
}

// WithWriteTimeout bounds each store append (default DefaultWriteTimeout).
// A non-positive value disables the timeout.
func WithWriteTimeout(d time.Duration) Option {
	return func(l *Log) { l.writeTimeout = d }
}

// Log records audit events into a domain.MetaStore. It implements
// domain.AuditLog and is safe for concurrent use.
type Log struct {
	store        domain.MetaStore
	log          *slog.Logger
	now          func() time.Time
	ids          *idGenerator
	writeTimeout time.Duration

	// async mode
	buffer    int
	mu        sync.RWMutex // guards closed and sends on queue
	closed    bool
	queue     chan queued
	done      chan struct{}
	closeOnce sync.Once
}

type queued struct {
	ctx context.Context
	ev  domain.AuditEvent
}

var _ domain.AuditLog = (*Log)(nil)

// New returns an audit log writing to store. A nil logger uses
// slog.Default(). Without options the log is synchronous.
func New(store domain.MetaStore, log *slog.Logger, opts ...Option) *Log {
	if log == nil {
		log = slog.Default()
	}
	l := &Log{
		store:        store,
		log:          log,
		now:          time.Now,
		ids:          newIDGenerator(),
		writeTimeout: DefaultWriteTimeout,
	}
	for _, o := range opts {
		o(l)
	}
	if l.buffer > 0 {
		l.queue = make(chan queued, l.buffer)
		l.done = make(chan struct{})
		go l.run()
	}
	return l
}

// Record normalizes e (see the package documentation) and persists it. It
// never returns or panics on storage errors; failures are logged.
func (l *Log) Record(ctx context.Context, e domain.AuditEvent) {
	if ctx == nil {
		ctx = context.Background()
	}
	e = l.normalize(ctx, e)
	// Decouple from request cancellation: an audit record must survive the
	// request that produced it. Context values are preserved.
	ctx = context.WithoutCancel(ctx)

	if l.queue != nil && l.enqueue(ctx, e) {
		return
	}
	l.write(ctx, e)
}

// enqueue tries a non-blocking send to the async writer. It reports false
// when the log is closed or the buffer is full.
func (l *Log) enqueue(ctx context.Context, e domain.AuditEvent) bool {
	l.mu.RLock()
	defer l.mu.RUnlock()
	if l.closed {
		return false
	}
	select {
	case l.queue <- queued{ctx: ctx, ev: e}:
		return true
	default:
		l.log.Warn("audit: async buffer full, writing synchronously",
			slog.Int("buffer", l.buffer))
		return false
	}
}

func (l *Log) run() {
	defer close(l.done)
	for q := range l.queue {
		l.write(q.ctx, q.ev)
	}
}

// write appends a normalized event to the store, logging failures.
func (l *Log) write(ctx context.Context, e domain.AuditEvent) {
	attrs := []any{
		slog.String("id", e.ID),
		slog.String("action", e.Action),
		slog.String("actor", e.Actor),
		slog.String("tenant_id", e.TenantID),
		slog.String("resource_type", e.ResourceType),
		slog.String("resource_id", e.ResourceID),
	}
	if l.store == nil {
		l.log.Error("audit: no store configured; event not persisted", attrs...)
		return
	}
	if l.writeTimeout > 0 {
		var cancel context.CancelFunc
		ctx, cancel = context.WithTimeout(ctx, l.writeTimeout)
		defer cancel()
	}
	err := func() (err error) {
		defer func() {
			if r := recover(); r != nil {
				err = fmt.Errorf("panic in AppendAudit: %v", r)
			}
		}()
		return l.store.AppendAudit(ctx, &e)
	}()
	if err != nil {
		l.log.Error("audit: append failed", append(attrs, slog.Any("error", err))...)
		return
	}
	l.log.Debug("audit: recorded", attrs...)
}

// normalize fills defaults and redacts sensitive details. The caller's
// Details map is never mutated.
func (l *Log) normalize(ctx context.Context, e domain.AuditEvent) domain.AuditEvent {
	if e.ID == "" {
		e.ID = l.ids.next(l.now())
	}
	if e.At.IsZero() {
		e.At = l.now()
	}
	e.At = e.At.UTC()
	if e.Actor == "" {
		e.Actor = ActorAnonymous
	}
	if e.RemoteAddr == "" {
		e.RemoteAddr = RemoteAddr(ctx)
	}
	e.Details = Redact(e.Details)
	return e
}

// Query returns events matching f, newest first. Limit defaults to
// DefaultQueryLimit and is capped at MaxQueryLimit. In async mode, events
// still in the buffer are not yet visible.
func (l *Log) Query(ctx context.Context, f domain.AuditFilter) ([]domain.AuditEvent, error) {
	if l.store == nil {
		return nil, ErrNoStore
	}
	switch {
	case f.Limit <= 0:
		f.Limit = DefaultQueryLimit
	case f.Limit > MaxQueryLimit:
		f.Limit = MaxQueryLimit
	}
	evs, err := l.store.ListAudit(ctx, f)
	if err != nil {
		return nil, fmt.Errorf("audit: query: %w", err)
	}
	if evs == nil {
		evs = []domain.AuditEvent{}
	}
	if len(evs) > f.Limit {
		evs = evs[:f.Limit]
	}
	return evs, nil
}

// Close stops the async writer after draining every buffered event. It is
// idempotent and a no-op for a synchronous log. Records made after Close
// are written synchronously.
func (l *Log) Close() error {
	if l.queue == nil {
		return nil
	}
	l.closeOnce.Do(func() {
		l.mu.Lock()
		l.closed = true
		close(l.queue)
		l.mu.Unlock()
		<-l.done
	})
	return nil
}
