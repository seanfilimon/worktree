package review

// v0.8 history-v2 (§16.10): force-push rules and the merge bypass.

import (
	"context"
	"errors"
	"slices"
	"testing"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

func TestResolveNoForcePush(t *testing.T) {
	rules := []domain.BranchProtection{
		{Pattern: "main"},
		{Pattern: "release/**", NoForcePush: true},
	}
	if Resolve(rules, "main").NoForcePush {
		t.Fatal("main must allow force pushes")
	}
	if !Resolve(rules, "release/1.0").NoForcePush {
		t.Fatal("release/1.0 must refuse force pushes")
	}
}

func TestCheckForcePush(t *testing.T) {
	ctx := context.Background()
	e := newEnv(t)
	e.authz.grants["carol"] = append(slices.Clone(maintainer), domain.ActBranchForcePush)

	// Without branch:force_push the authorizer's error is passed through.
	if _, err := e.svc.CheckForcePush(ctx, alice, e.tree, "main"); !errors.Is(err, domain.ErrForbidden) || err != e.authz.last() {
		t.Fatalf("alice: %v", err)
	}
	// Unprotected: allowed, nothing to bypass.
	rules, err := e.svc.CheckForcePush(ctx, carol, e.tree, "main")
	if err != nil || len(rules) != 0 {
		t.Fatalf("unprotected: %v %v", rules, err)
	}
	// require_linear and approvals need a bypass.
	e.protect(domain.BranchProtection{Pattern: "main", RequireLinear: true, RequiredApprovals: 1})
	rules, err = e.svc.CheckForcePush(ctx, carol, e.tree, "main")
	if err != nil || !slices.Equal(rules, []string{ForceRuleLinear, ForceRuleApprovals}) {
		t.Fatalf("linear: %v %v", rules, err)
	}
	// no_force_push is absolute.
	e.protect(domain.BranchProtection{Pattern: "ma*", NoForcePush: true})
	_, err = e.svc.CheckForcePush(ctx, carol, e.tree, "main")
	var denied *domain.PushDenied
	if !errors.As(err, &denied) || denied.Kind != "branch_protection" {
		t.Fatalf("no_force_push: %v", err)
	}
	if _, err := e.svc.CheckForcePush(ctx, carol, nil, "main"); !errors.Is(err, domain.ErrInvalid) {
		t.Fatalf("nil tree: %v", err)
	}
}

func TestMergeBypass(t *testing.T) {
	ctx := context.Background()
	e := newEnv(t)
	e.protect(domain.BranchProtection{Pattern: "main", RequiredApprovals: 2, RequireCIPass: true})
	e.open(t)

	// Without bypass the merge is refused as before.
	_, err := e.svc.MergeWith(ctx, carol, e.tree, 1, MergeOptions{})
	var denied *domain.PushDenied
	if !errors.As(err, &denied) || denied.Kind != "review_required" {
		t.Fatalf("no bypass: %v", err)
	}
	// Asking for a bypass without branch:bypass_protection is forbidden.
	_, err = e.svc.MergeWith(ctx, carol, e.tree, 1, MergeOptions{Bypass: true})
	if !errors.Is(err, domain.ErrForbidden) {
		t.Fatalf("carol bypass: %v", err)
	}
	// An owner with the permission merges past approvals and CI; audited.
	e.authz.grants["carol"] = append(slices.Clone(maintainer), domain.ActProtectionBypass)
	mr, err := e.svc.MergeWith(ctx, carol, e.tree, 1, MergeOptions{Bypass: true})
	if err != nil || mr.State != domain.MRStateMerged {
		t.Fatalf("bypass merge: %v %+v", err, mr)
	}
	if tip, _ := e.store.tip(e.tree.ID, "main"); tip != s3 {
		t.Fatalf("main = %s", tip)
	}
	var sawMerge, sawBypass bool
	for _, a := range e.audit.events {
		switch a.Action {
		case "mr.merge":
			sawMerge = a.Details["bypass"] == "true" && a.Details["bypassed"] == "approvals,ci"
		case AuditProtectionBypassed:
			sawBypass = a.Details["operation"] == "merge" && a.Details["bypass"] == "true" && a.Actor == "carol"
		}
	}
	if !sawMerge || !sawBypass {
		t.Fatalf("audit = %+v", e.audit.events)
	}
}

func TestMergeBypassNeverLiftsDrafts(t *testing.T) {
	ctx := context.Background()
	e := newEnv(t)
	e.authz.grants["carol"] = append(slices.Clone(maintainer), domain.ActProtectionBypass)
	mr := e.open(t)
	stored := e.store.mr(t, e.tree.ID, mr.Number)
	stored.Draft = true
	if err := e.store.UpdateMergeRequest(ctx, stored); err != nil {
		t.Fatal(err)
	}
	_, err := e.svc.MergeWith(ctx, carol, e.tree, mr.Number, MergeOptions{Bypass: true})
	blocked(t, err, CodeDraft)
}
