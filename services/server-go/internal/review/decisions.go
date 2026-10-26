package review

import (
	"context"
	"fmt"
	"strconv"
	"strings"
	"unicode/utf8"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// MaxBodyLen bounds review and comment bodies.
const MaxBodyLen = 64 << 10

// Review records a review decision on an open merge request.
//
//   - domain.ReviewApproved and domain.ReviewChangesRequested require
//     domain.ActMRApprove; authors cannot review their own merge request
//     this way (domain.ErrForbidden). The decision replaces the reviewer's
//     previous one and records the current source tip (SnapshotID), so an
//     approval goes stale when the source moves and a change request blocks
//     merging until the same reviewer approves. Approving the tip already
//     approved (with the same or no body) is a no-op.
//   - domain.ReviewCommented needs only domain.ActTreeRead and a non-empty
//     body; it adds a merge request comment and leaves approvals alone.
func (s *Service) Review(ctx context.Context, p *domain.Principal, tree *domain.Tree, number int, decision, body string) (*domain.MergeRequest, error) {
	if err := checkTree(tree); err != nil {
		return nil, err
	}
	act := domain.ActMRApprove
	switch decision {
	case domain.ReviewApproved, domain.ReviewChangesRequested:
	case domain.ReviewCommented:
		act = domain.ActTreeRead
	default:
		return nil, fmt.Errorf("decision must be one of approved, changes_requested, commented: %w", domain.ErrInvalid)
	}
	if err := s.authz.Authorize(ctx, p, act, tree.TenantID, tree); err != nil {
		return nil, err
	}
	if err := requirePrincipal(p); err != nil {
		return nil, err
	}
	if len(body) > MaxBodyLen || !utf8.ValidString(body) {
		return nil, fmt.Errorf("body must be at most %d bytes of UTF-8: %w", MaxBodyLen, domain.ErrInvalid)
	}
	if decision == domain.ReviewCommented && strings.TrimSpace(body) == "" {
		return nil, fmt.Errorf("a comment review needs a body: %w", domain.ErrInvalid)
	}

	unlock := s.lockTree(tree.ID)
	defer unlock()

	mr, err := s.openMR(ctx, tree.ID, number)
	if err != nil {
		return nil, err
	}
	if decision == domain.ReviewCommented {
		return s.commentReview(ctx, p, tree, mr, body)
	}
	if mr.AuthorID == p.AccountID {
		verb := "approve"
		if decision == domain.ReviewChangesRequested {
			verb = "request changes on"
		}
		return nil, fmt.Errorf("merge request !%d: authors cannot %s their own merge request: %w",
			number, verb, domain.ErrForbidden)
	}
	tip, err := s.branchTip(ctx, tree.ID, mr.SourceBranch)
	if err != nil {
		return nil, fmt.Errorf("merge request !%d: %w", number, err)
	}

	idx := -1
	for i, a := range mr.Approvals {
		if a.AccountID == p.AccountID {
			idx = i
			break
		}
	}
	if idx >= 0 && decision == domain.ReviewApproved {
		a := mr.Approvals[idx]
		if isApproval(a) && isCurrent(a, tip) && (body == "" || body == a.Body) {
			return mr, nil // idempotent
		}
	}
	now := s.now()
	rec := domain.Approval{
		AccountID:  p.AccountID,
		Username:   p.Username,
		At:         now,
		SnapshotID: tip,
		Decision:   decision,
		Body:       body,
	}
	if idx >= 0 {
		// Drop every earlier record of the reviewer (older stores may hold
		// duplicates) and append the new one.
		kept := mr.Approvals[:0]
		for _, a := range mr.Approvals {
			if a.AccountID != p.AccountID {
				kept = append(kept, a)
			}
		}
		mr.Approvals = kept
	}
	mr.Approvals = append(mr.Approvals, rec)
	mr.UpdatedAt = now
	if err := s.store.UpdateMergeRequest(ctx, mr); err != nil {
		return nil, fmt.Errorf("review: update merge request !%d: %w", number, err)
	}
	if decision == domain.ReviewApproved {
		s.publish(domain.EventMRApproved, p, tree, snapshotMR(mr))
		s.record(ctx, "mr.approve", p, tree, mr, map[string]string{
			"approvals": strconv.Itoa(countApprovals(mr, tip)),
			"snapshot":  tip,
		})
	} else {
		s.publish(domain.EventMRReviewed, p, tree, map[string]any{
			"merge_request": snapshotMR(mr), "decision": decision, "reviewer": p.Username,
		})
		s.record(ctx, "mr.review", p, tree, mr, map[string]string{"decision": decision, "snapshot": tip})
	}
	return mr, nil
}

// commentReview stores a "commented" review as a merge request comment.
// The caller holds the tree lock.
func (s *Service) commentReview(ctx context.Context, p *domain.Principal, tree *domain.Tree, mr *domain.MergeRequest, body string) (*domain.MergeRequest, error) {
	tip := ""
	if b, err := s.store.GetBranch(ctx, tree.ID, mr.SourceBranch); err == nil {
		tip = b.Tip
	}
	c, err := s.storeComment(ctx, p, tree, mr, CommentInput{Body: body, SnapshotID: tip}, nil)
	if err != nil {
		return nil, err
	}
	s.publish(domain.EventMRReviewed, p, tree, map[string]any{
		"merge_request": snapshotMR(mr), "decision": domain.ReviewCommented, "reviewer": p.Username, "comment_id": c.ID,
	})
	s.record(ctx, "mr.review", p, tree, mr, map[string]string{"decision": domain.ReviewCommented, "comment_id": c.ID})
	return mr, nil
}
