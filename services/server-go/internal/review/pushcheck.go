package review

import (
	"context"
	"fmt"
	"log/slog"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
)

// CheckPushPlan implements domain.PushChecker: a direct push to a branch
// whose effective protection has RequireLinear may not add merge snapshots
// (more than one parent); it is rejected with *domain.PushDenied{Kind:
// "branch_protection"}.
func (s *Service) CheckPushPlan(ctx context.Context, _ *domain.Principal, tree *domain.Tree, plan *domain.PushPlan) error {
	if err := checkTree(tree); err != nil {
		return err
	}
	if plan == nil {
		return nil
	}
	eff, err := s.ProtectionFor(ctx, tree.ID, plan.Branch)
	if err != nil {
		return err
	}
	if !eff.RequireLinear {
		return nil
	}
	for _, snap := range plan.NewSnapshots {
		if snap != nil && len(snap.Parents) > 1 {
			return &domain.PushDenied{
				Kind: "branch_protection",
				Reason: fmt.Sprintf("branch '%s' requires linear history: snapshot %s is a merge snapshot; rebase instead",
					plan.Branch, shortID(snap.ID)),
			}
		}
	}
	return nil
}

// Start subscribes to tree.deleted and removes the deleted tree's merge
// request comments and tag protections until ctx ends. It is a no-op
// without a bus.
func (s *Service) Start(ctx context.Context) {
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
				s.DeleteTreeDocs(context.WithoutCancel(ctx), e.TreeID)
			}
		}
	}()
}

// DeleteTreeDocs removes the review documents of a deleted tree.
func (s *Service) DeleteTreeDocs(ctx context.Context, treeID string) {
	if treeID == "" {
		return
	}
	for _, coll := range []string{CommentCollection, TagProtectionCollection} {
		if _, err := s.store.DeleteDocs(ctx, coll, meta.DocPrefix(treeID)); err != nil {
			s.logger().Warn("review: delete documents of deleted tree",
				slog.String("collection", coll), slog.String("tree_id", treeID), slog.Any("error", err))
		}
	}
}
