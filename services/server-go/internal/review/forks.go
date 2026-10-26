package review

// v0.8 forks (docs/ENGINEERING_PLAN.md §16.2, specs/tree/Forks.md §5):
// merge requests whose source branch lives in another tree of the same fork
// network (MergeRequest.SourceTreeID). The rest of the package reads the
// source tip through mrSourceTip: the fork's branch tip is copied into the
// target tree (package fork/snapcopy; objects are shared) so ancestry,
// mergeability, three-way merges, approvals and diffs all work on snapshots
// of the target tree. Merging needs the usual upstream permissions.

import (
	"context"
	"errors"
	"fmt"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/fork/snapcopy"
)

// maxForkDepth bounds ForkOf chains walked by forkChain.
const maxForkDepth = 32

// crossTree reports whether mr's source branch lives in another tree.
func crossTree(tree *domain.Tree, mr *domain.MergeRequest) bool {
	return mr.SourceTreeID != "" && tree != nil && mr.SourceTreeID != tree.ID
}

// mrSourceTip returns the tip of mr's source branch as a snapshot of tree:
// the branch tip itself for same-tree merge requests, else the copy of the
// fork's tip in tree.
func (s *Service) mrSourceTip(ctx context.Context, tree *domain.Tree, mr *domain.MergeRequest) (string, error) {
	if !crossTree(tree, mr) {
		return s.branchTip(ctx, tree.ID, mr.SourceBranch)
	}
	if _, err := s.store.GetTree(ctx, mr.SourceTreeID); err != nil {
		if isNotFound(err) {
			return "", fmt.Errorf("the source tree of merge request !%d no longer exists: %w", mr.Number, domain.ErrNotFound)
		}
		return "", fmt.Errorf("review: source tree: %w", err)
	}
	tip, err := s.branchTip(ctx, mr.SourceTreeID, mr.SourceBranch)
	if err != nil {
		return "", err
	}
	out, err := snapcopy.Copy(ctx, s.store, mr.SourceTreeID, tree.ID, tip)
	if err != nil {
		return "", fmt.Errorf("review: copy source of merge request !%d: %w", mr.Number, err)
	}
	return out, nil
}

// currentSourceTip is mrSourceTip with "" for a missing or empty source.
func (s *Service) currentSourceTip(ctx context.Context, tree *domain.Tree, mr *domain.MergeRequest) string {
	tip, err := s.mrSourceTip(ctx, tree, mr)
	if err != nil {
		return ""
	}
	return tip
}

// sourceTreeID returns the tree the source branch of a new merge request
// lives in (in.SourceTree, default tree).
func sourceTreeID(tree *domain.Tree, in OpenInput) string {
	if in.SourceTree != nil && in.SourceTree.ID != "" {
		return in.SourceTree.ID
	}
	return tree.ID
}

// mrSourceTreeID returns the tree mr's source branch lives in.
func mrSourceTreeID(mr *domain.MergeRequest) string {
	if mr.SourceTreeID != "" {
		return mr.SourceTreeID
	}
	return mr.TreeID
}

// authorizeOpen authorizes opening a merge request into tree. Same-tree
// requests need mr:create. A request from another tree of the fork network
// needs mr:create on tree, or tree:fork on tree plus tree:write on the
// source (outside contributors open requests from their forks); the caller
// must read the source tree either way.
func (s *Service) authorizeOpen(ctx context.Context, p *domain.Principal, tree *domain.Tree, in OpenInput) error {
	err := s.authz.Authorize(ctx, p, domain.ActMRCreate, tree.TenantID, tree)
	src := in.SourceTree
	if src == nil || src.ID == "" || src.ID == tree.ID {
		return err
	}
	if rerr := s.authz.Authorize(ctx, p, domain.ActTreeRead, src.TenantID, src); rerr != nil {
		return rerr
	}
	if err == nil {
		return s.checkNetwork(ctx, tree, src)
	}
	if !errors.Is(err, domain.ErrForbidden) {
		return err
	}
	if ferr := s.authz.Authorize(ctx, p, domain.ActTreeFork, tree.TenantID, tree); ferr != nil {
		return err
	}
	if werr := s.authz.Authorize(ctx, p, domain.ActTreeWrite, src.TenantID, src); werr != nil {
		return err
	}
	return s.checkNetwork(ctx, tree, src)
}

// checkNetwork refuses sources outside tree's fork network: one of the two
// must be forked (directly or transitively) from the other, or both from a
// common upstream.
func (s *Service) checkNetwork(ctx context.Context, tree, src *domain.Tree) error {
	a, b := s.forkChain(ctx, tree), s.forkChain(ctx, src)
	for _, id := range a {
		for _, other := range b {
			if id == other {
				return nil
			}
		}
	}
	return fmt.Errorf("tree %s is not a fork of %s (or of a common upstream): %w", src.Name, tree.Name, domain.ErrInvalid)
}

// forkChain returns t's id followed by the ids of its upstreams (ForkOf),
// nearest first; a deleted upstream ends the chain (its id included).
func (s *Service) forkChain(ctx context.Context, t *domain.Tree) []string {
	out := []string{t.ID}
	cur := t
	for range maxForkDepth {
		if cur.ForkOf == "" {
			break
		}
		out = append(out, cur.ForkOf)
		next, err := s.store.GetTree(ctx, cur.ForkOf)
		if err != nil {
			break
		}
		cur = next
	}
	return out
}

// MRSource describes where the source branch of a merge request lives
// (GET {tree}/merge-requests/{n}/source).
type MRSource struct {
	// CrossTree: the source branch lives in another tree (a fork).
	CrossTree bool   `json:"cross_tree"`
	TreeID    string `json:"tree_id"`
	// Tenant and Tree name the source tree ("" when the caller may not
	// read it).
	Tenant string `json:"tenant"`
	Tree   string `json:"tree"`
	Branch string `json:"branch"`
	// Tip is the source tip in the source tree; TargetTip the same content
	// as a snapshot of the merge request's tree (equal for same-tree
	// requests). Both are "" when the source is missing or empty.
	Tip       string `json:"tip"`
	TargetTip string `json:"target_tip"`
	// Missing: the source tree or branch no longer exists.
	Missing bool `json:"missing"`
}

// MergeRequestSource reports where mr !number's source branch lives and
// its tip as a snapshot of tree (so clients can diff it there). The caller
// needs tree:read on tree.
func (s *Service) MergeRequestSource(ctx context.Context, p *domain.Principal, tree *domain.Tree, number int) (*MRSource, error) {
	if err := checkTree(tree); err != nil {
		return nil, err
	}
	if err := s.authz.Authorize(ctx, p, domain.ActTreeRead, tree.TenantID, tree); err != nil {
		return nil, err
	}
	mr, err := s.getMR(ctx, tree.ID, number)
	if err != nil {
		return nil, err
	}
	out := &MRSource{TreeID: tree.ID, Branch: mr.SourceBranch, CrossTree: crossTree(tree, mr)}
	src := tree
	if out.CrossTree {
		out.TreeID = mr.SourceTreeID
		st, err := s.store.GetTree(ctx, mr.SourceTreeID)
		switch {
		case isNotFound(err):
			out.Missing = true
			return out, nil
		case err != nil:
			return nil, fmt.Errorf("review: source tree: %w", err)
		}
		src = st
	}
	if s.authz.Authorize(ctx, p, domain.ActTreeRead, src.TenantID, src) == nil {
		out.Tree = src.Name
		if t, err := s.store.GetTenant(ctx, src.TenantID); err == nil {
			out.Tenant = t.Slug
		}
	}
	b, err := s.store.GetBranch(ctx, src.ID, mr.SourceBranch)
	switch {
	case isNotFound(err):
		out.Missing = true
		return out, nil
	case err != nil:
		return nil, fmt.Errorf("review: get branch '%s': %w", mr.SourceBranch, err)
	}
	out.Tip = b.Tip
	if b.Tip == "" {
		return out, nil
	}
	if out.TargetTip, err = s.mrSourceTip(ctx, tree, mr); err != nil {
		return nil, err
	}
	return out, nil
}
