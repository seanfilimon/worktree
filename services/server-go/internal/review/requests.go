package review

// v0.7 deeper review (docs/ENGINEERING_PLAN.md §15.7): draft merge
// requests, review requests and automatic owner review requests.

import (
	"context"
	"errors"
	"fmt"
	"log/slog"
	"slices"
	"strings"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// MaxReviewers bounds MergeRequest.Reviewers.
const MaxReviewers = 100

// OpenInput describes a new merge request (OpenWith).
type OpenInput struct {
	Title       string
	Description string
	Source      string
	Target      string
	// Draft merge requests cannot be merged until marked ready (Ready); the
	// owners of their paths are requested when they become ready.
	Draft bool
	// Reviewers are requested explicitly (any accepted form, see
	// NormalizeOwner); they must exist.
	Reviewers []string
	// v0.8 forks (§16.2, forks.go): SourceTree is the tree the source
	// branch lives in (nil = the merge request's own tree); it must be in
	// the same fork network.
	SourceTree *domain.Tree
}

// OpenWith is Open with the v0.7 options: a non-draft merge request
// requests the owners of its changed paths as reviewers (publishing
// mr.review_requested together with the explicit reviewers).
func (s *Service) OpenWith(ctx context.Context, p *domain.Principal, tree *domain.Tree, in OpenInput) (*domain.MergeRequest, error) {
	return s.open(ctx, p, tree, in)
}

// normalizeReviewers validates refs for mr: canonical, deduplicated,
// existing, not the author.
func (s *Service) normalizeReviewers(ctx context.Context, tree *domain.Tree, author string, refs []string) ([]string, error) {
	if len(refs) > MaxReviewers {
		return nil, fmt.Errorf("at most %d reviewers can be requested: %w", MaxReviewers, domain.ErrInvalid)
	}
	out := []string{}
	for _, r := range refs {
		n, err := NormalizeOwner(r)
		if err != nil {
			return nil, err
		}
		if slices.Contains(out, n) {
			continue
		}
		if strings.EqualFold(n, "user:"+author) {
			return nil, fmt.Errorf("the author cannot be requested as a reviewer of their own merge request: %w", domain.ErrInvalid)
		}
		if err := s.checkPrincipalRef(ctx, tree, n); err != nil {
			return nil, err
		}
		out = append(out, n)
	}
	return out, nil
}

// addReviewers appends the refs mr does not have yet and returns them.
func addReviewers(mr *domain.MergeRequest, refs []string) []string {
	var added []string
	for _, r := range refs {
		if !slices.Contains(mr.Reviewers, r) && len(mr.Reviewers) < MaxReviewers {
			mr.Reviewers = append(mr.Reviewers, r)
			added = append(added, r)
		}
	}
	return added
}

// ownerReviewers returns the owners of the paths mr changes, minus the
// author (best effort: failures are logged and yield nothing).
func (s *Service) ownerReviewers(ctx context.Context, tree *domain.Tree, mr *domain.MergeRequest) []string {
	if s.objects == nil {
		return nil
	}
	source, target := s.tips(ctx, tree, mr)
	own, err := s.ownership(ctx, tree, mr, source, target)
	if err != nil {
		s.logger().Warn("review: compute owners for review requests", slog.String("tree_id", tree.ID),
			slog.Int("number", mr.Number), slog.Any("error", err))
		return nil
	}
	var out []string
	for _, po := range own.Paths {
		for _, o := range po.Owners {
			if strings.EqualFold(o, "user:"+mr.Author) || slices.Contains(out, o) {
				continue
			}
			out = append(out, o)
		}
	}
	return out
}

// publishReviewRequested publishes mr.review_requested with the payload
// {"number","reviewers","requested_by"} (consumed by notifications) and
// records the request.
func (s *Service) publishReviewRequested(ctx context.Context, p *domain.Principal, tree *domain.Tree, mr *domain.MergeRequest, added []string, requestedBy string) {
	if len(added) == 0 {
		return
	}
	if s.bus != nil {
		s.bus.Publish(domain.Event{
			Type:     domain.EventMRReviewRequested,
			TenantID: tree.TenantID,
			TreeID:   tree.ID,
			Actor:    requestedBy,
			Payload: map[string]any{
				"number":       mr.Number,
				"reviewers":    slices.Clone(added),
				"requested_by": requestedBy,
			},
			At: s.now(),
		})
	}
	s.record(ctx, "mr.review_request", p, tree, mr, map[string]string{
		"reviewers": strings.Join(added, ","), "requested_by": requestedBy,
	})
}

// mayManageReview allows the author of mr, or callers with act.
func (s *Service) mayManageReview(ctx context.Context, p *domain.Principal, tree *domain.Tree, mr *domain.MergeRequest, act domain.Action) error {
	if mr.AuthorID == p.AccountID {
		return nil
	}
	return s.authz.Authorize(ctx, p, act, tree.TenantID, tree)
}

// RequestReviewers adds reviewers ("user:x" / "team:y", see NormalizeOwner)
// to an open merge request and publishes mr.review_requested for the new
// ones. The author or a caller with domain.ActMRApprove may request.
func (s *Service) RequestReviewers(ctx context.Context, p *domain.Principal, tree *domain.Tree, number int, refs []string) (*domain.MergeRequest, error) {
	mr, unlock, err := s.reviewTarget(ctx, p, tree, number, domain.ActMRApprove)
	if err != nil {
		return nil, err
	}
	defer unlock()
	if len(refs) == 0 {
		return nil, fmt.Errorf("reviewers must not be empty: %w", domain.ErrInvalid)
	}
	norm, err := s.normalizeReviewers(ctx, tree, mr.Author, refs)
	if err != nil {
		return nil, err
	}
	added := addReviewers(mr, norm)
	if len(added) == 0 {
		return mr, nil
	}
	mr.UpdatedAt = s.now()
	if err := s.store.UpdateMergeRequest(ctx, mr); err != nil {
		return nil, fmt.Errorf("review: update merge request !%d: %w", number, err)
	}
	s.publishReviewRequested(ctx, p, tree, mr, added, p.Username)
	return mr, nil
}

// RemoveReviewers withdraws review requests (unknown references are
// ignored). The author or a caller with domain.ActMRApprove may remove.
func (s *Service) RemoveReviewers(ctx context.Context, p *domain.Principal, tree *domain.Tree, number int, refs []string) (*domain.MergeRequest, error) {
	mr, unlock, err := s.reviewTarget(ctx, p, tree, number, domain.ActMRApprove)
	if err != nil {
		return nil, err
	}
	defer unlock()
	if len(refs) == 0 {
		return nil, fmt.Errorf("reviewers must not be empty: %w", domain.ErrInvalid)
	}
	drop := map[string]bool{}
	for _, r := range refs {
		n, err := NormalizeOwner(r)
		if err != nil {
			return nil, err
		}
		drop[n] = true
	}
	kept := mr.Reviewers[:0:0]
	var removed []string
	for _, r := range mr.Reviewers {
		if drop[r] {
			removed = append(removed, r)
		} else {
			kept = append(kept, r)
		}
	}
	if len(removed) == 0 {
		return mr, nil
	}
	mr.Reviewers = kept
	mr.UpdatedAt = s.now()
	if err := s.store.UpdateMergeRequest(ctx, mr); err != nil {
		return nil, fmt.Errorf("review: update merge request !%d: %w", number, err)
	}
	s.record(ctx, "mr.review_request_remove", p, tree, mr, map[string]string{"reviewers": strings.Join(removed, ",")})
	return mr, nil
}

// Ready marks a draft merge request ready for review (publishing mr.ready)
// and requests the owners of its changed paths. A merge request that is not
// a draft is returned unchanged. The author or a caller with
// domain.ActMRMerge may mark it ready.
func (s *Service) Ready(ctx context.Context, p *domain.Principal, tree *domain.Tree, number int) (*domain.MergeRequest, error) {
	mr, unlock, err := s.reviewTarget(ctx, p, tree, number, domain.ActMRMerge)
	if err != nil {
		return nil, err
	}
	defer unlock()
	if !mr.Draft {
		return mr, nil
	}
	mr.Draft = false
	added := addReviewers(mr, s.ownerReviewers(ctx, tree, mr))
	mr.UpdatedAt = s.now()
	if err := s.store.UpdateMergeRequest(ctx, mr); err != nil {
		return nil, fmt.Errorf("review: update merge request !%d: %w", number, err)
	}
	s.publish(domain.EventMRReady, p, tree, snapshotMR(mr))
	s.record(ctx, "mr.ready", p, tree, mr, nil)
	s.publishReviewRequested(ctx, p, tree, mr, added, p.Username)
	return mr, nil
}

// reviewTarget authorizes tree:read, loads the open merge request under
// the tree lock and checks that p is its author or holds act. The caller
// must call unlock.
func (s *Service) reviewTarget(ctx context.Context, p *domain.Principal, tree *domain.Tree, number int, act domain.Action) (*domain.MergeRequest, func(), error) {
	if err := checkTree(tree); err != nil {
		return nil, nil, err
	}
	if err := s.authz.Authorize(ctx, p, domain.ActTreeRead, tree.TenantID, tree); err != nil {
		return nil, nil, err
	}
	if err := requirePrincipal(p); err != nil {
		return nil, nil, err
	}
	unlock := s.lockTree(tree.ID)
	mr, err := s.getMR(ctx, tree.ID, number)
	if err == nil {
		err = s.mayManageReview(ctx, p, tree, mr, act)
	}
	if err == nil {
		err = requireOpen(mr)
	}
	if err != nil {
		unlock()
		return nil, nil, err
	}
	return mr, unlock, nil
}

// SourcePushed requests the owners of the paths of every open, non-draft
// merge request whose source is branch (called for branch.pushed events).
// actor is the pusher's username.
func (s *Service) SourcePushed(ctx context.Context, treeID, branch, actor string) {
	if treeID == "" || branch == "" || s.objects == nil {
		return
	}
	tree, err := s.store.GetTree(ctx, treeID)
	if err != nil {
		if !errors.Is(err, domain.ErrNotFound) {
			s.logger().Warn("review: owner requests after push", slog.String("tree_id", treeID), slog.Any("error", err))
		}
		return
	}
	unlock := s.lockTree(tree.ID)
	defer unlock()
	open, err := s.store.ListMergeRequests(ctx, tree.ID, domain.MRStateOpen)
	if err != nil {
		s.logger().Warn("review: list merge requests", slog.String("tree_id", treeID), slog.Any("error", err))
		return
	}
	var p *domain.Principal
	if actor != "" {
		p = &domain.Principal{Username: actor}
		if a, err := s.store.GetAccountByUsername(ctx, actor); err == nil {
			p.AccountID = a.ID
		}
	}
	for i := range open {
		mr := &open[i]
		if mr.SourceBranch != branch || mr.Draft || crossTree(tree, mr) { // forks (§16.2): pushes here do not move fork sources
			continue
		}
		added := addReviewers(mr, s.ownerReviewers(ctx, tree, mr))
		if len(added) == 0 {
			continue
		}
		mr.UpdatedAt = s.now()
		if err := s.store.UpdateMergeRequest(ctx, mr); err != nil {
			s.logger().Warn("review: store owner review requests", slog.String("tree_id", treeID),
				slog.Int("number", mr.Number), slog.Any("error", err))
			continue
		}
		s.publishReviewRequested(ctx, p, tree, mr, added, actor)
	}
}

// eventString returns payload[key] of a bus event payload map.
func eventString(payload any, key string) string {
	switch m := payload.(type) {
	case map[string]any:
		v, _ := m[key].(string)
		return v
	case map[string]string:
		return m[key]
	}
	return ""
}
