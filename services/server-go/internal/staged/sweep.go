package staged

// v0.8 (docs/ENGINEERING_PLAN.md §16.8, workstream staged-v2): TTL expiry
// ticker, history pruning and tree-deletion cleanup.

import (
	"context"
	"fmt"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// SweepReport is the result of one Sweep.
type SweepReport struct {
	Expired int `json:"expired"`
	Pruned  int `json:"pruned"`
}

// Sweep expires every live entry older than MaxAge (status "expired",
// event staged.cleared) and prunes history older than HistoryRetention.
func (s *Service) Sweep(ctx context.Context) (SweepReport, error) {
	var rep SweepReport
	if s.MaxAge > 0 {
		trees, err := s.store.ListTrees(ctx, "")
		if err != nil {
			return rep, fmt.Errorf("staged: sweep: %w", err)
		}
		for i := range trees {
			t := &trees[i]
			all, err := s.store.ListStaged(ctx, t.ID, "")
			if err != nil {
				return rep, fmt.Errorf("staged: sweep %s: %w", t.ID, err)
			}
			for _, e := range all {
				if s.isExpired(e) && s.expire(ctx, t, e) {
					rep.Expired++
				}
			}
		}
	}
	pruned, err := s.pruneHistory(ctx)
	rep.Pruned = pruned
	if err != nil {
		return rep, fmt.Errorf("staged: prune history: %w", err)
	}
	return rep, nil
}

// Start runs Sweep every SweepInterval until ctx ends and, when the
// service has an event bus, deletes a tree's history when the tree is
// deleted (event tree.deleted). It returns immediately.
func (s *Service) Start(ctx context.Context) {
	interval := s.SweepInterval
	if interval <= 0 {
		interval = DefaultSweepInterval
	}
	go func() {
		t := time.NewTicker(interval)
		defer t.Stop()
		for {
			select {
			case <-ctx.Done():
				return
			case <-t.C:
				rep, err := s.Sweep(context.WithoutCancel(ctx))
				if err != nil {
					s.logger().WarnContext(ctx, "staged: sweep failed", "err", err)
				} else if rep.Expired+rep.Pruned > 0 {
					s.logger().InfoContext(ctx, "staged: sweep", "expired", rep.Expired, "pruned", rep.Pruned)
				}
			}
		}
	}()
	if s.bus == nil {
		return
	}
	ch, cancel := s.bus.Subscribe(domain.EventFilter{Types: []string{domain.EventTreeDeleted}})
	go func() {
		defer cancel()
		for {
			select {
			case <-ctx.Done():
				return
			case e, ok := <-ch:
				if !ok {
					return
				}
				s.HandleTreeDeleted(context.WithoutCancel(ctx), e.TreeID)
			}
		}
	}()
}

// HandleTreeDeleted removes the staged history of a deleted tree (live
// entries go with the tree's metadata bucket).
func (s *Service) HandleTreeDeleted(ctx context.Context, treeID string) {
	if treeID == "" {
		return
	}
	if _, err := s.store.DeleteDocs(ctx, CollHistory, domain.DocKey(treeID)+"\x00"); err != nil {
		s.logger().WarnContext(ctx, "staged: deleting history failed", "tree", treeID, "err", err)
	}
}
