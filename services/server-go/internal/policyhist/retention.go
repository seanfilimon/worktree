package policyhist

// server-ops (docs/ENGINEERING_PLAN.md §16.11): policy version retention
// (`--policy-version-retention AGE,COUNT`). The latest version of every
// policy is always kept, so a policy never loses its history entirely
// (which would make History synthesize a new version 1) and deleted
// policies stay restorable from their delete version. Version numbers are
// never reused: Record numbers new versions after the latest one.

import (
	"context"
	"errors"
	"fmt"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
)

// Prune deletes, for every policy of every tenant, the versions older
// than maxAge (0 = no age limit) and those beyond the newest keep (0 = no
// count limit), never the latest version. It returns how many versions it
// deleted.
func (r *Recorder) Prune(ctx context.Context, maxAge time.Duration, keep int) (int, error) {
	if maxAge <= 0 && keep <= 0 {
		return 0, nil
	}
	all, err := meta.ListDocsT[domain.PolicyVersion](ctx, r.store, Collection, "")
	if err != nil {
		return 0, fmt.Errorf("policyhist: list versions: %w", err)
	}
	cutoff := r.now().Add(-maxAge)
	deleted := 0
	var errs []error
	// Key order groups versions per (tenant, policy), oldest first.
	for start := 0; start < len(all); {
		end := start + 1
		for end < len(all) && all[end].TenantID == all[start].TenantID && all[end].PolicyID == all[start].PolicyID {
			end++
		}
		group := all[start:end]
		for i := 0; i < len(group)-1; i++ { // the latest is kept
			v := group[i]
			tooMany := keep > 0 && len(group)-i > keep
			tooOld := maxAge > 0 && v.ChangedAt.Before(cutoff)
			if !tooMany && !tooOld {
				continue
			}
			err := r.store.DeleteDoc(ctx, Collection, key(v.TenantID, v.PolicyID, v.Version))
			switch {
			case err == nil:
				deleted++
			case errors.Is(err, domain.ErrNotFound):
			default:
				errs = append(errs, err)
			}
		}
		start = end
	}
	return deleted, errors.Join(errs...)
}
