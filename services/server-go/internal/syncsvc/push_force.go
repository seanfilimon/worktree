package syncsvc

// v0.8 history-v2 (docs/ENGINEERING_PLAN.md §16.10): force pushes and the
// protection bypass of Push.
//
// Force push: PushRequest.force moves a branch even when new_tip does not
// descend from the server tip. It needs branch:force_push and is refused by
// protections with no_force_push (absolute), and by require_linear or
// required approvals unless the pusher bypasses branch protection. When
// the request carries old_tip, a force push only succeeds while the server
// tip still equals it (lease semantics: "force-with-lease"); the branch
// then moves with compare-and-swap from that tip. A successful force push
// publishes branch.force_pushed (besides branch.pushed with "forced":
// true), records the old tip in the server reflog and is audited with
// "forced": "true".
//
// Bypass: a pusher holding branch:bypass_protection (owners by default)
// who sends the gRPC metadata `x-wt-bypass-protection: true` (`wt push
// --bypass-protection`) pushes past the branch-protection checks of the
// review service (no_direct_push, allowed pushers, require_linear, and the
// force-push rules above). Every bypass is audited: "bypass": "true" on
// branch.push plus a branch.protection_bypassed record. Without the
// metadata nothing is bypassed, so owners see protection refusals like
// everybody else.

import (
	"context"
	"errors"
	"strings"

	"google.golang.org/grpc/metadata"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
	"github.com/seanfilimon/worktree/services/server-go/internal/review"
)

// BypassMetadataKey is the gRPC metadata key with which a push asks to
// bypass branch protection ("true" or "1").
const BypassMetadataKey = "x-wt-bypass-protection"

// AuditProtectionBypassed is the audit action of every protection bypass.
const AuditProtectionBypassed = review.AuditProtectionBypassed

// forcePushPolicy is implemented by review services that decide force
// pushes (internal/review.Service.CheckForcePush).
type forcePushPolicy interface {
	CheckForcePush(ctx context.Context, p *domain.Principal, tree *domain.Tree, branch string) ([]string, error)
}

// pushState carries the v0.8 decisions of one push.
type pushState struct {
	// bypassRequested: the caller sent BypassMetadataKey.
	bypassRequested bool
	// bypassAllowed caches the branch:bypass_protection check (nil = not
	// checked yet).
	bypassAllowed *bool
	// bypassed lists the refusals that were bypassed.
	bypassed []string
	// forced: the push rewinds the branch.
	forced bool
}

func newPushState(ctx context.Context) *pushState {
	return &pushState{bypassRequested: bypassRequested(ctx)}
}

// bypassRequested reports whether the incoming request carries
// BypassMetadataKey with a true value.
func bypassRequested(ctx context.Context) bool {
	md, ok := metadata.FromIncomingContext(ctx)
	if !ok {
		return false
	}
	for _, v := range md.Get(BypassMetadataKey) {
		switch strings.ToLower(strings.TrimSpace(v)) {
		case "1", "true", "yes":
			return true
		}
	}
	return false
}

// bypassProtection reports whether the branch-protection refusal err may
// be bypassed: it must be a *domain.PushDenied of kind branch_protection,
// the caller must have asked for the bypass and hold
// branch:bypass_protection. The bypassed reason is remembered for the
// audit.
func (s *Service) bypassProtection(ctx context.Context, p *domain.Principal, t *domain.Tree, st *pushState, err error) bool {
	var denied *domain.PushDenied
	if !errors.As(err, &denied) || denied.Kind != "branch_protection" {
		return false
	}
	if !s.mayBypass(ctx, p, t, st) {
		return false
	}
	st.bypassed = append(st.bypassed, denied.Reason)
	return true
}

// mayBypass reports (once per push) whether the caller asked for and may
// bypass branch protection.
func (s *Service) mayBypass(ctx context.Context, p *domain.Principal, t *domain.Tree, st *pushState) bool {
	if !st.bypassRequested {
		return false
	}
	if st.bypassAllowed == nil {
		ok := s.d.Authz.Authorize(ctx, p, domain.ActProtectionBypass, t.TenantID, t) == nil
		st.bypassAllowed = &ok
	}
	return *st.bypassAllowed
}

// isProtectionCheck reports whether the push checker c is the review
// service, whose refusals are branch protection (RequireLinear).
func (s *Service) isProtectionCheck(c domain.PushChecker) bool {
	if s.d.Review == nil || c == nil {
		return false
	}
	rc, ok := s.d.Review.(domain.PushChecker)
	return ok && rc == c
}

// checkForcePush decides a push whose new tip does not descend from the
// server tip (step 6 of Push). It returns (nil, nil) when the force push
// may proceed (st.forced is then set), otherwise the rejection.
func (s *Service) checkForcePush(ctx context.Context, p *domain.Principal, t *domain.Tree, branch, newTip, serverTip string, req *syncpb.PushRequest, st *pushState) (*syncpb.PushResponse, error) {
	if !req.GetForce() {
		return rejected(conflict(serverTip)), nil
	}
	// Lease: the client's last known tip must still be the server tip.
	if lease := req.GetOldTip().GetValue(); lease != "" && lease != serverTip {
		return rejected(conflict(serverTip)), nil
	}
	if fp, ok := s.d.Review.(forcePushPolicy); ok {
		rules, err := fp.CheckForcePush(ctx, p, t, branch)
		if err != nil {
			if errors.Is(err, domain.ErrForbidden) {
				s.auditRejected(ctx, p, t, branch, newTip, "access_denied")
				return rejected(accessDenied("force pushing to " + branch + " requires " + string(domain.ActBranchForcePush))), nil
			}
			return s.policyRejection(ctx, p, t, branch, newTip, err, "force push check")
		}
		if len(rules) > 0 {
			if !s.mayBypass(ctx, p, t, st) {
				denied := &domain.PushDenied{Kind: "branch_protection", Reason: "branch '" + branch + "' is protected (" +
					strings.Join(rules, ", ") + "): force pushes need branch:bypass_protection (wt push --force --bypass-protection)"}
				return s.policyRejection(ctx, p, t, branch, newTip, denied, "force push check")
			}
			for _, r := range rules {
				st.bypassed = append(st.bypassed, "force push past "+r)
			}
		}
	} else if err := s.d.Authz.Authorize(ctx, p, domain.ActBranchForcePush, t.TenantID, t); err != nil {
		if errors.Is(err, domain.ErrForbidden) {
			s.auditRejected(ctx, p, t, branch, newTip, "access_denied")
			return rejected(accessDenied("force pushing to " + branch + " requires " + string(domain.ActBranchForcePush))), nil
		}
		return nil, s.toStatus(ctx, err, "authorize")
	}
	st.forced = true
	return nil, nil
}

// afterPush runs the v0.8 bookkeeping of an accepted push that moved the
// branch: it annotates the branch.pushed payload and branch.push audit
// details, records the server reflog, publishes branch.force_pushed and
// audits protection bypasses.
func (s *Service) afterPush(ctx context.Context, p *domain.Principal, t *domain.Tree, branch, oldTip, newTip string, count int, st *pushState, payload map[string]any, details map[string]string) {
	bypass := len(st.bypassed) > 0
	if st.forced {
		payload["forced"] = true
		details["forced"] = "true"
	}
	if bypass {
		payload["bypass"] = true
		details["bypass"] = "true"
		details["bypassed"] = strings.Join(st.bypassed, "; ")
	}
	s.recordReflog(ctx, p, t, ReflogEntry{
		Branch:   branch,
		OldTip:   oldTip,
		NewTip:   newTip,
		Forced:   st.forced,
		Bypass:   bypass,
		Bypassed: st.bypassed,
		Count:    count,
	})
	if st.forced {
		s.publish(domain.Event{
			Type:     domain.EventBranchForced,
			TenantID: t.TenantID,
			TreeID:   t.ID,
			Actor:    p.Username,
			Payload: map[string]any{
				"branch":  branch,
				"old_tip": oldTip,
				"new_tip": newTip,
				"bypass":  bypass,
			},
		})
	}
	if bypass {
		operation := "push"
		if st.forced {
			operation = "force_push"
		}
		s.audit(ctx, p, domain.AuditEvent{
			TenantID:     t.TenantID,
			Action:       AuditProtectionBypassed,
			ResourceType: "branch",
			ResourceID:   t.ID + "/" + branch,
			Details: map[string]string{
				"tree":      t.Name,
				"branch":    branch,
				"operation": operation,
				"bypass":    "true",
				"rules":     strings.Join(st.bypassed, "; "),
				"old_tip":   oldTip,
				"new_tip":   newTip,
			},
		})
	}
}
