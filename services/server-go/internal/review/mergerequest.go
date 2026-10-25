package review

import (
	"context"
	"errors"
	"fmt"
	"strconv"
	"strings"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// Open creates a merge request from source into target. It requires
// domain.ActMRCreate, a non-empty title, two distinct existing branches
// that both have snapshots, and no other open merge request for the same
// source and target (domain.ErrAlreadyExists).
func (s *Service) Open(ctx context.Context, p *domain.Principal, tree *domain.Tree, title, description, source, target string) (*domain.MergeRequest, error) {
	if err := checkTree(tree); err != nil {
		return nil, err
	}
	if err := s.authz.Authorize(ctx, p, domain.ActMRCreate, tree.TenantID, tree); err != nil {
		return nil, err
	}
	if err := requirePrincipal(p); err != nil {
		return nil, err
	}
	title = strings.TrimSpace(title)
	source = strings.TrimSpace(source)
	target = strings.TrimSpace(target)
	switch {
	case title == "":
		return nil, fmt.Errorf("merge request title must not be empty: %w", domain.ErrInvalid)
	case source == "" || target == "":
		return nil, fmt.Errorf("merge request source and target branches are required: %w", domain.ErrInvalid)
	case source == target:
		return nil, fmt.Errorf("merge request source and target must differ (both '%s'): %w", source, domain.ErrInvalid)
	}
	for _, name := range []string{source, target} {
		if _, err := s.branchTip(ctx, tree.ID, name); err != nil {
			return nil, err
		}
	}

	unlock := s.lockTree(tree.ID)
	defer unlock()

	open, err := s.store.ListMergeRequests(ctx, tree.ID, domain.MRStateOpen)
	if err != nil {
		return nil, fmt.Errorf("review: list merge requests: %w", err)
	}
	for i := range open {
		if open[i].SourceBranch == source && open[i].TargetBranch == target {
			return nil, fmt.Errorf("merge request !%d from '%s' into '%s' is already open: %w",
				open[i].Number, source, target, domain.ErrAlreadyExists)
		}
	}

	now := s.now()
	mr := &domain.MergeRequest{
		TreeID:       tree.ID,
		Title:        title,
		Description:  description,
		SourceBranch: source,
		TargetBranch: target,
		AuthorID:     p.AccountID,
		Author:       p.Username,
		State:        domain.MRStateOpen,
		Approvals:    []domain.Approval{},
		CreatedAt:    now,
		UpdatedAt:    now,
	}
	if err := s.store.CreateMergeRequest(ctx, mr); err != nil {
		return nil, fmt.Errorf("review: create merge request: %w", err)
	}
	s.publish(domain.EventMROpened, p, tree, snapshotMR(mr))
	s.record(ctx, "mr.open", p, tree, mr, map[string]string{"title": mr.Title})
	return mr, nil
}

// Approve records the caller's approval of the current source tip: it is
// Review with decision domain.ReviewApproved and no body. It requires
// domain.ActMRApprove and an open merge request; authors cannot approve
// their own merge requests (domain.ErrForbidden). Approving the same tip
// twice is a no-op.
func (s *Service) Approve(ctx context.Context, p *domain.Principal, tree *domain.Tree, number int) (*domain.MergeRequest, error) {
	return s.Review(ctx, p, tree, number, domain.ReviewApproved, "")
}

// Close abandons an open merge request. The author may close their own
// merge request (as long as they can still read the tree); anyone else
// needs domain.ActMRMerge.
func (s *Service) Close(ctx context.Context, p *domain.Principal, tree *domain.Tree, number int) (*domain.MergeRequest, error) {
	if err := checkTree(tree); err != nil {
		return nil, err
	}
	// Gate visibility first so callers who cannot see the tree learn
	// nothing about its merge requests.
	if err := s.authz.Authorize(ctx, p, domain.ActTreeRead, tree.TenantID, tree); err != nil {
		return nil, err
	}
	if err := requirePrincipal(p); err != nil {
		return nil, err
	}

	unlock := s.lockTree(tree.ID)
	defer unlock()

	mr, err := s.getMR(ctx, tree.ID, number)
	if err != nil {
		return nil, err
	}
	if mr.AuthorID != p.AccountID {
		if err := s.authz.Authorize(ctx, p, domain.ActMRMerge, tree.TenantID, tree); err != nil {
			return nil, err
		}
	}
	if err := requireOpen(mr); err != nil {
		return nil, err
	}
	mr.State = domain.MRStateClosed
	mr.UpdatedAt = s.now()
	if err := s.store.UpdateMergeRequest(ctx, mr); err != nil {
		return nil, fmt.Errorf("review: update merge request !%d: %w", number, err)
	}
	s.publish(domain.EventMRClosed, p, tree, snapshotMR(mr))
	s.record(ctx, "mr.close", p, tree, mr, nil)
	return mr, nil
}

// Merge fast-forwards the target branch to the source branch's tip.
//
// It requires domain.ActMRMerge, an open merge request and the number of
// current approvals (for the source tip) demanded by the target's
// effective protection (otherwise *domain.PushDenied{Kind:
// "review_required"}). Outstanding change requests, required CI checks,
// open blocking TODOs and linear history fail with a *BlockedError (code
// "merge_blocked"); a member of an open linked group fails with code
// "linked_group" (merge the group instead). The target tip must be an
// ancestor of the source tip (or the target empty); otherwise the result
// wraps domain.ErrConflict and the author must merge the target into the
// source locally and push. A target that moves concurrently also yields
// domain.ErrConflict. When the target already contains the source tip (the
// same snapshot or a descendant of it) the merge request is simply marked
// merged without moving the branch.
func (s *Service) Merge(ctx context.Context, p *domain.Principal, tree *domain.Tree, number int) (*domain.MergeRequest, error) {
	if err := checkTree(tree); err != nil {
		return nil, err
	}
	if err := s.authz.Authorize(ctx, p, domain.ActMRMerge, tree.TenantID, tree); err != nil {
		return nil, err
	}
	if err := requirePrincipal(p); err != nil {
		return nil, err
	}

	unlock := s.lockTree(tree.ID)
	defer unlock()

	mr, err := s.openMR(ctx, tree.ID, number)
	if err != nil {
		return nil, err
	}
	ev, err := s.evaluate(ctx, tree, mr, false)
	if err != nil {
		return nil, err
	}
	if err := ev.mergeError(); err != nil {
		return nil, err
	}
	moved, err := s.moveTarget(ctx, tree, mr, ev)
	if err != nil {
		return nil, err
	}
	if err := s.markMerged(ctx, mr, ev.sourceTip); err != nil {
		return nil, err
	}
	s.announceMerge(ctx, p, tree, mr, ev, moved, nil)
	return mr, nil
}

// moveTarget fast-forwards the target branch of mr to the evaluated source
// tip (unless the target is up to date) and reports whether it moved.
func (s *Service) moveTarget(ctx context.Context, tree *domain.Tree, mr *domain.MergeRequest, ev *evaluation) (bool, error) {
	if ev.upToDate {
		return false, nil
	}
	if err := s.store.CompareAndSwapBranchTip(ctx, tree.ID, mr.TargetBranch, ev.targetTip, ev.sourceTip); err != nil {
		if errors.Is(err, domain.ErrConflict) {
			return false, fmt.Errorf("merge request !%d: target branch '%s' moved during the merge; retry: %w",
				mr.Number, mr.TargetBranch, err)
		}
		return false, fmt.Errorf("review: update branch '%s': %w", mr.TargetBranch, err)
	}
	return true, nil
}

// markMerged stores mr as merged at sourceTip.
func (s *Service) markMerged(ctx context.Context, mr *domain.MergeRequest, sourceTip string) error {
	now := s.now()
	mr.State = domain.MRStateMerged
	mr.MergedAt = &now
	mr.MergedSnapshot = sourceTip
	mr.UpdatedAt = now
	if err := s.store.UpdateMergeRequest(ctx, mr); err != nil {
		// The branch has already moved; surface the failure so the caller
		// can retry (a retry finds source == target and completes).
		return fmt.Errorf("review: branch '%s' updated but merge request !%d could not be saved: %w",
			mr.TargetBranch, mr.Number, err)
	}
	return nil
}

// announceMerge publishes the events and audit record of a completed merge.
func (s *Service) announceMerge(ctx context.Context, p *domain.Principal, tree *domain.Tree, mr *domain.MergeRequest, ev *evaluation, moved bool, extra map[string]string) {
	s.publish(domain.EventMRMerged, p, tree, snapshotMR(mr))
	if moved {
		s.publish(domain.EventBranchPushed, p, tree, map[string]any{
			"branch":        mr.TargetBranch,
			"old_tip":       ev.targetTip,
			"new_tip":       ev.sourceTip,
			"merge_request": mr.Number,
		})
	}
	details := map[string]string{
		"old_tip":    ev.targetTip,
		"new_tip":    ev.sourceTip,
		"approvals":  strconv.Itoa(countApprovals(mr, ev.sourceTip)),
		"up_to_date": strconv.FormatBool(!moved),
	}
	for k, v := range extra {
		details[k] = v
	}
	s.record(ctx, "mr.merge", p, tree, mr, details)
}

// classifyMerge classifies a merge of sourceTip into targetTip. It returns
// upToDate=true when the target already contains the source (same tip, or
// the source tip is an ancestor of the target tip), false when the target
// can be fast-forwarded, and an error wrapping domain.ErrConflict when the
// two have diverged.
func (s *Service) classifyMerge(ctx context.Context, mr *domain.MergeRequest, sourceTip, targetTip string) (upToDate bool, err error) {
	switch {
	case sourceTip == targetTip:
		return true, nil
	case targetTip == "":
		return false, nil
	}
	ff, err := s.hist.IsAncestor(ctx, targetTip, sourceTip)
	if err != nil {
		return false, fmt.Errorf("review: check ancestry of '%s': %w", mr.TargetBranch, err)
	}
	if ff {
		return false, nil
	}
	contained, err := s.hist.IsAncestor(ctx, sourceTip, targetTip)
	if err != nil {
		return false, fmt.Errorf("review: check ancestry of '%s': %w", mr.SourceBranch, err)
	}
	if contained {
		return true, nil
	}
	return false, fmt.Errorf("merge request !%d: target has diverged; merge target into source locally and push: %w",
		mr.Number, domain.ErrConflict)
}

// branchTip returns the tip of an existing, non-empty branch.
func (s *Service) branchTip(ctx context.Context, treeID, name string) (string, error) {
	b, err := s.store.GetBranch(ctx, treeID, name)
	if err != nil {
		if isNotFound(err) {
			return "", fmt.Errorf("branch '%s' not found: %w", name, domain.ErrNotFound)
		}
		return "", fmt.Errorf("review: get branch '%s': %w", name, err)
	}
	if b.Tip == "" {
		return "", fmt.Errorf("branch '%s' has no snapshots: %w", name, domain.ErrInvalid)
	}
	return b.Tip, nil
}

func (s *Service) getMR(ctx context.Context, treeID string, number int) (*domain.MergeRequest, error) {
	mr, err := s.store.GetMergeRequest(ctx, treeID, number)
	if err != nil {
		if isNotFound(err) {
			return nil, fmt.Errorf("merge request !%d not found: %w", number, domain.ErrNotFound)
		}
		return nil, fmt.Errorf("review: get merge request !%d: %w", number, err)
	}
	return mr, nil
}

func (s *Service) openMR(ctx context.Context, treeID string, number int) (*domain.MergeRequest, error) {
	mr, err := s.getMR(ctx, treeID, number)
	if err != nil {
		return nil, err
	}
	if err := requireOpen(mr); err != nil {
		return nil, err
	}
	return mr, nil
}

func requireOpen(mr *domain.MergeRequest) error {
	if mr.State != domain.MRStateOpen {
		return fmt.Errorf("merge request !%d is %s: %w", mr.Number, mr.State, domain.ErrConflict)
	}
	return nil
}

// isApproval reports whether a records an approval (Decision "" is a
// pre-v0.5 approval).
func isApproval(a domain.Approval) bool {
	return a.Decision == "" || a.Decision == domain.ReviewApproved
}

// isCurrent reports whether an approval was given for sourceTip ("" =
// recorded before v0.5, always current).
func isCurrent(a domain.Approval, sourceTip string) bool {
	return a.SnapshotID == "" || a.SnapshotID == sourceTip
}

// countApprovals returns the number of distinct approvers other than the
// author whose approval is for sourceTip.
func countApprovals(mr *domain.MergeRequest, sourceTip string) int {
	current, _ := tallyApprovals(mr, sourceTip)
	return current
}

// tallyApprovals counts distinct non-author approvers whose approval is
// current (for sourceTip) and those whose approval is stale.
func tallyApprovals(mr *domain.MergeRequest, sourceTip string) (current, stale int) {
	cur := make(map[string]struct{}, len(mr.Approvals))
	old := make(map[string]struct{})
	for _, a := range mr.Approvals {
		if a.AccountID == "" || a.AccountID == mr.AuthorID || !isApproval(a) {
			continue
		}
		if isCurrent(a, sourceTip) {
			cur[a.AccountID] = struct{}{}
		} else {
			old[a.AccountID] = struct{}{}
		}
	}
	for id := range cur {
		delete(old, id)
	}
	return len(cur), len(old)
}
