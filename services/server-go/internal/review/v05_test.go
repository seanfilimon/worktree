package review

import (
	"context"
	"errors"
	"fmt"
	"reflect"
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/ci"
	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/events"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
)

// blocked asserts err is a *BlockedError with code and returns it.
func blocked(t *testing.T, err error, code string) *BlockedError {
	t.Helper()
	var be *BlockedError
	if !errors.As(err, &be) {
		t.Fatalf("error = %v (%T), want *BlockedError", err, err)
	}
	if be.Code != code {
		t.Fatalf("BlockedError.Code = %q, want %q (%s)", be.Code, code, be.Message)
	}
	if !errors.Is(err, domain.ErrConflict) {
		t.Fatal("BlockedError does not wrap ErrConflict")
	}
	return be
}

func kinds(c *MergeChecks) []string {
	out := []string{}
	for _, b := range c.Blockers {
		out = append(out, b.Kind)
	}
	return out
}

func (e *env) checks(t *testing.T) *MergeChecks {
	t.Helper()
	c, err := e.svc.Checks(ctx, eve, e.tree, 1)
	if err != nil {
		t.Fatalf("Checks: %v", err)
	}
	return c
}

func (e *env) ciStatus(t *testing.T, snapshot, context, state string) {
	t.Helper()
	st := domain.CIStatus{TreeID: e.tree.ID, SnapshotID: snapshot, Context: context, State: state}
	if err := e.store.PutDoc(ctx, ci.Collection, domain.DocKey(e.tree.ID, snapshot, context), st); err != nil {
		t.Fatal(err)
	}
}

func (e *env) todo(t *testing.T, number int, branch, status string, blocking bool) {
	t.Helper()
	td := domain.Todo{ID: fmt.Sprint(number), TreeID: e.tree.ID, Number: number, TargetBranch: branch, Status: status, Blocking: blocking, Title: "t"}
	if err := e.store.PutDoc(ctx, TodoCollection, domain.DocKey(e.tree.ID, fmt.Sprintf("%010d", number)), td); err != nil {
		t.Fatal(err)
	}
}

func TestStaleApprovals(t *testing.T) {
	e := newEnv(t)
	e.protect(domain.BranchProtection{Pattern: "main", RequiredApprovals: 1})
	e.hist.parents["s6"] = []string{s3}
	e.open(t)
	if _, err := e.svc.Approve(ctx, bob, e.tree, 1); err != nil {
		t.Fatal(err)
	}
	if c := e.checks(t); !c.Mergeable || c.Approvals.Current != 1 || c.Approvals.Required != 1 || c.Approvals.Stale != 0 {
		t.Fatalf("checks before push = %+v", c)
	}
	// The source moves: the approval of s3 no longer counts.
	e.store.setBranch(e.tree.ID, "feature", "s6")
	c := e.checks(t)
	if c.Mergeable || c.Approvals.Current != 0 || c.Approvals.Stale != 1 || !slices.Equal(kinds(c), []string{BlockerApprovals}) {
		t.Fatalf("checks after push = %+v", c)
	}
	if !strings.Contains(c.Blockers[0].Message, "1 stale") {
		t.Fatalf("blocker = %+v", c.Blockers[0])
	}
	_, err := e.svc.Merge(ctx, carol, e.tree, 1)
	if pd := pushDenied(t, err, "review_required"); pd.Reason != "0 of 1 approvals" {
		t.Fatalf("reason = %q", pd.Reason)
	}
	// Re-approving records the new tip and replaces the old record.
	mr, err := e.svc.Approve(ctx, bob, e.tree, 1)
	if err != nil {
		t.Fatal(err)
	}
	if len(mr.Approvals) != 1 || mr.Approvals[0].SnapshotID != "s6" {
		t.Fatalf("approvals = %+v", mr.Approvals)
	}
	if mr, err := e.svc.Merge(ctx, carol, e.tree, 1); err != nil || mr.MergedSnapshot != "s6" {
		t.Fatalf("Merge = %+v, %v", mr, err)
	}
}

func TestLegacyApprovalCounts(t *testing.T) {
	e := newEnv(t)
	e.protect(domain.BranchProtection{Pattern: "main", RequiredApprovals: 1})
	e.open(t)
	mr := e.store.mr(t, e.tree.ID, 1)
	mr.Approvals = []domain.Approval{{AccountID: bob.AccountID, Username: "bob"}} // pre-v0.5 record
	if err := e.store.UpdateMergeRequest(ctx, mr); err != nil {
		t.Fatal(err)
	}
	if c := e.checks(t); !c.Mergeable || c.Approvals.Current != 1 {
		t.Fatalf("checks = %+v", c)
	}
	// Approving again is a no-op: the legacy approval is current.
	before := len(e.bus.events)
	if _, err := e.svc.Approve(ctx, bob, e.tree, 1); err != nil || len(e.bus.events) != before {
		t.Fatalf("re-approve: %v, events %v", err, e.bus.types())
	}
}

func TestReviewDecisions(t *testing.T) {
	t.Run("errors", func(t *testing.T) {
		tests := []struct {
			name     string
			p        *domain.Principal
			decision string
			body     string
			wantErr  error
		}{
			{"unknown decision", bob, "maybe", "", domain.ErrInvalid},
			{"viewer requests changes", eve, domain.ReviewChangesRequested, "", domain.ErrForbidden},
			{"author requests changes", alice, domain.ReviewChangesRequested, "", domain.ErrForbidden},
			{"comment without body", eve, domain.ReviewCommented, "  ", domain.ErrInvalid},
			{"anonymous comment", nil, domain.ReviewCommented, "hi", domain.ErrUnauthenticated},
			{"body too long", bob, domain.ReviewApproved, strings.Repeat("x", MaxBodyLen+1), domain.ErrInvalid},
		}
		for _, tt := range tests {
			t.Run(tt.name, func(t *testing.T) {
				e := newEnv(t)
				e.open(t)
				if _, err := e.svc.Review(ctx, tt.p, e.tree, 1, tt.decision, tt.body); !errors.Is(err, tt.wantErr) {
					t.Fatalf("err = %v, want %v", err, tt.wantErr)
				}
			})
		}
	})

	t.Run("changes requested blocks until the reviewer approves", func(t *testing.T) {
		e := newEnv(t)
		e.open(t)
		mr, err := e.svc.Review(ctx, bob, e.tree, 1, domain.ReviewChangesRequested, "please fix")
		if err != nil {
			t.Fatal(err)
		}
		want := domain.Approval{AccountID: bob.AccountID, Username: "bob", At: fixedNow, SnapshotID: s3, Decision: domain.ReviewChangesRequested, Body: "please fix"}
		if !reflect.DeepEqual(mr.Approvals, []domain.Approval{want}) {
			t.Fatalf("approvals = %+v", mr.Approvals)
		}
		c := e.checks(t)
		if c.Mergeable || !slices.Equal(kinds(c), []string{BlockerChangesRequested}) || !slices.Equal(c.Approvals.ChangesRequested, []string{"bob"}) {
			t.Fatalf("checks = %+v", c)
		}
		_, err = e.svc.Merge(ctx, carol, e.tree, 1)
		if be := blocked(t, err, CodeMergeBlocked); !strings.Contains(be.Message, "changes requested by bob") {
			t.Fatalf("message = %q", be.Message)
		}
		// Another reviewer's approval does not lift bob's request.
		if _, err := e.svc.Approve(ctx, carol, e.tree, 1); err != nil {
			t.Fatal(err)
		}
		if _, err := e.svc.Merge(ctx, carol, e.tree, 1); err == nil {
			t.Fatal("merged with an open change request")
		}
		if _, err := e.svc.Approve(ctx, bob, e.tree, 1); err != nil {
			t.Fatal(err)
		}
		if mr := e.store.mr(t, e.tree.ID, 1); len(mr.Approvals) != 2 {
			t.Fatalf("approvals = %+v", mr.Approvals)
		}
		if _, err := e.svc.Merge(ctx, carol, e.tree, 1); err != nil {
			t.Fatal(err)
		}
		if got := e.bus.types(); !slices.Contains(got, domain.EventMRReviewed) {
			t.Fatalf("events = %v", got)
		}
		if got := e.audit.actions(); !slices.Contains(got, "mr.review") {
			t.Fatalf("audit = %v", got)
		}
	})

	t.Run("commented review adds a comment", func(t *testing.T) {
		e := newEnv(t)
		e.open(t)
		mr, err := e.svc.Review(ctx, eve, e.tree, 1, domain.ReviewCommented, "looks interesting")
		if err != nil {
			t.Fatal(err)
		}
		if len(mr.Approvals) != 0 {
			t.Fatalf("approvals = %+v", mr.Approvals)
		}
		cs, err := e.svc.Comments(ctx, eve, e.tree, 1)
		if err != nil || len(cs) != 1 || cs[0].Body != "looks interesting" || cs[0].Author != "eve" || cs[0].SnapshotID != s3 {
			t.Fatalf("comments = %+v, %v", cs, err)
		}
	})
}

func TestMergeRequiresCI(t *testing.T) {
	t.Run("named checks", func(t *testing.T) {
		e := newEnv(t)
		e.protect(domain.BranchProtection{Pattern: "main", RequireCIPass: true, RequiredChecks: []string{"test", "build"}})
		e.open(t)
		c := e.checks(t)
		if c.Mergeable || !slices.Equal(c.CI.Missing, []string{"build", "test"}) || !slices.Equal(c.CI.Required, []string{"build", "test"}) || c.CI.State != "" {
			t.Fatalf("checks = %+v", c)
		}
		e.ciStatus(t, s3, "build", domain.CISuccess)
		e.ciStatus(t, s3, "test", domain.CIPending)
		e.ciStatus(t, s3, "lint", domain.CIFailure) // not required
		e.ciStatus(t, s2, "test", domain.CISuccess) // other snapshot
		c = e.checks(t)
		if c.Mergeable || len(c.CI.Missing) != 0 || c.CI.State != domain.CIFailure || !slices.Equal(kinds(c), []string{BlockerCI}) {
			t.Fatalf("checks = %+v", c)
		}
		_, err := e.svc.Merge(ctx, carol, e.tree, 1)
		if be := blocked(t, err, CodeMergeBlocked); !strings.Contains(be.Message, "test is pending") {
			t.Fatalf("message = %q", be.Message)
		}
		e.ciStatus(t, s3, "test", domain.CISuccess)
		if c := e.checks(t); !c.Mergeable {
			t.Fatalf("checks = %+v", c)
		}
		if _, err := e.svc.Merge(ctx, carol, e.tree, 1); err != nil {
			t.Fatal(err)
		}
	})

	t.Run("every reported check", func(t *testing.T) {
		e := newEnv(t)
		e.protect(domain.BranchProtection{Pattern: "main", RequireCIPass: true})
		e.open(t)
		if c := e.checks(t); c.Mergeable || !strings.Contains(c.Blockers[0].Message, "no CI checks") {
			t.Fatalf("checks = %+v", c)
		}
		e.ciStatus(t, s3, "a", domain.CISuccess)
		e.ciStatus(t, s3, "b", domain.CIError)
		if c := e.checks(t); c.Mergeable {
			t.Fatalf("checks = %+v", c)
		}
		e.ciStatus(t, s3, "b", domain.CISuccess)
		if c := e.checks(t); !c.Mergeable || c.CI.State != domain.CISuccess {
			t.Fatalf("checks = %+v", c)
		}
	})

	t.Run("not required", func(t *testing.T) {
		e := newEnv(t)
		e.protect(domain.BranchProtection{Pattern: "main", RequiredChecks: []string{"build"}}) // without RequireCIPass
		e.open(t)
		e.ciStatus(t, s3, "build", domain.CIFailure)
		if c := e.checks(t); !c.Mergeable || c.CI.State != domain.CIFailure || len(c.CI.Required) != 0 {
			t.Fatalf("checks = %+v", c)
		}
	})
}

func TestMergeBlockedByTodos(t *testing.T) {
	e := newEnv(t)
	e.open(t)
	e.todo(t, 1, "feature", domain.TodoOpen, true)
	e.todo(t, 2, "feature", domain.TodoInProgress, true)
	e.todo(t, 3, "feature", domain.TodoDone, true)
	e.todo(t, 4, "feature", domain.TodoOpen, false)
	e.todo(t, 5, "other", domain.TodoOpen, true)
	e.todo(t, 6, "", domain.TodoOpen, true) // default branch (main)
	if c := e.checks(t); !c.Mergeable || c.Todos.BlockingOpen != 2 {
		t.Fatalf("without BlockOnTodos: %+v", c)
	}
	e.protect(domain.BranchProtection{Pattern: "main", BlockOnTodos: true})
	c := e.checks(t)
	if c.Mergeable || !slices.Equal(kinds(c), []string{BlockerTodos}) || !strings.Contains(c.Blockers[0].Message, "#1, #2") {
		t.Fatalf("checks = %+v", c)
	}
	_, err := e.svc.Merge(ctx, carol, e.tree, 1)
	blocked(t, err, CodeMergeBlocked)
}

func TestRequireLinear(t *testing.T) {
	t.Run("merge", func(t *testing.T) {
		e := newEnv(t)
		e.hist.parents["s7"] = []string{s3, s5} // merge of feature and other
		e.protect(domain.BranchProtection{Pattern: "main", RequireLinear: true})
		e.open(t)
		if c := e.checks(t); !c.Mergeable {
			t.Fatalf("linear source: %+v", c)
		}
		e.store.setBranch(e.tree.ID, "feature", "s7")
		c := e.checks(t)
		if c.Mergeable || !slices.Equal(kinds(c), []string{BlockerLinear}) {
			t.Fatalf("checks = %+v", c)
		}
		_, err := e.svc.Merge(ctx, carol, e.tree, 1)
		blocked(t, err, CodeMergeBlocked)
	})

	t.Run("direct push", func(t *testing.T) {
		e := newEnv(t)
		merge := &domain.Snapshot{ID: "m1", Parents: []string{s2, s3}}
		linear := &domain.Snapshot{ID: "l1", Parents: []string{s2}}
		plan := &domain.PushPlan{Branch: "main", NewSnapshots: []*domain.Snapshot{linear, merge}}
		if err := e.svc.CheckPushPlan(ctx, bob, e.tree, plan); err != nil {
			t.Fatalf("unprotected: %v", err)
		}
		e.protect(domain.BranchProtection{Pattern: "main", RequireLinear: true})
		pd := pushDenied(t, e.svc.CheckPushPlan(ctx, bob, e.tree, plan), "branch_protection")
		if !strings.Contains(pd.Reason, "linear history") {
			t.Fatalf("reason = %q", pd.Reason)
		}
		if err := e.svc.CheckPushPlan(ctx, bob, e.tree, &domain.PushPlan{Branch: "main", NewSnapshots: []*domain.Snapshot{linear}}); err != nil {
			t.Fatalf("linear push: %v", err)
		}
		if err := e.svc.CheckPushPlan(ctx, bob, e.tree, &domain.PushPlan{Branch: "feature", NewSnapshots: []*domain.Snapshot{merge}}); err != nil {
			t.Fatalf("other branch: %v", err)
		}
	})
}

func TestChecksReportsEverything(t *testing.T) {
	e := newEnv(t)
	e.protect(domain.BranchProtection{Pattern: "main", RequiredApprovals: 2, RequireCIPass: true, RequiredChecks: []string{"build"}, BlockOnTodos: true})
	e.open(t)
	e.todo(t, 1, "feature", domain.TodoOpen, true)
	e.store.setBranch(e.tree.ID, "main", s4) // diverged
	if _, err := e.svc.Review(ctx, bob, e.tree, 1, domain.ReviewChangesRequested, ""); err != nil {
		t.Fatal(err)
	}
	c := e.checks(t)
	want := []string{BlockerApprovals, BlockerChangesRequested, BlockerCI, BlockerTodos, BlockerDiverged}
	if c.Mergeable || !slices.Equal(kinds(c), want) {
		t.Fatalf("blockers = %v, want %v", kinds(c), want)
	}
	// Approvals are reported first (403 review_required).
	_, err := e.svc.Merge(ctx, carol, e.tree, 1)
	pushDenied(t, err, "review_required")
	// Closed merge requests report their state.
	if _, err := e.svc.Close(ctx, alice, e.tree, 1); err != nil {
		t.Fatal(err)
	}
	if c := e.checks(t); c.Mergeable || kinds(c)[0] != BlockerState {
		t.Fatalf("closed: %+v", c)
	}
	if _, err := e.svc.Checks(ctx, nil, e.tree, 1); !errors.Is(err, domain.ErrUnauthenticated) {
		t.Fatalf("anonymous checks err = %v", err)
	}
	if _, err := e.svc.Checks(ctx, eve, e.tree, 42); !errors.Is(err, domain.ErrNotFound) {
		t.Fatalf("missing MR err = %v", err)
	}
}

func TestComments(t *testing.T) {
	e := newEnv(t)
	e.authz.grants["root"] = append(slices.Clone(maintainer), domain.ActTreeAdmin)
	e.open(t)

	if _, err := e.svc.AddComment(ctx, eve, e.tree, 1, CommentInput{Body: ""}); !errors.Is(err, domain.ErrInvalid) {
		t.Fatalf("empty body err = %v", err)
	}
	if _, err := e.svc.AddComment(ctx, eve, e.tree, 1, CommentInput{Body: "x", Line: 3}); !errors.Is(err, domain.ErrInvalid) {
		t.Fatalf("line without path err = %v", err)
	}
	if _, err := e.svc.AddComment(ctx, eve, e.tree, 9, CommentInput{Body: "x"}); !errors.Is(err, domain.ErrNotFound) {
		t.Fatalf("missing MR err = %v", err)
	}
	if _, err := e.svc.AddComment(ctx, nil, e.tree, 1, CommentInput{Body: "x"}); !errors.Is(err, domain.ErrUnauthenticated) {
		t.Fatalf("anonymous err = %v", err)
	}

	var ids []string
	for i := range 11 {
		c, err := e.svc.AddComment(ctx, eve, e.tree, 1, CommentInput{Body: fmt.Sprintf("c%d", i), Path: "a.txt", Line: i + 1})
		if err != nil {
			t.Fatal(err)
		}
		ids = append(ids, c.ID)
	}
	if ids[0] != "1" || ids[10] != "11" {
		t.Fatalf("ids = %v", ids)
	}
	cs, err := e.svc.Comments(ctx, eve, e.tree, 1)
	if err != nil || len(cs) != 11 || cs[10].Body != "c10" || cs[9].Body != "c9" || cs[0].SnapshotID != s3 || cs[0].Path != "a.txt" {
		t.Fatalf("comments = %+v, %v", cs, err)
	}

	body := "edited"
	resolved := true
	if _, err := e.svc.UpdateComment(ctx, bob, e.tree, 1, "1", CommentPatch{Body: &body}); !errors.Is(err, domain.ErrForbidden) {
		t.Fatalf("other user edit err = %v", err)
	}
	c, err := e.svc.UpdateComment(ctx, eve, e.tree, 1, "1", CommentPatch{Body: &body})
	if err != nil || c.Body != "edited" || c.Resolved {
		t.Fatalf("author edit = %+v, %v", c, err)
	}
	c, err = e.svc.UpdateComment(ctx, root, e.tree, 1, "1", CommentPatch{Resolved: &resolved})
	if err != nil || !c.Resolved || c.Body != "edited" {
		t.Fatalf("admin resolve = %+v, %v", c, err)
	}
	if _, err := e.svc.UpdateComment(ctx, eve, e.tree, 1, "1", CommentPatch{}); !errors.Is(err, domain.ErrInvalid) {
		t.Fatalf("empty patch err = %v", err)
	}
	if _, err := e.svc.UpdateComment(ctx, eve, e.tree, 1, "99", CommentPatch{Body: &body}); !errors.Is(err, domain.ErrNotFound) {
		t.Fatalf("missing comment err = %v", err)
	}
	if err := e.svc.DeleteComment(ctx, bob, e.tree, 1, "2"); !errors.Is(err, domain.ErrForbidden) {
		t.Fatalf("other user delete err = %v", err)
	}
	if err := e.svc.DeleteComment(ctx, eve, e.tree, 1, "2"); err != nil {
		t.Fatal(err)
	}
	if err := e.svc.DeleteComment(ctx, root, e.tree, 1, "3"); err != nil {
		t.Fatal(err)
	}
	if cs, _ := e.svc.Comments(ctx, eve, e.tree, 1); len(cs) != 9 {
		t.Fatalf("after delete: %d comments", len(cs))
	}
	if got := e.bus.types(); slices.Index(got, domain.EventMRCommented) < 0 {
		t.Fatalf("events = %v", got)
	}
}

// groupEnv has a second tree (tree-2) with its own feature→main MR !1.
func groupEnv(t *testing.T) (*env, *domain.Tree) {
	t.Helper()
	e := newEnv(t)
	t2 := &domain.Tree{ID: "tree-2", TenantID: e.tree.TenantID, Name: "api", DefaultBranch: "main"}
	if err := e.store.CreateTree(ctx, e.tree); err != nil {
		t.Fatal(err)
	}
	if err := e.store.CreateTree(ctx, t2); err != nil {
		t.Fatal(err)
	}
	e.store.setBranch(t2.ID, "main", s2)
	e.store.setBranch(t2.ID, "feature", s5)
	e.open(t)
	if _, err := e.svc.Open(ctx, alice, t2, "API side", "", "feature", "main"); err != nil {
		t.Fatal(err)
	}
	return e, t2
}

func TestLinkedGroups(t *testing.T) {
	t.Run("create errors", func(t *testing.T) {
		e, t2 := groupEnv(t)
		two := []GroupMember{{e.tree, 1}, {t2, 1}}
		tests := []struct {
			name    string
			p       *domain.Principal
			gname   string
			members []GroupMember
			wantErr error
		}{
			{"no name", carol, " ", two, domain.ErrInvalid},
			{"one member", carol, "g", two[:1], domain.ErrInvalid},
			{"duplicate", carol, "g", []GroupMember{{e.tree, 1}, {e.tree, 1}}, domain.ErrInvalid},
			{"developer", bob, "g", two, domain.ErrForbidden},
			{"missing MR", carol, "g", []GroupMember{{e.tree, 1}, {t2, 7}}, domain.ErrNotFound},
		}
		for _, tt := range tests {
			t.Run(tt.name, func(t *testing.T) {
				if _, err := e.svc.CreateGroup(ctx, tt.p, e.tree.TenantID, tt.gname, tt.members); !errors.Is(err, tt.wantErr) {
					t.Fatalf("err = %v, want %v", err, tt.wantErr)
				}
			})
		}
		if mr := e.store.mr(t, e.tree.ID, 1); mr.LinkedGroup != "" {
			t.Fatalf("failed create linked the MR: %+v", mr)
		}
	})

	t.Run("atomic merge", func(t *testing.T) {
		e, t2 := groupEnv(t)
		g, err := e.svc.CreateGroup(ctx, carol, e.tree.TenantID, "release 1", []GroupMember{{e.tree, 1}, {t2, 1}})
		if err != nil {
			t.Fatal(err)
		}
		if g.State != domain.MRStateOpen || len(g.Members) != 2 || g.CreatedBy != "carol" {
			t.Fatalf("group = %+v", g)
		}
		if mr := e.store.mr(t, t2.ID, 1); mr.LinkedGroup != g.ID {
			t.Fatalf("member not linked: %+v", mr)
		}
		if _, err := e.svc.CreateGroup(ctx, carol, e.tree.TenantID, "again", []GroupMember{{e.tree, 1}, {t2, 1}}); !errors.Is(err, domain.ErrConflict) {
			t.Fatalf("second group err = %v", err)
		}
		// Merging a member alone is refused.
		_, err = e.svc.Merge(ctx, carol, e.tree, 1)
		blocked(t, err, CodeLinkedGroup)
		if c := e.checks(t); c.Mergeable || kinds(c)[0] != BlockerLinkedGroup {
			t.Fatalf("checks = %+v", c)
		}
		gs, err := e.svc.ListGroups(ctx, e.tree.TenantID)
		if err != nil || len(gs) != 1 || gs[0].ID != g.ID {
			t.Fatalf("groups = %+v, %v", gs, err)
		}

		if _, err := e.svc.MergeGroup(ctx, bob, e.tree.TenantID, g.ID); !errors.Is(err, domain.ErrForbidden) {
			t.Fatalf("developer merge err = %v", err)
		}
		merged, err := e.svc.MergeGroup(ctx, carol, e.tree.TenantID, g.ID)
		if err != nil {
			t.Fatal(err)
		}
		if merged.State != domain.MRStateMerged || merged.MergedAt == nil {
			t.Fatalf("group = %+v", merged)
		}
		if tip, _ := e.store.tip(e.tree.ID, "main"); tip != s3 {
			t.Fatalf("tree-1 main = %s", tip)
		}
		if tip, _ := e.store.tip(t2.ID, "main"); tip != s5 {
			t.Fatalf("tree-2 main = %s", tip)
		}
		for _, tr := range []*domain.Tree{e.tree, t2} {
			if mr := e.store.mr(t, tr.ID, 1); mr.State != domain.MRStateMerged {
				t.Fatalf("%s MR = %+v", tr.ID, mr)
			}
		}
		if got := e.bus.types(); got[len(got)-1] != domain.EventGroupMerged {
			t.Fatalf("events = %v", got)
		}
		stored, err := e.svc.GetGroup(ctx, e.tree.TenantID, g.ID)
		if err != nil || stored.State != domain.MRStateMerged {
			t.Fatalf("stored = %+v, %v", stored, err)
		}
		if _, err := e.svc.MergeGroup(ctx, carol, e.tree.TenantID, g.ID); !errors.Is(err, domain.ErrConflict) {
			t.Fatalf("second merge err = %v", err)
		}
	})

	t.Run("blocked member stops everything", func(t *testing.T) {
		e, t2 := groupEnv(t)
		e.store.PutProtection(ctx, &domain.BranchProtection{TreeID: t2.ID, Pattern: "main", RequiredApprovals: 1})
		g, err := e.svc.CreateGroup(ctx, carol, e.tree.TenantID, "g", []GroupMember{{e.tree, 1}, {t2, 1}})
		if err != nil {
			t.Fatal(err)
		}
		_, err = e.svc.MergeGroup(ctx, carol, e.tree.TenantID, g.ID)
		if be := blocked(t, err, CodeMergeBlocked); !strings.Contains(be.Message, "api!1") {
			t.Fatalf("message = %q", be.Message)
		}
		if tip, _ := e.store.tip(e.tree.ID, "main"); tip != s2 {
			t.Fatalf("tree-1 main moved to %s", tip)
		}
	})

	t.Run("rollback", func(t *testing.T) {
		e, t2 := groupEnv(t)
		g, err := e.svc.CreateGroup(ctx, carol, e.tree.TenantID, "g", []GroupMember{{e.tree, 1}, {t2, 1}})
		if err != nil {
			t.Fatal(err)
		}
		calls := 0
		e.store.beforeCAS = func() {
			calls++
			if calls == 2 { // tree-2's target moves under the merge
				e.store.setBranch(t2.ID, "main", s4)
			}
		}
		if _, err := e.svc.MergeGroup(ctx, carol, e.tree.TenantID, g.ID); !errors.Is(err, domain.ErrConflict) {
			t.Fatalf("err = %v, want ErrConflict", err)
		}
		e.store.beforeCAS = nil
		if tip, _ := e.store.tip(e.tree.ID, "main"); tip != s2 {
			t.Fatalf("tree-1 main = %s, want rolled back to %s", tip, s2)
		}
		for _, tr := range []*domain.Tree{e.tree, t2} {
			if mr := e.store.mr(t, tr.ID, 1); mr.State != domain.MRStateOpen {
				t.Fatalf("%s MR = %s", tr.ID, mr.State)
			}
		}
		if g, _ := e.svc.GetGroup(ctx, e.tree.TenantID, g.ID); g.State != domain.MRStateOpen {
			t.Fatalf("group = %+v", g)
		}
	})

	t.Run("close", func(t *testing.T) {
		e, t2 := groupEnv(t)
		g, err := e.svc.CreateGroup(ctx, carol, e.tree.TenantID, "g", []GroupMember{{e.tree, 1}, {t2, 1}})
		if err != nil {
			t.Fatal(err)
		}
		if _, err := e.svc.CloseGroup(ctx, bob, e.tree.TenantID, g.ID); !errors.Is(err, domain.ErrForbidden) {
			t.Fatalf("developer close err = %v", err)
		}
		closed, err := e.svc.CloseGroup(ctx, carol, e.tree.TenantID, g.ID)
		if err != nil || closed.State != domain.MRStateClosed {
			t.Fatalf("close = %+v, %v", closed, err)
		}
		if mr := e.store.mr(t, e.tree.ID, 1); mr.LinkedGroup != "" {
			t.Fatalf("still linked: %+v", mr)
		}
		if _, err := e.svc.Merge(ctx, carol, e.tree, 1); err != nil {
			t.Fatalf("merge after close: %v", err)
		}
		if _, err := e.svc.GetGroup(ctx, e.tree.TenantID, "nope"); !errors.Is(err, domain.ErrNotFound) {
			t.Fatalf("missing group err = %v", err)
		}
	})
}

func TestTagProtections(t *testing.T) {
	e := newEnv(t)
	e.authz.grants["root"] = append(slices.Clone(maintainer), domain.ActTreeAdmin)
	if _, err := e.svc.PutTagProtection(ctx, carol, e.tree, domain.TagProtection{Pattern: "v*"}); !errors.Is(err, domain.ErrForbidden) {
		t.Fatalf("maintainer put err = %v", err)
	}
	if _, err := e.svc.PutTagProtection(ctx, root, e.tree, domain.TagProtection{Pattern: "a//b"}); !errors.Is(err, domain.ErrInvalid) {
		t.Fatalf("bad pattern err = %v", err)
	}
	tp, err := e.svc.PutTagProtection(ctx, root, e.tree, domain.TagProtection{Pattern: "v*", NoMove: true, NoDelete: true, AllowedTaggers: []string{"carol", " ", "bob", "carol"}})
	if err != nil {
		t.Fatal(err)
	}
	if tp.TreeID != e.tree.ID || !slices.Equal(tp.AllowedTaggers, []string{"bob", "carol"}) {
		t.Fatalf("protection = %+v", tp)
	}
	if _, err := e.svc.PutTagProtection(ctx, root, e.tree, domain.TagProtection{Pattern: "rel/**", NoDelete: true}); err != nil {
		t.Fatal(err)
	}
	list, err := e.svc.TagProtections(ctx, e.tree.ID)
	if err != nil || len(list) != 2 || list[0].Pattern != "rel/**" || list[1].Pattern != "v*" {
		t.Fatalf("list = %+v, %v", list, err)
	}

	tests := []struct {
		name string
		p    *domain.Principal
		tag  string
		op   TagOp
		deny string
	}{
		{"allowed tagger creates", bob, "v1", TagCreate, ""},
		{"other user creates", alice, "v1", TagCreate, "may not create"},
		{"anonymous", nil, "v1", TagCreate, "may not create"},
		{"move protected", bob, "v1", TagMove, "cannot be moved"},
		{"delete protected", carol, "v1", TagDelete, "cannot be deleted"},
		{"unmatched tag", alice, "x1", TagDelete, ""},
		{"globstar delete", alice, "rel/1/2", TagDelete, "cannot be deleted"},
		{"globstar move allowed", alice, "rel/1", TagMove, ""},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			err := e.svc.CheckTag(ctx, tt.p, e.tree, tt.tag, tt.op)
			if tt.deny == "" {
				if err != nil {
					t.Fatalf("err = %v", err)
				}
				return
			}
			if pd := pushDenied(t, err, "tag_protection"); !strings.Contains(pd.Reason, tt.deny) {
				t.Fatalf("reason = %q, want %q", pd.Reason, tt.deny)
			}
		})
	}

	if err := e.svc.DeleteTagProtection(ctx, root, e.tree, "v*"); err != nil {
		t.Fatal(err)
	}
	if err := e.svc.DeleteTagProtection(ctx, root, e.tree, "v*"); !errors.Is(err, domain.ErrNotFound) {
		t.Fatalf("second delete err = %v", err)
	}
	if err := e.svc.CheckTag(ctx, alice, e.tree, "v1", TagDelete); err != nil {
		t.Fatalf("after delete: %v", err)
	}
}

func TestTreeDeletedCleansDocuments(t *testing.T) {
	e := newEnv(t)
	bus := events.NewBus()
	e.svc.bus = bus
	cctx, cancel := context.WithCancel(ctx)
	defer cancel()
	e.svc.Start(cctx)
	e.authz.grants["root"] = append(slices.Clone(maintainer), domain.ActTreeAdmin)
	e.open(t)
	if _, err := e.svc.AddComment(ctx, eve, e.tree, 1, CommentInput{Body: "x"}); err != nil {
		t.Fatal(err)
	}
	if _, err := e.svc.PutTagProtection(ctx, root, e.tree, domain.TagProtection{Pattern: "v*"}); err != nil {
		t.Fatal(err)
	}
	// Another tree's documents stay.
	other := &domain.Tree{ID: "tree-1x", TenantID: e.tree.TenantID}
	if _, err := e.svc.PutTagProtection(ctx, root, other, domain.TagProtection{Pattern: "v*"}); err != nil {
		t.Fatal(err)
	}
	bus.Publish(domain.Event{Type: domain.EventTreeDeleted, TenantID: e.tree.TenantID, TreeID: e.tree.ID})
	deadline := time.Now().Add(5 * time.Second)
	for {
		c, _ := meta.ListDocsT[domain.ReviewComment](ctx, e.store, CommentCollection, "")
		tp, _ := e.svc.TagProtections(ctx, e.tree.ID)
		if len(c) == 0 && len(tp) == 0 {
			break
		}
		if time.Now().After(deadline) {
			t.Fatalf("documents not removed: %d comments, %d protections", len(c), len(tp))
		}
		time.Sleep(10 * time.Millisecond)
	}
	if tp, _ := e.svc.TagProtections(ctx, other.ID); len(tp) != 1 {
		t.Fatalf("other tree's protections = %+v", tp)
	}
}
