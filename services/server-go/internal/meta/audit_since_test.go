package meta

import (
	"context"
	"testing"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// A Since bound outside the UnixNano range (years 1677..2262) must not wrap
// around: far-future bounds select nothing, far-past bounds select all.
func TestListAuditSinceOutsideUnixNanoRange(t *testing.T) {
	for name, newStore := range map[string]storeFactory{"memory": newMemoryForTest, "bolt": newBoltForTest} {
		t.Run(name, func(t *testing.T) {
			s := newStore(t)
			ctx := context.Background()
			must(t, s.AppendAudit(ctx, &domain.AuditEvent{Action: "tree.create", At: at(0)}))

			future, err := s.ListAudit(ctx, domain.AuditFilter{Since: time.Date(3000, 1, 1, 0, 0, 0, 0, time.UTC)})
			must(t, err)
			wantEqual(t, "events since year 3000", len(future), 0)

			past, err := s.ListAudit(ctx, domain.AuditFilter{Since: time.Date(1000, 1, 1, 0, 0, 0, 0, time.UTC)})
			must(t, err)
			wantEqual(t, "events since year 1000", len(past), 1)
		})
	}
}
