package review

// Tests of v0.8 rebase merges and delete_source_branch (§16.9, rebase.go).

import (
	"context"
	"errors"
	"reflect"
	"slices"
	"strings"
	"testing"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// rebaseEnv: base ← main (edits a.txt line 1) and base ← f1 (edits a.txt
// line 5) ← f2 (adds c.txt) on feature; opens feature → main.
func rebaseEnv(t *testing.T) (e *menv, base, mainTip, f1, f2 string) {
	t.Helper()
	e = newMergeEnv(t)
	base = e.commit(t, map[string]string{"a.txt": fiveLines})
	mainTip = e.commit(t, map[string]string{"a.txt": "ONE\n2\n3\n4\n5\n"}, base)
	f1 = e.commit(t, map[string]string{"a.txt": "1\n2\n3\n4\nFIVE\n"}, base)
	f2 = e.commit(t, map[string]string{"a.txt": "1\n2\n3\n4\nFIVE\n", "c.txt": "new\n"}, f1)
	e.branch(t, "main", mainTip)
	e.branch(t, "feature", f2)
	e.open(t, "feature")
	return e, base, mainTip, f1, f2
}

func TestRebaseMerge(t *testing.T) {
	e, _, mainTip, f1, f2 := rebaseEnv(t)
	ctx := context.Background()
	mb, err := e.svc.Mergeability(ctx, eve, e.tree, 1)
	if err != nil || !slices.Contains(mb.Methods, MergeRebase) || mb.Default != domain.MergeCommit {
		t.Fatalf("mergeability = %+v, %v", mb, err)
	}
	mr, err := e.svc.MergeWith(ctx, carol, e.tree, 1, MergeOptions{Method: MergeRebase, Message: "ignored"})
	if err != nil {
		t.Fatal(err)
	}
	if mr.MergeMethod != MergeRebase || mr.State != domain.MRStateMerged || e.tip(t, "main") != mr.MergedSnapshot {
		t.Fatalf("merged = %+v", mr)
	}
	last, err := e.store.GetSnapshot(ctx, mr.MergedSnapshot)
	if err != nil {
		t.Fatal(err)
	}
	first, err := e.store.GetSnapshot(ctx, last.Parents[0])
	if err != nil {
		t.Fatal(err)
	}
	orig1, _ := e.store.GetSnapshot(ctx, f1)
	orig2, _ := e.store.GetSnapshot(ctx, f2)
	if !reflect.DeepEqual(first.Parents, []string{mainTip}) || first.Message != orig1.Message || !first.CreatedAt.Equal(orig1.CreatedAt) ||
		first.Author != orig1.Author || first.PushedBy != carol.AccountID || first.BranchName != "main" {
		t.Fatalf("first replayed = %+v", first)
	}
	if last.Message != orig2.Message || last.ID == f2 {
		t.Fatalf("last replayed = %+v", last)
	}
	if got := e.files(t, first.ID); !reflect.DeepEqual(got, map[string]string{"a.txt": "ONE\n2\n3\n4\nFIVE\n"}) {
		t.Fatalf("first files = %#v", got)
	}
	if got := e.files(t, last.ID); !reflect.DeepEqual(got, map[string]string{"a.txt": "ONE\n2\n3\n4\nFIVE\n", "c.txt": "new\n"}) {
		t.Fatalf("last files = %#v", got)
	}
	if !slices.Contains(e.bus.types(), domain.EventBranchPushed) {
		t.Fatalf("events = %v", e.bus.types())
	}
}

func TestRebaseFastForwardAndRestrictions(t *testing.T) {
	ctx := context.Background()
	t.Run("fast-forward", func(t *testing.T) {
		e := newMergeEnv(t)
		base := e.commit(t, map[string]string{"a.txt": "a\n"})
		feat := e.commit(t, map[string]string{"a.txt": "b\n"}, base)
		e.branch(t, "main", base)
		e.branch(t, "feature", feat)
		e.open(t, "feature")
		mr, err := e.svc.MergeWith(ctx, carol, e.tree, 1, MergeOptions{Method: MergeRebase})
		if err != nil || mr.MergedSnapshot != feat || e.tip(t, "main") != feat || mr.MergeMethod != MergeRebase {
			t.Fatalf("merged = %+v, %v", mr, err)
		}
	})
	t.Run("require linear allows rebase", func(t *testing.T) {
		e, _, _, _, _ := rebaseEnv(t)
		e.protect(t, domain.BranchProtection{Pattern: "main", RequireLinear: true, MergeMethods: []string{MergeRebase}})
		mb, err := e.svc.Mergeability(ctx, eve, e.tree, 1)
		if err != nil || !reflect.DeepEqual(mb.Methods, []string{MergeRebase}) || mb.Default != MergeRebase {
			t.Fatalf("mergeability = %+v, %v", mb, err)
		}
		if _, err := e.svc.Merge(ctx, carol, e.tree, 1); err != nil {
			t.Fatal(err)
		}
	})
	t.Run("require signed refuses rebase", func(t *testing.T) {
		e, _, _, _, _ := rebaseEnv(t)
		e.protect(t, domain.BranchProtection{Pattern: "main", RequireSigned: true})
		_, err := e.svc.MergeWith(ctx, carol, e.tree, 1, MergeOptions{Method: MergeRebase})
		var me *MethodError
		if !errors.As(err, &me) {
			t.Fatalf("signed: %v", err)
		}
	})
	t.Run("merge snapshots", func(t *testing.T) {
		e := newMergeEnv(t)
		base := e.commit(t, map[string]string{"a.txt": fiveLines})
		mainTip := e.commit(t, map[string]string{"a.txt": "ONE\n2\n3\n4\n5\n"}, base)
		side := e.commit(t, map[string]string{"a.txt": fiveLines, "s.txt": "s\n"}, base)
		f1 := e.commit(t, map[string]string{"a.txt": "1\n2\n3\n4\nFIVE\n"}, base)
		merge := e.commit(t, map[string]string{"a.txt": "1\n2\n3\n4\nFIVE\n", "s.txt": "s\n"}, f1, side)
		e.branch(t, "main", mainTip)
		e.branch(t, "feature", merge)
		e.open(t, "feature")
		mb, err := e.svc.Mergeability(ctx, eve, e.tree, 1)
		if err != nil || slices.Contains(mb.Methods, MergeRebase) || !strings.Contains(mb.Reasons[MergeRebase], "merge snapshot") {
			t.Fatalf("mergeability = %+v, %v", mb, err)
		}
		if _, err := e.svc.MergeWith(ctx, carol, e.tree, 1, MergeOptions{Method: MergeRebase}); !errors.Is(err, domain.ErrConflict) {
			t.Fatalf("rebase of merges: %v", err)
		}
	})
	t.Run("conflicts", func(t *testing.T) {
		e := newMergeEnv(t)
		base := e.commit(t, map[string]string{"a.txt": fiveLines})
		mainTip := e.commit(t, map[string]string{"a.txt": "1\n2\nMAIN\n4\n5\n"}, base)
		f1 := e.commit(t, map[string]string{"a.txt": "1\n2\nFEAT\n4\n5\n"}, base)
		e.branch(t, "main", mainTip)
		e.branch(t, "feature", f1)
		e.open(t, "feature")
		mb, err := e.svc.Mergeability(ctx, eve, e.tree, 1)
		if err != nil || len(mb.Methods) != 0 || !strings.Contains(mb.Reasons[MergeRebase], "cannot be rebased") {
			t.Fatalf("mergeability = %+v, %v", mb, err)
		}
		_, err = e.svc.MergeWith(ctx, carol, e.tree, 1, MergeOptions{Method: MergeRebase})
		var ce *ConflictError
		if !errors.As(err, &ce) || len(ce.Conflicts) != 1 || ce.Conflicts[0].Path != "a.txt" {
			t.Fatalf("rebase conflict: %v", err)
		}
		if e.tip(t, "main") != mainTip {
			t.Fatal("target moved")
		}
	})
	t.Run("already applied snapshots are dropped", func(t *testing.T) {
		e := newMergeEnv(t)
		base := e.commit(t, map[string]string{"a.txt": "a\n"})
		mainTip := e.commit(t, map[string]string{"a.txt": "a\n", "x.txt": "x\n"}, base)
		pick := e.commit(t, map[string]string{"a.txt": "a\n", "x.txt": "x\n"}, base) // same change as main
		f2 := e.commit(t, map[string]string{"a.txt": "a\n", "x.txt": "x\n", "y.txt": "y\n"}, pick)
		e.branch(t, "main", mainTip)
		e.branch(t, "feature", f2)
		e.open(t, "feature")
		mr, err := e.svc.MergeWith(ctx, carol, e.tree, 1, MergeOptions{Method: MergeRebase})
		if err != nil {
			t.Fatal(err)
		}
		tip, _ := e.store.GetSnapshot(ctx, mr.MergedSnapshot)
		if !reflect.DeepEqual(tip.Parents, []string{mainTip}) || tip.Message != "c04" {
			t.Fatalf("tip = %+v", tip)
		}
	})
	t.Run("quota", func(t *testing.T) {
		e, _, mainTip, _, _ := rebaseEnv(t)
		q := &quotaChecker{deny: true}
		e.svc.SetQuota(q)
		_, err := e.svc.MergeWith(ctx, carol, e.tree, 1, MergeOptions{Method: MergeRebase})
		var qe *QuotaError
		if !errors.As(err, &qe) || e.tip(t, "main") != mainTip {
			t.Fatalf("quota: %v", err)
		}
		if plan := q.plans[0]; len(plan.NewSnapshots) != 2 || len(plan.NewSnapshots[0].Parents) != 2 {
			t.Fatalf("plan = %+v", plan)
		}
	})
}

func TestMergeDeletesSourceBranch(t *testing.T) {
	ctx := context.Background()
	t.Run("deleted", func(t *testing.T) {
		e, _, _, _, _ := rebaseEnv(t)
		mr, err := e.svc.MergeWith(ctx, carol, e.tree, 1, MergeOptions{Method: MergeRebase, DeleteSourceBranch: true})
		if err != nil || mr.State != domain.MRStateMerged {
			t.Fatalf("merge: %+v, %v", mr, err)
		}
		if _, err := e.store.GetBranch(ctx, e.tree.ID, "feature"); !errors.Is(err, domain.ErrNotFound) {
			t.Fatalf("source branch still exists: %v", err)
		}
		if !slices.Contains(e.bus.types(), domain.EventBranchDeleted) || !slices.Contains(e.audit.actions(), "branch.delete") {
			t.Fatalf("events %v audit %v", e.bus.types(), e.audit.actions())
		}
		var details map[string]string
		for _, a := range e.audit.events {
			if a.Action == "mr.merge" {
				details = a.Details
			}
		}
		if details["source_branch_deleted"] != "true" || details["method"] != MergeRebase {
			t.Fatalf("mr.merge audit = %v", details)
		}
	})
	t.Run("refused before merging", func(t *testing.T) {
		e, _, mainTip, _, _ := rebaseEnv(t)
		e.protect(t, domain.BranchProtection{Pattern: "feature", NoDelete: true})
		_, err := e.svc.MergeWith(ctx, carol, e.tree, 1, MergeOptions{DeleteSourceBranch: true})
		var denied *domain.PushDenied
		if !errors.As(err, &denied) || e.tip(t, "main") != mainTip {
			t.Fatalf("protected source: %v", err)
		}
		if mr, _ := e.store.GetMergeRequest(ctx, e.tree.ID, 1); mr.State != domain.MRStateOpen {
			t.Fatal("the merge request must stay open")
		}
	})
	t.Run("used by another merge request", func(t *testing.T) {
		e, _, _, _, _ := rebaseEnv(t)
		e.branch(t, "release", e.tip(t, "main"))
		if _, err := e.svc.Open(ctx, alice, e.tree, "to release", "", "feature", "release"); err != nil {
			t.Fatal(err)
		}
		if _, err := e.svc.MergeWith(ctx, carol, e.tree, 1, MergeOptions{DeleteSourceBranch: true}); !errors.Is(err, domain.ErrConflict) || !strings.Contains(err.Error(), "!2") {
			t.Fatalf("shared source: %v", err)
		}
	})
	t.Run("needs branch:delete", func(t *testing.T) {
		e, _, _, _, _ := rebaseEnv(t)
		e.svc.authz = &fakeAuthz{grants: map[string][]domain.Action{"carol": {domain.ActTreeRead, domain.ActTreeWrite, domain.ActMRMerge}}}
		if _, err := e.svc.MergeWith(ctx, carol, e.tree, 1, MergeOptions{DeleteSourceBranch: true}); !errors.Is(err, domain.ErrForbidden) {
			t.Fatalf("no branch:delete: %v", err)
		}
	})
}
