package webhooks

// server-ops (docs/ENGINEERING_PLAN.md §16.11): webhook delivery
// retention with a count, for the background retention service. The
// limits are Options.Keep and Options.Retention (--webhook-delivery-keep,
// --webhook-delivery-retention); new deliveries keep pruning their own
// hook as before.

import (
	"context"
	"errors"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
)

// PruneDeliveries applies the delivery-log retention to every hook and
// returns how many deliveries it deleted.
func (s *Service) PruneDeliveries(ctx context.Context) (int, error) {
	hooks, err := meta.ListDocsT[hookDoc](ctx, s.d.Store, CollHooks, "")
	if err != nil {
		return 0, err
	}
	cutoff := s.now().Add(-s.o.Retention)
	deleted := 0
	var errs []error
	for i := range hooks {
		list, err := s.deliveriesOf(ctx, hooks[i].ID)
		if err != nil {
			errs = append(errs, err)
			continue
		}
		excess := len(list) - s.o.Keep
		for j := range list { // oldest first
			if j >= excess && !list[j].CreatedAt.Before(cutoff) {
				continue
			}
			err := s.d.Store.DeleteDoc(ctx, CollDeliveries, DeliveryKey(&list[j]))
			switch {
			case err == nil:
				deleted++
			case errors.Is(err, domain.ErrNotFound):
			default:
				errs = append(errs, err)
				continue
			}
			s.mu.Lock()
			delete(s.pending, list[j].ID)
			s.mu.Unlock()
		}
	}
	return deleted, errors.Join(errs...)
}
