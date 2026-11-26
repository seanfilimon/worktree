package syncsvc

// Reflog restore (restore.go, specs/tree/Undo.md §4.1) through the real
// meta store, authorizer and review service.

import (
	"context"
	"errors"
	"strings"
	"testing"

	"github.com/seanfilimon/worktree/services/server-go/internal/authz"
	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
	"github.com/seanfilimon/worktree/services/server-go/internal/review"
)

// pushed pushes c1 ← c2 to main as dave and returns both.
func (f *forceFixture) pushed() (c1, c2 commit) {
	f.t.Helper()
	c1 = f.commit(map[string]string{"a.txt": "1"})
	c2 = f.commit(map[string]string{"a.txt": "2"}, c1)
	if resp := f.push(f.as("dave", false), pushReq(f.tree.ID, "main", "", c1)); !resp.GetAccepted() {
		f.t.Fatalf("push c1: %v", resp)
	}
	if resp := f.push(f.as("dave", false), pushReq(f.tree.ID, "main", c1.id, c2)); !resp.GetAccepted() {
		f.t.Fatalf("push c2: %v", resp)
	}
	return c1, c2
}

func (f *forceFixture) restore(user, tip, expected string, bypass bool) (*RestoreResult, error) {
	return f.svc.RestoreBranch(f.ctx, f.users[user], f.tree, RestoreRequest{Branch: "main", Tip: tip, ExpectedTip: expected, Bypass: bypass})
}

func wantDenied(t *testing.T, err error, substr string) {
	t.Helper()
	var denied *domain.PushDenied
	if !errors.As(err, &denied) || denied.Kind != "branch_protection" || !strings.Contains(denied.Reason, substr) {
		t.Fatalf("err = %v, want a branch_protection refusal mentioning %q", err, substr)
	}
}

func TestRestoreBranch(t *testing.T) {
	f := newForceFixture(t)
	c1, c2 := f.pushed()

	// Viewers and developers lack the permissions; refusals are audited.
	if _, err := f.restore("vera", c1.id, c2.id, false); !errors.Is(err, domain.ErrForbidden) || !strings.Contains(err.Error(), "tree:write") {
		t.Fatalf("viewer: %v", err)
	}
	_, err := f.restore("dave", c1.id, c2.id, false)
	if !errors.Is(err, domain.ErrForbidden) || !strings.Contains(err.Error(), string(domain.ActBranchForcePush)) {
		t.Fatalf("developer: %v", err)
	}
	if n := len(f.audit.actions(AuditBranchRestoreRejected)); n != 2 {
		t.Fatalf("rejections audited = %d, want 2", n)
	}
	if _, err := f.svc.RestoreBranch(f.ctx, nil, f.tree, RestoreRequest{Branch: "main", Tip: c1.id, ExpectedTip: c2.id}); !errors.Is(err, domain.ErrUnauthenticated) {
		t.Fatalf("anonymous: %v", err)
	}
	// Bypass without branch:bypass_protection is forbidden.
	if _, err := f.restore("adam", c1.id, c2.id, true); !errors.Is(err, domain.ErrForbidden) || !strings.Contains(err.Error(), "bypass_protection") {
		t.Fatalf("admin bypass: %v", err)
	}

	// Lease: the branch moved since the caller looked.
	_, err = f.restore("adam", c1.id, c1.id, false)
	var conflict *RestoreConflict
	if !errors.As(err, &conflict) || !errors.Is(err, domain.ErrConflict) || conflict.CurrentTip != c2.id {
		t.Fatalf("stale lease: %v", err)
	}
	// Only tips of the branch's reflog can be restored.
	c3 := f.commit(map[string]string{"a.txt": "3"}, c1)
	if _, err := f.restore("adam", c3.id, c2.id, false); !errors.Is(err, domain.ErrInvalid) || !strings.Contains(err.Error(), "not in the reflog") {
		t.Fatalf("unknown tip: %v", err)
	}
	if _, err := f.restore("adam", c2.id, c2.id, false); !errors.Is(err, domain.ErrInvalid) || !strings.Contains(err.Error(), "already at") {
		t.Fatalf("same tip: %v", err)
	}
	if _, err := f.svc.RestoreBranch(f.ctx, f.users["adam"], f.tree, RestoreRequest{Branch: "nope", Tip: c1.id, ExpectedTip: c2.id}); !errors.Is(err, domain.ErrNotFound) {
		t.Fatalf("unknown branch: %v", err)
	}
	for _, req := range []RestoreRequest{{Tip: c1.id, ExpectedTip: c2.id}, {Branch: "main", ExpectedTip: c2.id}, {Branch: "main", Tip: c1.id}, {Branch: "a b", Tip: c1.id, ExpectedTip: c2.id}} {
		if _, err := f.svc.RestoreBranch(f.ctx, f.users["adam"], f.tree, req); !errors.Is(err, domain.ErrInvalid) {
			t.Fatalf("%+v: %v", req, err)
		}
	}
	if f.tip("main") != c2.id {
		t.Fatal("refused restores must not move main")
	}

	// An admin rewinds main to c1 (undoing dave's second push).
	res, err := f.restore("adam", c1.id, c2.id, false)
	if err != nil {
		t.Fatal(err)
	}
	if !res.Forced || res.OldTip != c2.id || res.NewTip != c1.id || res.Bypass || f.tip("main") != c1.id {
		t.Fatalf("restore = %+v, main = %s", res, f.tip("main"))
	}
	if e := res.Entry; e.Kind != ReflogKindRestore || e.OldTip != c2.id || e.NewTip != c1.id || !e.Forced || e.Actor != "adam" || e.Bypassed == nil {
		t.Fatalf("entry = %+v", e)
	}
	entries, err := ListReflog(f.ctx, f.store, f.tree.ID, "main", 0)
	must(t, err)
	if len(entries) != 3 || entries[0].Kind != ReflogKindRestore || entries[0].OldTip != c2.id {
		t.Fatalf("reflog = %+v", entries)
	}
	restored := f.bus.ofType(EventBranchRestored)
	if len(restored) != 1 || restored[0].Actor != "adam" {
		t.Fatalf("restored events = %+v", restored)
	}
	if p := restored[0].Payload.(map[string]any); p["old_tip"] != c2.id || p["new_tip"] != c1.id || p["forced"] != true {
		t.Fatalf("restored payload = %v", p)
	}
	pushed := f.bus.ofType(domain.EventBranchPushed)
	if last := pushed[len(pushed)-1].Payload.(map[string]any); last["restored"] != true || last["new_tip"] != c1.id || last["forced"] != true {
		t.Fatalf("branch.pushed payload = %v", last)
	}
	audits := f.audit.actions(AuditBranchRestore)
	if len(audits) != 1 || audits[0].Details["old_tip"] != c2.id || audits[0].Details["forced"] != "true" || audits[0].Actor != "adam" {
		t.Fatalf("audit = %+v", audits)
	}

	// The replaced tip is in the reflog, so the restore can be restored
	// (forward this time: c2 descends from c1).
	res, err = f.restore("adam", c2.id, c1.id, false)
	if err != nil || res.Forced || f.tip("main") != c2.id {
		t.Fatalf("restore forward = %+v, %v", res, err)
	}
	if n := len(f.bus.ofType(domain.EventBranchForced)); n != 0 {
		t.Fatalf("restores must not publish branch.force_pushed (%d)", n)
	}
}

func TestRestoreProtections(t *testing.T) {
	f := newForceFixture(t)
	c1, c2 := f.pushed()

	// require_linear: rewinding needs the owner bypass.
	f.protect(domain.BranchProtection{Pattern: "main", RequireLinear: true})
	wantDenied(t, func() error { _, err := f.restore("adam", c1.id, c2.id, false); return err }(), "require_linear")
	wantDenied(t, func() error { _, err := f.restore("olivia", c1.id, c2.id, false); return err }(), "bypass")
	res, err := f.restore("olivia", c1.id, c2.id, true)
	if err != nil || !res.Bypass || !res.Entry.Bypass || len(res.Bypassed) == 0 {
		t.Fatalf("owner bypass = %+v, %v", res, err)
	}
	bypass := f.audit.actions(AuditProtectionBypassed)
	if len(bypass) != 1 || bypass[0].Details["operation"] != "restore" || bypass[0].Actor != "olivia" {
		t.Fatalf("bypass audit = %+v", bypass)
	}
	if a := f.audit.actions(AuditBranchRestore); len(a) != 1 || a[0].Details["bypass"] != "true" {
		t.Fatalf("restore audit = %+v", a)
	}

	// Forward moves follow the direct-push rules (no_direct_push), which
	// owners may bypass.
	f.protect(domain.BranchProtection{Pattern: "main", NoDirectPush: true})
	wantDenied(t, func() error { _, err := f.restore("adam", c2.id, c1.id, false); return err }(), "merge request")
	if res, err := f.restore("olivia", c2.id, c1.id, true); err != nil || res.Forced || !res.Bypass {
		t.Fatalf("forward bypass = %+v, %v", res, err)
	}

	// no_force_push refuses rewinds absolutely, even with the bypass.
	f.protect(domain.BranchProtection{Pattern: "main", NoForcePush: true})
	wantDenied(t, func() error { _, err := f.restore("olivia", c1.id, c2.id, true); return err }(), "no_force_push")
	if f.tip("main") != c2.id {
		t.Fatalf("main = %s, want %s", f.tip("main"), c2.id)
	}
	if n := len(f.audit.actions(AuditBranchRestoreRejected)); n != 4 {
		t.Fatalf("rejections audited = %d, want 4", n)
	}
}

// racingStore moves the branch (a concurrent push) right before the
// restore's compare-and-swap.
type racingStore struct {
	*meta.Memory
	race func()
}

func (r *racingStore) CompareAndSwapBranchTip(ctx context.Context, treeID, name, oldTip, newTip string) error {
	if r.race != nil {
		race := r.race
		r.race = nil
		race()
	}
	return r.Memory.CompareAndSwapBranchTip(ctx, treeID, name, oldTip, newTip)
}

func TestRestoreLosesRaceToConcurrentPush(t *testing.T) {
	f := newForceFixture(t)
	c1, c2 := f.pushed()
	c3 := f.commit(map[string]string{"a.txt": "3"}, c2)
	racing := &racingStore{Memory: f.store}
	hist := meta.NewHistory(racing)
	az := authz.New(racing, authz.Options{})
	svc := New(Deps{Store: racing, Objects: f.objects, History: hist, Authz: az, Review: review.New(racing, hist, az, nil, nil), Bus: f.bus, Audit: f.audit})
	racing.race = func() {
		if resp := f.push(f.as("dave", false), pushReq(f.tree.ID, "main", c2.id, c3)); !resp.GetAccepted() {
			t.Fatalf("concurrent push: %v", resp)
		}
	}
	_, err := svc.RestoreBranch(f.ctx, f.users["adam"], f.tree, RestoreRequest{Branch: "main", Tip: c1.id, ExpectedTip: c2.id})
	var conflict *RestoreConflict
	if !errors.As(err, &conflict) || conflict.CurrentTip != c3.id || !strings.Contains(err.Error(), "moved") {
		t.Fatalf("race: %v", err)
	}
	if f.tip("main") != c3.id {
		t.Fatalf("main = %s, want the concurrent push %s", f.tip("main"), c3.id)
	}
}

func TestRestoreRefusesCollectedContent(t *testing.T) {
	f := newForceFixture(t)
	c1, c2 := f.pushed()
	must(t, f.objects.Delete(f.ctx, c1.manifest))
	if _, err := f.restore("adam", c1.id, c2.id, false); !errors.Is(err, domain.ErrNotFound) || !strings.Contains(err.Error(), "garbage-collected") {
		t.Fatalf("collected: %v", err)
	}
}
