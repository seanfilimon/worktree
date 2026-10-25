package audit

import (
	"context"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// v0.5 operational actions (§13.3).
const (
	ActionServerStart = "server.start"
	ActionServerStop  = "server.stop"
	ActionGCRun       = "gc.run"
	ActionLimitsPut   = "tenant.limits"
)

// ExportPageSize is the page size Each reads the store with.
const ExportPageSize = 500

// Each calls fn for every event of store matching f, newest first, with no
// overall limit (f.Limit is ignored). It pages through the store by time:
// each page ends at the oldest instant it reached, and every event of that
// instant is then read in full so that events sharing a timestamp are
// neither skipped nor repeated. An error from fn stops the iteration and is
// returned.
func Each(ctx context.Context, store Lister, f domain.AuditFilter, fn func(domain.AuditEvent) error) error {
	return each(ctx, store, f, ExportPageSize, fn)
}

// Lister lists audit events newest first without capping the limit;
// domain.MetaStore and *Log implement it.
type Lister interface {
	ListAudit(ctx context.Context, f domain.AuditFilter) ([]domain.AuditEvent, error)
}

// ListAudit lists events straight from the store (no limit cap), for
// paged exports with Each.
func (l *Log) ListAudit(ctx context.Context, f domain.AuditFilter) ([]domain.AuditEvent, error) {
	if l.store == nil {
		return nil, ErrNoStore
	}
	return l.store.ListAudit(ctx, f)
}

func each(ctx context.Context, store Lister, f domain.AuditFilter, page int, fn func(domain.AuditEvent) error) error {
	cursor := f.Until
	for {
		if err := ctx.Err(); err != nil {
			return err
		}
		pf := f
		pf.Until, pf.Limit = cursor, page
		evs, err := store.ListAudit(ctx, pf)
		if err != nil {
			return err
		}
		if len(evs) < page {
			for _, e := range evs {
				if err := fn(e); err != nil {
					return err
				}
			}
			return nil
		}
		oldest := evs[len(evs)-1].At
		for _, e := range evs {
			if !e.At.After(oldest) {
				break
			}
			if err := fn(e); err != nil {
				return err
			}
		}
		// Every event at exactly `oldest` (there may be more than fit the
		// page).
		tf := f
		tf.Since, tf.Until, tf.Limit = oldest, oldest.Add(time.Nanosecond), 1<<30
		same, err := store.ListAudit(ctx, tf)
		if err != nil {
			return err
		}
		for _, e := range same {
			if err := fn(e); err != nil {
				return err
			}
		}
		cursor = oldest
	}
}
