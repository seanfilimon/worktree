package meta

import (
	"context"
	"testing"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// PruneAudit conformance (§13.3 audit retention): both stores delete
// exactly the events strictly before the bound and keep the rest in order.
func TestPruneAuditConformance(t *testing.T) {
	for name, newStore := range map[string]storeFactory{"memory": newMemoryForTest, "bolt": newBoltForTest} {
		t.Run(name, func(t *testing.T) {
			s := newStore(t)
			ctx := context.Background()
			// Out-of-order appends and two events sharing one instant.
			for i, sec := range []int{5, 1, 3, 3, 9, 0} {
				must(t, s.AppendAudit(ctx, &domain.AuditEvent{ID: "e" + string(rune('a'+i)), Action: "x.y", At: at(sec)}))
			}

			n, err := s.PruneAudit(ctx, time.Time{})
			must(t, err)
			wantEqual(t, "zero bound prunes nothing", n, 0)

			n, err = s.PruneAudit(ctx, at(3))
			must(t, err)
			wantEqual(t, "pruned before 3s", n, 2) // 0s and 1s

			left, err := s.ListAudit(ctx, domain.AuditFilter{Limit: 100})
			must(t, err)
			var secs []int
			for _, e := range left {
				secs = append(secs, int(e.At.Sub(base)/time.Second))
			}
			wantEqual(t, "remaining (newest first)", secs, []int{9, 5, 3, 3})

			n, err = s.PruneAudit(ctx, at(3))
			must(t, err)
			wantEqual(t, "idempotent", n, 0)

			n, err = s.PruneAudit(ctx, time.Date(3000, 1, 1, 0, 0, 0, 0, time.UTC))
			must(t, err)
			wantEqual(t, "far-future bound prunes all", n, 4)
			left, err = s.ListAudit(ctx, domain.AuditFilter{})
			must(t, err)
			wantEqual(t, "empty after prune", len(left), 0)

			// Appending after a prune still works and is listed.
			must(t, s.AppendAudit(ctx, &domain.AuditEvent{Action: "a.b", At: at(20)}))
			left, err = s.ListAudit(ctx, domain.AuditFilter{})
			must(t, err)
			wantEqual(t, "append after prune", len(left), 1)

			cctx, cancel := context.WithCancel(ctx)
			cancel()
			if _, err := s.PruneAudit(cctx, at(100)); err == nil {
				t.Fatal("PruneAudit with a canceled context succeeded")
			}
		})
	}
}
