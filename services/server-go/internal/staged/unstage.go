package staged

// v0.5 (docs/ENGINEERING_PLAN.md §13.11, workstream daemon): explicit
// removal of a staged snapshot (`wt unstage`, `wt stage --discard`, gRPC
// Unstage, REST DELETE {tree}/staged). v0.8 (§16.8): labels and history.

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

// LabelUnstager removes one labeled slot (v0.8, §16.8).
type LabelUnstager interface {
	UnstageLabel(ctx context.Context, p *domain.Principal, tree *domain.Tree, accountID, branch, label string) ([]domain.StagedSnapshot, error)
}

var (
	_ Unstager      = (*Service)(nil)
	_ LabelUnstager = (*Service)(nil)
)

// Unstage removes staged snapshots of accountID in tree: every label of
// branch, or of every branch when branch is "". accountID "" means the
// caller.
//
// Removing one's own entries requires domain.ActTreeRead; removing another
// account's requires domain.ActTreeAdmin. Each removed entry is recorded in
// the history with status "cleared" and publishes domain.EventStagedCleared
// (payload Cleared, Reason "unstaged"). The removed entries are returned
// (empty, not an error, when nothing matched), sorted by branch then label.
func (s *Service) Unstage(ctx context.Context, p *domain.Principal, tree *domain.Tree, accountID, branch string) ([]domain.StagedSnapshot, error) {
	return s.unstage(ctx, p, tree, accountID, branch, nil)
}

// UnstageLabel is Unstage restricted to one slot: (branch, label), where
// label "" is the default slot. branch is required.
func (s *Service) UnstageLabel(ctx context.Context, p *domain.Principal, tree *domain.Tree, accountID, branch, label string) ([]domain.StagedSnapshot, error) {
	if branch == "" {
		return nil, fmt.Errorf("staged: unstage: branch is required with a label: %w", domain.ErrInvalid)
	}
	if err := ValidateLabel(label); err != nil {
		return nil, fmt.Errorf("staged: unstage: %w", err)
	}
	return s.unstage(ctx, p, tree, accountID, branch, &label)
}

func (s *Service) unstage(ctx context.Context, p *domain.Principal, tree *domain.Tree, accountID, branch string, label *string) ([]domain.StagedSnapshot, error) {
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
	removed := []domain.StagedSnapshot{}
	for _, e := range all {
		if e.AccountID != accountID || (branch != "" && e.BranchName != branch) || (label != nil && e.Label != *label) {
			continue
		}
		mu := s.lockFor(tree.ID, e.AccountID, e.BranchName)
		mu.Lock()
		cur, err := s.find(ctx, tree.ID, e.AccountID, e.BranchName, e.Label)
		if err == nil && cur == nil {
			err = domain.ErrNotFound // removed concurrently
		}
		if err == nil {
			e = *cur
			err = s.store.DeleteStagedLabel(ctx, tree.ID, e.AccountID, e.BranchName, e.Label)
		}
		mu.Unlock()
		if errors.Is(err, domain.ErrNotFound) {
			continue
		}
		if err != nil {
			return removed, fmt.Errorf("staged: unstage: %w", err)
		}
		removed = append(removed, e)
		s.ended(ctx, tree, e, StatusCleared, ReasonUnstaged, p.Username, "")
	}
	sort.Slice(removed, func(i, j int) bool {
		if removed[i].BranchName != removed[j].BranchName {
			return removed[i].BranchName < removed[j].BranchName
		}
		return removed[i].Label < removed[j].Label
	})
	return removed, nil
}
