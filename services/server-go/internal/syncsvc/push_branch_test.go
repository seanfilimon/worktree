package syncsvc

import (
	"context"
	"errors"
	"path"
	"sync"
	"testing"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// branchAuthz adds domain.AccessEvaluator to fakeAuthz: branch:push is
// refused for branches matching deny, and evaluation errors can be
// injected. It records every evaluated request.
type branchAuthz struct {
	*fakeAuthz
	deny string // path.Match pattern of denied branches
	err  error

	mu    sync.Mutex
	calls []domain.Resource
}

func (a *branchAuthz) Evaluate(ctx context.Context, p *domain.Principal, act domain.Action, res domain.Resource, _ domain.RequestEnv) (domain.Decision, error) {
	a.mu.Lock()
	a.calls = append(a.calls, res)
	a.mu.Unlock()
	if a.err != nil {
		return domain.Decision{}, a.err
	}
	if act != domain.ActBranchPush {
		return domain.Decision{}, errors.New("unexpected action " + string(act))
	}
	if ok, _ := path.Match(a.deny, res.Branch); ok {
		return domain.Decision{Effect: "deny", Reasons: []domain.DecisionReason{{Source: "policy", Effect: "deny"}}}, nil
	}
	if err := a.Authorize(ctx, p, domain.ActTreeWrite, res.TenantID, res.Tree); err != nil {
		return domain.Decision{Effect: "implicit_deny"}, nil
	}
	return domain.Decision{Allowed: true, Effect: "allow"}, nil
}

func (a *branchAuthz) EffectivePermissions(context.Context, *domain.Principal, string, *domain.Tree) ([]domain.Action, error) {
	return nil, errors.New("not used")
}

var _ domain.AccessEvaluator = (*branchAuthz)(nil)

func TestPushBranchPolicyDeny(t *testing.T) {
	h := newHarness(t)
	az := &branchAuthz{fakeAuthz: h.authz, deny: "release/*"}
	h.svc.d.Authz = az // before any RPC reaches the service

	ctx := h.as("alice")
	treeID := h.newTree("repo")
	c1 := makeCommit(treeID, map[string]string{"a.txt": "one"})
	h.upload(ctx, c1)

	// A denied branch is rejected with AccessDenied and nothing moves.
	resp, err := h.client.Push(ctx, pushReq(treeID, "release/1.0", "", c1))
	if err != nil {
		t.Fatalf("push: %v", err)
	}
	if resp.GetAccepted() {
		t.Fatal("push to a denied branch was accepted")
	}
	ad := resp.GetRejectionReason().GetAccessDenied()
	if ad == nil || ad.GetReason() == "" {
		t.Fatalf("rejection = %v, want access_denied", resp.GetRejectionReason())
	}
	if tip := h.tip(treeID, "release/1.0"); tip != "" {
		t.Fatalf("denied branch tip = %q, want unset", tip)
	}
	if len(h.audit.actions("branch.push_rejected")) == 0 {
		t.Fatal("denied push was not audited")
	}

	// The evaluator saw the branch-scoped resource.
	az.mu.Lock()
	last := az.calls[len(az.calls)-1]
	az.mu.Unlock()
	if last.Branch != "release/1.0" || last.Tree == nil || last.Tree.ID != treeID || last.TenantID != tenantAcme {
		t.Fatalf("evaluated resource = %+v", last)
	}

	// Other branches of the same tree are unaffected.
	h.mustPush(ctx, pushReq(treeID, "main", "", c1))
	if h.tip(treeID, "main") != c1.id {
		t.Fatal("main was not updated")
	}

	// Evaluation failures are internal errors, not silent allows.
	az.err = errors.New("store down")
	_, err = h.client.Push(ctx, pushReq(treeID, "feature", "", c1))
	if err == nil {
		t.Fatal("push succeeded although branch evaluation failed")
	}
}

// TestPushBranchCheckRunsAfterTreeWrite: callers without tree:write get the
// tree-level refusal; the branch check is not consulted.
func TestPushBranchCheckRunsAfterTreeWrite(t *testing.T) {
	h := newHarness(t)
	az := &branchAuthz{fakeAuthz: h.authz, deny: "*"}
	h.svc.d.Authz = az
	treeID := h.newTree("repo")
	c1 := makeCommit(treeID, map[string]string{"a.txt": "one"})
	h.upload(h.as("alice"), c1)

	resp, err := h.client.Push(h.as("bob"), pushReq(treeID, "main", "", c1)) // bob is a viewer
	if err != nil {
		t.Fatalf("push: %v", err)
	}
	if resp.GetRejectionReason().GetAccessDenied().GetReason() != "you do not have write access to this tree" {
		t.Fatalf("rejection = %v", resp.GetRejectionReason())
	}
	az.mu.Lock()
	n := len(az.calls)
	az.mu.Unlock()
	if n != 0 {
		t.Fatalf("branch evaluation ran %d times for a caller without tree:write", n)
	}
}
