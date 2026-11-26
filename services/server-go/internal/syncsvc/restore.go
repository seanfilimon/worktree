package syncsvc

// Reflog restore (specs/tree/Undo.md §4.1, history-v2 follow-up of
// docs/ENGINEERING_PLAN.md §16.10): move a branch to a tip recorded in its
// server reflog. It backs POST {tree}/reflog/restore and `wt undo` of a
// push.
//
// A restore moves the branch ref; it never creates a snapshot. The target
// must be a snapshot the branch pointed at (old_tip or new_tip of one of
// its reflog entries), so a restore can only bring back history the server
// accepted on that branch before. Nothing is destroyed either: the tip the
// restore replaces is kept in a new reflog entry (kind "restore"), so every
// restore can itself be restored.
//
// Rules:
//
//   - lease: expected_tip must equal the current tip, and the branch moves
//     with compare-and-swap from it (a concurrent push loses the lease too);
//   - branch:force_push is required (admin role and up), whichever way the
//     branch moves — a restore is a server-side force push to a recorded
//     tip, and tree:write plus branch:push policies apply first as for any
//     push;
//   - a restore that rewinds (the target does not descend from the current
//     tip) follows the force-push protections (review.CheckForcePush):
//     no_force_push refuses it absolutely, require_linear and required
//     approvals refuse it unless the caller bypasses protection;
//   - a restore that moves forward (the target descends from the current
//     tip, e.g. bringing back a rewound tip) follows the direct-push
//     protections (no_direct_push, allowed pushers), which can be bypassed;
//   - bypass = true needs branch:bypass_protection (forbidden otherwise,
//     like the merge bypass) and is always audited.
//
// A restore publishes branch.pushed (payload "restored": true and "forced"
// for rewinds, so watchers, mirrors and indexes follow the new tip) and
// branch.restored, appends a reflog entry and is audited as branch.restore
// (refusals as branch.restore_rejected).

import (
	"context"
	"errors"
	"fmt"
	"strings"
	"unicode"
	"unicode/utf8"

	"github.com/seanfilimon/worktree/services/server-go/internal/authz"
	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// Reflog kinds, audit actions and the event of a restore.
const (
	ReflogKindPush      = "push"
	ReflogKindForcePush = "force_push"
	ReflogKindRestore   = "restore"

	AuditBranchRestore         = "branch.restore"
	AuditBranchRestoreRejected = "branch.restore_rejected"

	// EventBranchRestored mirrors domain.EventBranchRestored.
	EventBranchRestored = domain.EventBranchRestored
)

// RestoreRequest asks to move Branch to Tip.
type RestoreRequest struct {
	Branch string
	// Tip is the snapshot to restore; it must be the old_tip or new_tip of
	// one of the branch's reflog entries.
	Tip string
	// ExpectedTip is the lease: the branch tip the caller saw. Required.
	ExpectedTip string
	// Bypass asks to bypass branch protection (branch:bypass_protection).
	Bypass bool
	// RemoteAddr is recorded in the audit log (REST callers).
	RemoteAddr string
}

// RestoreResult is the outcome of a successful restore.
type RestoreResult struct {
	Branch string `json:"branch"`
	// OldTip is the tip the restore replaced, NewTip the restored one.
	OldTip string `json:"old_tip"`
	NewTip string `json:"new_tip"`
	// Forced: the restore rewound the branch (old_tip is not an ancestor of
	// new_tip).
	Forced   bool     `json:"forced"`
	Bypass   bool     `json:"bypass"`
	Bypassed []string `json:"bypassed"`
	// Entry is the reflog entry recording the restore.
	Entry ReflogEntry `json:"entry"`
}

// RestoreConflict reports a lost lease: the branch is no longer at the tip
// the caller expected. It wraps domain.ErrConflict.
type RestoreConflict struct {
	Branch      string
	CurrentTip  string
	ExpectedTip string
}

func (e *RestoreConflict) Error() string {
	return fmt.Sprintf("branch '%s' moved: its tip is now %s, not %s (someone pushed since); reload the reflog and retry",
		e.Branch, shortTip(e.CurrentTip), shortTip(e.ExpectedTip))
}

func (e *RestoreConflict) Unwrap() error { return domain.ErrConflict }

// RestoreBranch moves req.Branch of tree t to req.Tip, a tip recorded in the
// branch's server reflog (see the rules at the top of this file).
//
// Errors wrap domain sentinels: ErrUnauthenticated, ErrInvalid (malformed
// request, tip not in the reflog, already at the tip), ErrForbidden
// (permissions), *domain.PushDenied of kind branch_protection (protection),
// ErrNotFound (unknown branch, or a target whose snapshot or manifest is no
// longer stored) and *RestoreConflict (lost lease).
func (s *Service) RestoreBranch(ctx context.Context, p *domain.Principal, t *domain.Tree, req RestoreRequest) (*RestoreResult, error) {
	if p == nil {
		return nil, fmt.Errorf("restoring a branch requires authentication: %w", domain.ErrUnauthenticated)
	}
	if t == nil {
		return nil, fmt.Errorf("tree is required: %w", domain.ErrInvalid)
	}
	if err := validRestoreRequest(req); err != nil {
		return nil, err
	}
	branch := req.Branch
	ctx = authz.WithRequestCache(ctx)
	reject := func(reason string) {
		s.audit(ctx, p, domain.AuditEvent{
			TenantID:     t.TenantID,
			Action:       AuditBranchRestoreRejected,
			ResourceType: "branch",
			ResourceID:   t.ID + "/" + branch,
			RemoteAddr:   req.RemoteAddr,
			Details:      map[string]string{"tree": t.Name, "branch": branch, "tip": req.Tip, "reason": reason},
		})
	}

	// Authorization: as a push (tree:write, branch:push policies), then
	// branch:force_push.
	if err := s.authorizeRestore(ctx, p, t, branch); err != nil {
		if errors.Is(err, domain.ErrForbidden) {
			reject("access_denied")
		}
		return nil, err
	}
	if req.Bypass && s.d.Authz.Authorize(ctx, p, domain.ActProtectionBypass, t.TenantID, t) != nil {
		reject("access_denied")
		return nil, fmt.Errorf("bypassing branch protection requires %s: %w", domain.ActProtectionBypass, domain.ErrForbidden)
	}

	cur, err := s.d.Store.GetBranch(ctx, t.ID, branch)
	if err != nil {
		if errors.Is(err, domain.ErrNotFound) {
			return nil, fmt.Errorf("branch '%s' not found: %w", branch, domain.ErrNotFound)
		}
		return nil, err
	}
	if cur.Tip != req.ExpectedTip {
		reject("conflict")
		return nil, &RestoreConflict{Branch: branch, CurrentTip: cur.Tip, ExpectedTip: req.ExpectedTip}
	}
	if cur.Tip == req.Tip {
		return nil, fmt.Errorf("branch '%s' is already at %s: %w", branch, shortTip(req.Tip), domain.ErrInvalid)
	}
	if err := s.checkRestoreTarget(ctx, t, branch, req.Tip); err != nil {
		return nil, err
	}

	forward := cur.Tip == ""
	if !forward {
		if forward, err = s.d.History.IsAncestor(ctx, cur.Tip, req.Tip); err != nil {
			return nil, fmt.Errorf("ancestry check: %w", err)
		}
	}
	bypassed, err := s.checkRestoreProtection(ctx, p, t, branch, forward, req.Bypass)
	if err != nil {
		var denied *domain.PushDenied
		if errors.As(err, &denied) {
			reject("branch_protection")
		} else if errors.Is(err, domain.ErrForbidden) {
			reject("access_denied")
		}
		return nil, err
	}

	if err := s.d.Store.CompareAndSwapBranchTip(ctx, t.ID, branch, cur.Tip, req.Tip); err != nil {
		if !errors.Is(err, domain.ErrConflict) {
			return nil, fmt.Errorf("update branch '%s': %w", branch, err)
		}
		now, terr := s.branchTip(ctx, t.ID, branch)
		if terr != nil {
			return nil, terr
		}
		reject("conflict")
		return nil, &RestoreConflict{Branch: branch, CurrentTip: now, ExpectedTip: req.ExpectedTip}
	}

	res := &RestoreResult{
		Branch:   branch,
		OldTip:   cur.Tip,
		NewTip:   req.Tip,
		Forced:   !forward,
		Bypass:   len(bypassed) > 0,
		Bypassed: bypassed,
	}
	res.Entry = s.recordReflog(ctx, p, t, ReflogEntry{
		Branch:   branch,
		OldTip:   cur.Tip,
		NewTip:   req.Tip,
		Kind:     ReflogKindRestore,
		Forced:   res.Forced,
		Bypass:   res.Bypass,
		Bypassed: bypassed,
	})
	s.announceRestore(ctx, p, t, res, req.RemoteAddr)
	return res, nil
}

// validRestoreRequest checks the shape of req (wrapping domain.ErrInvalid).
func validRestoreRequest(req RestoreRequest) error {
	switch {
	case req.Branch == "" || len(req.Branch) > maxRefLen:
		return fmt.Errorf("branch must be 1-%d bytes: %w", maxRefLen, domain.ErrInvalid)
	case !utf8.ValidString(req.Branch) || strings.ContainsFunc(req.Branch, func(r rune) bool {
		return unicode.IsSpace(r) || unicode.IsControl(r)
	}):
		return fmt.Errorf("branch name %q is invalid: %w", req.Branch, domain.ErrInvalid)
	case !validID(req.Tip):
		return fmt.Errorf("tip is missing or malformed: %w", domain.ErrInvalid)
	case !validID(req.ExpectedTip):
		return fmt.Errorf("expected_tip (the current branch tip) is missing or malformed: %w", domain.ErrInvalid)
	}
	return nil
}

// authorizeRestore requires tree:write, no access policy denying branch:push
// on the branch, and branch:force_push.
func (s *Service) authorizeRestore(ctx context.Context, p *domain.Principal, t *domain.Tree, branch string) error {
	if err := s.d.Authz.Authorize(ctx, p, domain.ActTreeWrite, t.TenantID, t); err != nil {
		if errors.Is(err, domain.ErrForbidden) {
			return fmt.Errorf("restoring branch '%s' requires %s: %w", branch, domain.ActTreeWrite, domain.ErrForbidden)
		}
		return err
	}
	if ev, ok := s.d.Authz.(domain.AccessEvaluator); ok {
		res := domain.Resource{TenantID: t.TenantID, Tree: t, Branch: branch}
		d, err := ev.Evaluate(ctx, p, domain.ActBranchPush, res, authz.RequestEnvFrom(ctx))
		if err != nil {
			return err
		}
		if !d.Allowed {
			return fmt.Errorf("pushing to branch '%s' is denied by an access policy: %w", branch, domain.ErrForbidden)
		}
	}
	if err := s.d.Authz.Authorize(ctx, p, domain.ActBranchForcePush, t.TenantID, t); err != nil {
		if errors.Is(err, domain.ErrForbidden) {
			return fmt.Errorf("restoring branch '%s' requires %s: %w", branch, domain.ActBranchForcePush, domain.ErrForbidden)
		}
		return err
	}
	return nil
}

// checkRestoreTarget requires tip to be recorded in the branch's reflog
// and its snapshot (and manifest) to still be stored.
func (s *Service) checkRestoreTarget(ctx context.Context, t *domain.Tree, branch, tip string) error {
	entries, err := ListReflog(ctx, s.d.Store, t.ID, branch, 0)
	if err != nil {
		return fmt.Errorf("read reflog: %w", err)
	}
	found := false
	for _, e := range entries {
		if e.OldTip == tip || e.NewTip == tip {
			found = true
			break
		}
	}
	if !found {
		return fmt.Errorf("snapshot %s is not in the reflog of branch '%s' (only tips the branch had in the last %d days can be restored): %w",
			shortTip(tip), branch, int(ReflogTTL.Hours()/24), domain.ErrInvalid)
	}
	snap, err := s.d.Store.GetSnapshot(ctx, tip)
	if err != nil || snap.TreeID != t.ID {
		if err == nil || errors.Is(err, domain.ErrNotFound) {
			return fmt.Errorf("snapshot %s is no longer stored on the server: %w", shortTip(tip), domain.ErrNotFound)
		}
		return err
	}
	if s.d.Objects != nil && !snap.ManifestHash.IsZero() {
		ok, err := s.d.Objects.Has(ctx, snap.ManifestHash)
		if err != nil {
			return err
		}
		if !ok {
			return fmt.Errorf("the content of snapshot %s was garbage-collected: %w", shortTip(tip), domain.ErrNotFound)
		}
	}
	return nil
}

// checkRestoreProtection applies branch protection to a restore and returns
// the bypassed refusals. Rewinds follow the force-push rules, forward moves
// the direct-push rules.
func (s *Service) checkRestoreProtection(ctx context.Context, p *domain.Principal, t *domain.Tree, branch string, forward, bypass bool) ([]string, error) {
	if s.d.Review == nil {
		return []string{}, nil
	}
	bypassed := []string{}
	if forward {
		err := s.d.Review.CheckPush(ctx, p, t, branch)
		if err == nil {
			return bypassed, nil
		}
		var denied *domain.PushDenied
		if bypass && errors.As(err, &denied) && denied.Kind == "branch_protection" {
			return append(bypassed, denied.Reason), nil
		}
		return nil, err
	}
	fp, ok := s.d.Review.(forcePushPolicy)
	if !ok {
		return bypassed, nil
	}
	rules, err := fp.CheckForcePush(ctx, p, t, branch)
	if err != nil {
		return nil, err // no_force_push is absolute
	}
	if len(rules) == 0 {
		return bypassed, nil
	}
	if !bypass {
		return nil, &domain.PushDenied{Kind: "branch_protection", Reason: "branch '" + branch + "' is protected (" +
			strings.Join(rules, ", ") + "): restoring an older tip rewinds it and needs branch:bypass_protection (bypass)"}
	}
	for _, r := range rules {
		bypassed = append(bypassed, "restore past "+r)
	}
	return bypassed, nil
}

// announceRestore publishes the events and audit records of a restore.
func (s *Service) announceRestore(ctx context.Context, p *domain.Principal, t *domain.Tree, res *RestoreResult, remote string) {
	s.publish(domain.Event{
		Type:     domain.EventBranchPushed,
		TenantID: t.TenantID,
		TreeID:   t.ID,
		Actor:    p.Username,
		Payload: map[string]any{
			"branch":   res.Branch,
			"old_tip":  res.OldTip,
			"new_tip":  res.NewTip,
			"count":    0,
			"forced":   res.Forced,
			"restored": true,
		},
	})
	s.publish(domain.Event{
		Type:     EventBranchRestored,
		TenantID: t.TenantID,
		TreeID:   t.ID,
		Actor:    p.Username,
		Payload: map[string]any{
			"branch":  res.Branch,
			"old_tip": res.OldTip,
			"new_tip": res.NewTip,
			"forced":  res.Forced,
			"bypass":  res.Bypass,
		},
	})
	details := map[string]string{
		"tree":    t.Name,
		"branch":  res.Branch,
		"old_tip": res.OldTip,
		"new_tip": res.NewTip,
		"forced":  fmt.Sprint(res.Forced),
	}
	if res.Bypass {
		details["bypass"] = "true"
		details["bypassed"] = strings.Join(res.Bypassed, "; ")
	}
	s.audit(ctx, p, domain.AuditEvent{
		TenantID:     t.TenantID,
		Action:       AuditBranchRestore,
		ResourceType: "branch",
		ResourceID:   t.ID + "/" + res.Branch,
		RemoteAddr:   remote,
		Details:      details,
	})
	if res.Bypass {
		s.audit(ctx, p, domain.AuditEvent{
			TenantID:     t.TenantID,
			Action:       AuditProtectionBypassed,
			ResourceType: "branch",
			ResourceID:   t.ID + "/" + res.Branch,
			RemoteAddr:   remote,
			Details: map[string]string{
				"tree":      t.Name,
				"branch":    res.Branch,
				"operation": "restore",
				"bypass":    "true",
				"rules":     strings.Join(res.Bypassed, "; "),
				"old_tip":   res.OldTip,
				"new_tip":   res.NewTip,
			},
		})
	}
}

func shortTip(id string) string {
	if id == "" {
		return "(none)"
	}
	if len(id) > 8 {
		return id[:8]
	}
	return id
}
