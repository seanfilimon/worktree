package review

// v0.8 history-v2 (docs/ENGINEERING_PLAN.md §16.10): protection bypass on
// merges. A caller holding branch:bypass_protection (owners by default)
// may merge past the review gates of the target's protection (required
// approvals, change requests, owner approvals, CI and blocking TODOs) by
// asking for it explicitly (MergeOptions.Bypass, REST body
// {"bypass": true}). The bypass is always audited ("bypass": "true" on
// mr.merge plus a branch.protection_bypassed record). Drafts, linked
// groups, closed merge requests, merge-method restrictions and conflicts
// are never bypassed.

import (
	"context"
	"fmt"
	"slices"
	"strings"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// bypassableBlockers are the merge blockers branch:bypass_protection lifts.
var bypassableBlockers = []string{BlockerApprovals, BlockerChangesRequested, BlockerOwners, BlockerCI, BlockerTodos}

// AuditProtectionBypassed is the audit action recorded for every bypass of
// branch protection (merges here, pushes in internal/syncsvc).
const AuditProtectionBypassed = "branch.protection_bypassed"

// bypassMerge decides a merge whose readiness check failed with mergeErr.
// Without opts.Bypass, or when a blocker that cannot be bypassed applies,
// it returns mergeErr. With the bypass it requires
// domain.ActProtectionBypass (ErrForbidden otherwise) and returns the
// bypassed blocker kinds.
func (s *Service) bypassMerge(ctx context.Context, p *domain.Principal, tree *domain.Tree, ev *evaluation, requested bool, mergeErr error) ([]string, error) {
	if !requested {
		return nil, mergeErr
	}
	if err := ev.hardMergeError(); err != nil {
		return nil, err
	}
	if err := s.authz.Authorize(ctx, p, domain.ActProtectionBypass, tree.TenantID, tree); err != nil {
		return nil, fmt.Errorf("bypassing branch protection requires %s: %w", domain.ActProtectionBypass, err)
	}
	var kinds []string
	for _, b := range ev.blockers(bypassableBlockers...) {
		if !slices.Contains(kinds, b.Kind) {
			kinds = append(kinds, b.Kind)
		}
	}
	return kinds, nil
}

// hardMergeError is the part of mergeError that a bypass never lifts.
func (ev *evaluation) hardMergeError() error {
	if ev.sourceErr != nil {
		return ev.sourceErr
	}
	if ev.mr.State != domain.MRStateOpen {
		return requireOpen(ev.mr)
	}
	if bs := ev.blockers(BlockerDraft); len(bs) > 0 {
		return &BlockedError{Code: CodeDraft, Message: bs[0].Message, Blockers: bs}
	}
	if bs := ev.blockers(BlockerLinkedGroup); len(bs) > 0 {
		return &BlockedError{Code: CodeLinkedGroup, Message: bs[0].Message, Blockers: bs}
	}
	return nil
}

// bypassAudit returns the extra mr.merge audit details of a bypassed merge
// and records the branch.protection_bypassed event (nil when nothing was
// bypassed).
func (s *Service) bypassAudit(ctx context.Context, p *domain.Principal, tree *domain.Tree, mr *domain.MergeRequest, kinds []string) map[string]string {
	if len(kinds) == 0 {
		return nil
	}
	rules := strings.Join(kinds, ",")
	if s.audit != nil {
		ev := domain.AuditEvent{
			TenantID:     tree.TenantID,
			Actor:        "anonymous",
			Action:       AuditProtectionBypassed,
			ResourceType: "branch",
			ResourceID:   tree.ID + "/" + mr.TargetBranch,
			Details: map[string]string{
				"tree":          tree.Name,
				"branch":        mr.TargetBranch,
				"operation":     "merge",
				"merge_request": fmt.Sprint(mr.Number),
				"bypass":        "true",
				"rules":         rules,
			},
			At: s.now(),
		}
		if !p.Anonymous() {
			ev.ActorID, ev.Actor = p.AccountID, p.Username
		}
		s.audit.Record(ctx, ev)
	}
	return map[string]string{"bypass": "true", "bypassed": rules}
}
