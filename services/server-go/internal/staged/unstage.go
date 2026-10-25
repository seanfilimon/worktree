package staged

// v0.5 (docs/ENGINEERING_PLAN.md §13.11, workstream daemon): explicit
// removal of a staged snapshot (`wt unstage`, `wt stage --discard`, gRPC
// Unstage, REST DELETE {tree}/staged).

import (
	"context"
	"errors"
	"fmt"
	"sort"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// ReasonUnstaged is the Cleared.Reason of an explicit Unstage.
const ReasonUnstaged = "unstaged"

// Unstager is implemented by staged-snapshot services that support explicit
// removal. syncsvc and httpapi type-assert their domain.StagedService to it
// (domain.StagedService itself is shared and only changed additively).
type Unstager interface {
	Unstage(ctx context.Context, p *domain.Principal, tree *domain.Tree, accountID, branch string) ([]domain.StagedSnapshot, error)
}

var _ Unstager = (*Service)(nil)

// Unstage removes staged snapshots of accountID in tree: the entry for
// branch, or every branch when branch is "". accountID "" means the caller.
//
// Removing one's own entries requires domain.ActTreeRead; removing another
// account's requires domain.ActTreeAdmin. Each removed entry publishes
// domain.EventStagedCleared (payload Cleared, Reason "unstaged"). The removed
// entries are returned (empty, not an error, when nothing matched), sorted by
// branch.
func (s *Service) Unstage(ctx context.Context, p *domain.Principal, tree *domain.Tree, accountID, branch string) ([]domain.StagedSnapshot, error) {
	if tree == nil {
		return nil, fmt.Errorf("staged: unstage: nil tree: %w", domain.ErrInvalid)
	}
	if p == nil {
		return nil, fmt.Errorf("staged: unstage: anonymous principal: %w", domain.ErrUnauthenticated)
	}
	if accountID == "" {
		accountID = p.AccountID
	}
	act := domain.ActTreeRead
	if accountID != p.AccountID {
		act = domain.ActTreeAdmin
	}
	if err := s.authz.Authorize(ctx, p, act, tree.TenantID, tree); err != nil {
		return nil, err
	}

	all, err := s.store.ListStaged(ctx, tree.ID, branch)
	if err != nil {
		return nil, fmt.Errorf("staged: unstage: %w", err)
	}
	var removed []domain.StagedSnapshot
	for _, e := range all {
		if e.AccountID != accountID || (branch != "" && e.BranchName != branch) {
			continue
		}
		mu := s.lockFor(tree.ID, e.AccountID, e.BranchName)
		mu.Lock()
		err := s.store.DeleteStaged(ctx, tree.ID, e.AccountID, e.BranchName)
		mu.Unlock()
		if errors.Is(err, domain.ErrNotFound) {
			continue // removed concurrently
		}
		if err != nil {
			return removed, fmt.Errorf("staged: unstage: %w", err)
		}
		removed = append(removed, e)
		s.publish(domain.Event{
			Type:     domain.EventStagedCleared,
			TenantID: tree.TenantID,
			TreeID:   tree.ID,
			Actor:    p.Username,
			Payload: Cleared{
				TreeID:     tree.ID,
				TenantID:   tree.TenantID,
				AccountID:  e.AccountID,
				Author:     e.Author,
				BranchName: e.BranchName,
				SnapshotID: e.SnapshotID,
				Reason:     ReasonUnstaged,
			},
		})
	}
	sort.Slice(removed, func(i, j int) bool { return removed[i].BranchName < removed[j].BranchName })
	if removed == nil {
		removed = []domain.StagedSnapshot{}
	}
	return removed, nil
}
