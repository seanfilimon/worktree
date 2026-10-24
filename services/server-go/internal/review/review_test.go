package review

import (
	"context"
	"errors"
	"fmt"
	"reflect"
	"strings"
	"sync"
	"testing"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

var ctx = context.Background()

// pushDenied asserts err is a *domain.PushDenied of the given kind and
// returns it.
func pushDenied(t *testing.T, err error, kind string) *domain.PushDenied {
	t.Helper()
	var pd *domain.PushDenied
	if !errors.As(err, &pd) {
		t.Fatalf("error = %v (%T), want *domain.PushDenied", err, err)
	}
	if pd.Kind != kind {
		t.Fatalf("PushDenied.Kind = %q, want %q (reason %q)", pd.Kind, kind, pd.Reason)
	}
	return pd
}

func TestCheckPush(t *testing.T) {
	tests := []struct {
		name    string
		rules   []domain.BranchProtection
		p       *domain.Principal
		branch  string
		wantErr error  // errors.Is target
		kind    string // PushDenied kind
		reason  string
	}{
		{name: "unprotected", p: alice, branch: "main"},
		{name: "anonymous", p: nil, branch: "main", wantErr: domain.ErrUnauthenticated},
		{name: "no write access", p: eve, branch: "feature", wantErr: domain.ErrForbidden},
		{
			name: "authz checked before protection", p: eve, branch: "main",
			rules:   []domain.BranchProtection{{Pattern: "main", NoDirectPush: true}},
			wantErr: domain.ErrForbidden,
		},
		{
			name: "no direct push", p: alice, branch: "main",
			rules: []domain.BranchProtection{{Pattern: "main", NoDirectPush: true}},
			kind:  "branch_protection", reason: "branch 'main' is protected: changes must go through a merge request",
		},
		{
			name: "no direct push applies to server admins", p: root, branch: "main",
			rules: []domain.BranchProtection{{Pattern: "main", NoDirectPush: true}},
			kind:  "branch_protection", reason: "branch 'main' is protected: changes must go through a merge request",
		},
		{
			name: "glob rule", p: carol, branch: "release/1.0/rc",
			rules: []domain.BranchProtection{{Pattern: "release/**", NoDirectPush: true}},
			kind:  "branch_protection", reason: "branch 'release/1.0/rc' is protected: changes must go through a merge request",
		},
		{
			name: "other branch unaffected", p: alice, branch: "feature",
			rules: []domain.BranchProtection{{Pattern: "main", NoDirectPush: true}},
		},
		{
			name: "only approvals required", p: alice, branch: "main",
			rules: []domain.BranchProtection{{Pattern: "main", RequiredApprovals: 2, NoDelete: true}},
		},
		{
			name: "allowed pusher", p: bob, branch: "main",
			rules: []domain.BranchProtection{{Pattern: "main", AllowedPushers: []string{"bob"}}},
		},
		{
			name: "not an allowed pusher", p: alice, branch: "main",
			rules: []domain.BranchProtection{{Pattern: "main", AllowedPushers: []string{"bob"}}},
			kind:  "branch_protection", reason: "branch 'main' is protected: user 'alice' is not an allowed pusher",
		},
		{
			name: "allowed pushers apply to admins", p: root, branch: "main",
			rules: []domain.BranchProtection{{Pattern: "main", AllowedPushers: []string{"bob"}}},
			kind:  "branch_protection", reason: "branch 'main' is protected: user 'root' is not an allowed pusher",
		},
		{
			name: "must be in every list", p: alice, branch: "main",
			rules: []domain.BranchProtection{
				{Pattern: "*", AllowedPushers: []string{"alice", "bob"}},
				{Pattern: "main", AllowedPushers: []string{"bob"}},
			},
			kind: "branch_protection", reason: "branch 'main' is protected: user 'alice' is not an allowed pusher",
		},
		{
			name: "in every list", p: bob, branch: "main",
			rules: []domain.BranchProtection{
				{Pattern: "*", AllowedPushers: []string{"alice", "bob"}},
				{Pattern: "main", AllowedPushers: []string{"bob"}},
				{Pattern: "m*", RequiredApprovals: 1},
			},
		},
		{
			name: "allowed pusher still blocked by no direct push", p: bob, branch: "main",
			rules: []domain.BranchProtection{
				{Pattern: "main", AllowedPushers: []string{"bob"}},
				{Pattern: "ma*", NoDirectPush: true},
			},
			kind: "branch_protection", reason: "branch 'main' is protected: changes must go through a merge request",
		},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			e := newEnv(t)
			e.protect(tt.rules...)
			err := e.svc.CheckPush(ctx, tt.p, e.tree, tt.branch)
			switch {
			case tt.wantErr != nil:
				if !errors.Is(err, tt.wantErr) {
					t.Fatalf("err = %v, want %v", err, tt.wantErr)
				}
				if err != e.authz.last() {
					t.Fatalf("authorizer error not returned unchanged: %v", err)
				}
			case tt.kind != "":
				pd := pushDenied(t, err, tt.kind)
				if pd.Reason != tt.reason {
					t.Fatalf("reason = %q, want %q", pd.Reason, tt.reason)
				}
			case err != nil:
				t.Fatalf("unexpected error: %v", err)
			}
		})
	}
}

func TestCheckPushNilTree(t *testing.T) {
	e := newEnv(t)
	if err := e.svc.CheckPush(ctx, alice, nil, "main"); !errors.Is(err, domain.ErrInvalid) {
		t.Fatalf("err = %v, want ErrInvalid", err)
	}
}

func TestCheckDeleteBranch(t *testing.T) {
	tests := []struct {
		name    string
		rules   []domain.BranchProtection
		p       *domain.Principal
		branch  string
		wantErr error
		reason  string
	}{
		{name: "allowed", p: carol, branch: "feature"},
		{name: "anonymous", p: nil, branch: "feature", wantErr: domain.ErrUnauthenticated},
		{name: "developer lacks branch:delete", p: alice, branch: "feature", wantErr: domain.ErrForbidden},
		{name: "default branch", p: carol, branch: "main", reason: "branch 'main' is the default branch and cannot be deleted"},
		{name: "default branch even for admins", p: root, branch: "main", reason: "branch 'main' is the default branch and cannot be deleted"},
		{
			name: "no delete rule", p: root, branch: "release/1",
			rules:  []domain.BranchProtection{{Pattern: "release/*", NoDelete: true}},
			reason: "branch 'release/1' is protected and cannot be deleted",
		},
		{
			name: "protected but deletable", p: carol, branch: "release/1",
			rules: []domain.BranchProtection{{Pattern: "release/*", NoDirectPush: true, RequiredApprovals: 3}},
		},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			e := newEnv(t)
			e.protect(tt.rules...)
			err := e.svc.CheckDeleteBranch(ctx, tt.p, e.tree, tt.branch)
			switch {
			case tt.wantErr != nil:
				if !errors.Is(err, tt.wantErr) || err != e.authz.last() {
					t.Fatalf("err = %v, want authorizer's %v", err, tt.wantErr)
				}
			case tt.reason != "":
				if pd := pushDenied(t, err, "branch_protection"); pd.Reason != tt.reason {
					t.Fatalf("reason = %q, want %q", pd.Reason, tt.reason)
				}
			case err != nil:
				t.Fatalf("unexpected error: %v", err)
			}
		})
	}
}

func TestProtectionFor(t *testing.T) {
	e := newEnv(t)
	e.protect(
		domain.BranchProtection{Pattern: "main", RequiredApprovals: 1},
		domain.BranchProtection{Pattern: "**", RequiredApprovals: 3, NoDelete: true},
	)
	eff, err := e.svc.ProtectionFor(ctx, e.tree.ID, "main")
	if err != nil {
		t.Fatal(err)
	}
	if eff.RequiredApprovals != 3 || !eff.NoDelete || eff.NoDirectPush || len(eff.Patterns) != 2 {
		t.Fatalf("unexpected effective protection %+v", eff)
	}
}

func TestOpen(t *testing.T) {
	tests := []struct {
		name           string
		p              *domain.Principal
		title          string
		source, target string
		wantErr        error
		authzErr       bool
	}{
		{name: "anonymous", p: nil, title: "x", source: "feature", target: "main", wantErr: domain.ErrUnauthenticated, authzErr: true},
		{name: "viewer", p: eve, title: "x", source: "feature", target: "main", wantErr: domain.ErrForbidden, authzErr: true},
		{name: "empty title", p: alice, title: "  ", source: "feature", target: "main", wantErr: domain.ErrInvalid},
		{name: "missing source", p: alice, title: "x", source: "", target: "main", wantErr: domain.ErrInvalid},
		{name: "same branch", p: alice, title: "x", source: "main", target: "main", wantErr: domain.ErrInvalid},
		{name: "unknown source", p: alice, title: "x", source: "nope", target: "main", wantErr: domain.ErrNotFound},
		{name: "unknown target", p: alice, title: "x", source: "feature", target: "nope", wantErr: domain.ErrNotFound},
		{name: "empty source", p: alice, title: "x", source: "empty", target: "main", wantErr: domain.ErrInvalid},
		{name: "empty target", p: alice, title: "x", source: "feature", target: "empty", wantErr: domain.ErrInvalid},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			e := newEnv(t)
			mr, err := e.svc.Open(ctx, tt.p, e.tree, tt.title, "", tt.source, tt.target)
			if !errors.Is(err, tt.wantErr) || mr != nil {
				t.Fatalf("Open = %v, %v; want %v", mr, err, tt.wantErr)
			}
			if tt.authzErr && err != e.authz.last() {
				t.Fatalf("authorizer error not returned unchanged: %v", err)
			}
			if got := e.bus.types(); len(got) != 0 {
				t.Fatalf("events published on failure: %v", got)
			}
		})
	}

	t.Run("success", func(t *testing.T) {
		e := newEnv(t)
		mr, err := e.svc.Open(ctx, alice, e.tree, "  Add feature ", "Long description", " feature", "main ")
		if err != nil {
			t.Fatal(err)
		}
		want := domain.MergeRequest{
			ID: "mr-1", TreeID: e.tree.ID, Number: 1, Title: "Add feature", Description: "Long description",
			SourceBranch: "feature", TargetBranch: "main", AuthorID: alice.AccountID, Author: "alice",
			State: domain.MRStateOpen, Approvals: []domain.Approval{}, CreatedAt: fixedNow, UpdatedAt: fixedNow,
		}
		if !reflect.DeepEqual(*mr, want) {
			t.Fatalf("mr = %+v\nwant %+v", *mr, want)
		}
		if !reflect.DeepEqual(*e.store.mr(t, e.tree.ID, 1), want) {
			t.Fatal("stored merge request differs")
		}
		if len(e.bus.events) != 1 {
			t.Fatalf("events = %v", e.bus.types())
		}
		ev := e.bus.events[0]
		if ev.Type != domain.EventMROpened || ev.TenantID != "ten-acme" || ev.TreeID != e.tree.ID || ev.Actor != "alice" {
			t.Fatalf("event = %+v", ev)
		}
		if p, ok := ev.Payload.(domain.MergeRequest); !ok || p.Number != 1 {
			t.Fatalf("payload = %#v", ev.Payload)
		}
		if len(e.audit.events) != 1 {
			t.Fatalf("audit = %v", e.audit.actions())
		}
		a := e.audit.events[0]
		if a.Action != "mr.open" || a.ActorID != alice.AccountID || a.Actor != "alice" || a.TenantID != "ten-acme" ||
			a.ResourceType != "merge_request" || a.ResourceID != "mr-1" || a.Details["number"] != "1" ||
			a.Details["source_branch"] != "feature" || a.Details["target_branch"] != "main" {
			t.Fatalf("audit event = %+v", a)
		}
	})

	t.Run("duplicate and reopen", func(t *testing.T) {
		e := newEnv(t)
		first := e.open(t)
		if _, err := e.svc.Open(ctx, bob, e.tree, "Again", "", "feature", "main"); !errors.Is(err, domain.ErrAlreadyExists) {
			t.Fatalf("duplicate Open err = %v, want ErrAlreadyExists", err)
		}
		// Different direction / target is fine.
		if mr, err := e.svc.Open(ctx, alice, e.tree, "Reverse", "", "main", "feature"); err != nil || mr.Number != 2 {
			t.Fatalf("reverse Open = %+v, %v", mr, err)
		}
		if _, err := e.svc.Close(ctx, alice, e.tree, first.Number); err != nil {
			t.Fatal(err)
		}
		if mr, err := e.svc.Open(ctx, alice, e.tree, "Retry", "", "feature", "main"); err != nil || mr.Number != 3 {
			t.Fatalf("Open after close = %+v, %v", mr, err)
		}
	})

	t.Run("concurrent duplicates", func(t *testing.T) {
		e := newEnv(t)
		var wg sync.WaitGroup
		var mu sync.Mutex
		ok, dup := 0, 0
		for range 32 {
			wg.Go(func() {
				_, err := e.svc.Open(ctx, alice, e.tree, "Race", "", "feature", "main")
				mu.Lock()
				defer mu.Unlock()
				switch {
				case err == nil:
					ok++
				case errors.Is(err, domain.ErrAlreadyExists):
					dup++
				default:
					t.Errorf("unexpected error: %v", err)
				}
			})
		}
		wg.Wait()
		if ok != 1 || dup != 31 {
			t.Fatalf("ok=%d dup=%d, want 1 and 31", ok, dup)
		}
	})
}

func TestApprove(t *testing.T) {
	t.Run("errors", func(t *testing.T) {
		tests := []struct {
			name     string
			p        *domain.Principal
			number   int
			setup    func(e *env)
			wantErr  error
			authzErr bool
		}{
			{name: "anonymous", p: nil, number: 1, wantErr: domain.ErrUnauthenticated, authzErr: true},
			{name: "viewer", p: eve, number: 1, wantErr: domain.ErrForbidden, authzErr: true},
			{name: "author", p: alice, number: 1, wantErr: domain.ErrForbidden},
			{name: "not found", p: bob, number: 9, wantErr: domain.ErrNotFound},
			{name: "closed", p: bob, number: 1, wantErr: domain.ErrConflict, setup: func(e *env) {
				if _, err := e.svc.Close(ctx, alice, e.tree, 1); err != nil {
					panic(err)
				}
			}},
		}
		for _, tt := range tests {
			t.Run(tt.name, func(t *testing.T) {
				e := newEnv(t)
				e.open(t)
				if tt.setup != nil {
					tt.setup(e)
				}
				before := len(e.bus.events)
				mr, err := e.svc.Approve(ctx, tt.p, e.tree, tt.number)
				if !errors.Is(err, tt.wantErr) || mr != nil {
					t.Fatalf("Approve = %v, %v; want %v", mr, err, tt.wantErr)
				}
				if tt.authzErr && err != e.authz.last() {
					t.Fatalf("authorizer error not returned unchanged: %v", err)
				}
				if len(e.bus.events) != before {
					t.Fatal("event published on failure")
				}
			})
		}
	})

	t.Run("records approvals idempotently", func(t *testing.T) {
		e := newEnv(t)
		e.open(t)
		mr, err := e.svc.Approve(ctx, bob, e.tree, 1)
		if err != nil {
			t.Fatal(err)
		}
		want := []domain.Approval{{AccountID: bob.AccountID, Username: "bob", At: fixedNow}}
		if !reflect.DeepEqual(mr.Approvals, want) {
			t.Fatalf("approvals = %+v", mr.Approvals)
		}
		again, err := e.svc.Approve(ctx, bob, e.tree, 1)
		if err != nil || len(again.Approvals) != 1 {
			t.Fatalf("second approve = %+v, %v", again, err)
		}
		if _, err := e.svc.Approve(ctx, carol, e.tree, 1); err != nil {
			t.Fatal(err)
		}
		if got := len(e.store.mr(t, e.tree.ID, 1).Approvals); got != 2 {
			t.Fatalf("stored approvals = %d, want 2", got)
		}
		wantEvents := []string{domain.EventMROpened, domain.EventMRApproved, domain.EventMRApproved}
		if got := e.bus.types(); !reflect.DeepEqual(got, wantEvents) {
			t.Fatalf("events = %v, want %v", got, wantEvents)
		}
		wantAudit := []string{"mr.open", "mr.approve", "mr.approve"}
		if got := e.audit.actions(); !reflect.DeepEqual(got, wantAudit) {
			t.Fatalf("audit = %v, want %v", got, wantAudit)
		}
		if e.audit.events[2].Details["approvals"] != "2" || e.audit.events[2].Actor != "carol" {
			t.Fatalf("audit details = %+v", e.audit.events[2])
		}
	})

	t.Run("concurrent approvals are not lost", func(t *testing.T) {
		e := newEnv(t)
		e.open(t)
		const n = 40
		reviewers := make([]*domain.Principal, n)
		for i := range reviewers {
			reviewers[i] = &domain.Principal{AccountID: fmt.Sprintf("acc-%d", i), Username: fmt.Sprintf("u%d", i)}
			e.authz.grants[reviewers[i].Username] = developer
		}
		var wg sync.WaitGroup
		for _, p := range reviewers {
			wg.Go(func() {
				// Each reviewer approves twice to exercise idempotency
				// under contention.
				for range 2 {
					if _, err := e.svc.Approve(ctx, p, e.tree, 1); err != nil {
						t.Errorf("Approve(%s): %v", p.Username, err)
					}
				}
			})
		}
		wg.Wait()
		if got := len(e.store.mr(t, e.tree.ID, 1).Approvals); got != n {
			t.Fatalf("approvals = %d, want %d", got, n)
		}
	})
}

func TestClose(t *testing.T) {
	tests := []struct {
		name     string
		p        *domain.Principal
		number   int
		wantErr  error
		authzErr bool
	}{
		{name: "author", p: alice},
		{name: "maintainer", p: carol},
		{name: "server admin with merge rights", p: root},
		{name: "other developer", p: bob, wantErr: domain.ErrForbidden, authzErr: true},
		{name: "anonymous", p: nil, wantErr: domain.ErrUnauthenticated, authzErr: true},
		{name: "not found", p: alice, number: 7, wantErr: domain.ErrNotFound},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			e := newEnv(t)
			e.open(t)
			number := tt.number
			if number == 0 {
				number = 1
			}
			mr, err := e.svc.Close(ctx, tt.p, e.tree, number)
			if tt.wantErr != nil {
				if !errors.Is(err, tt.wantErr) || mr != nil {
					t.Fatalf("Close = %v, %v; want %v", mr, err, tt.wantErr)
				}
				if tt.authzErr && err != e.authz.last() {
					t.Fatalf("authorizer error not returned unchanged: %v", err)
				}
				if s := e.store.mr(t, e.tree.ID, 1).State; s != domain.MRStateOpen {
					t.Fatalf("state = %s after failed close", s)
				}
				return
			}
			if err != nil {
				t.Fatal(err)
			}
			if mr.State != domain.MRStateClosed || e.store.mr(t, e.tree.ID, 1).State != domain.MRStateClosed {
				t.Fatalf("state = %s", mr.State)
			}
			if got := e.bus.types(); !reflect.DeepEqual(got, []string{domain.EventMROpened, domain.EventMRClosed}) {
				t.Fatalf("events = %v", got)
			}
			if got := e.audit.actions(); !reflect.DeepEqual(got, []string{"mr.open", "mr.close"}) {
				t.Fatalf("audit = %v", got)
			}
			// Closing again is a state conflict; so is approving or merging.
			if _, err := e.svc.Close(ctx, alice, e.tree, 1); !errors.Is(err, domain.ErrConflict) {
				t.Fatalf("second Close err = %v, want ErrConflict", err)
			}
			if _, err := e.svc.Merge(ctx, carol, e.tree, 1); !errors.Is(err, domain.ErrConflict) {
				t.Fatalf("Merge closed err = %v, want ErrConflict", err)
			}
		})
	}

	t.Run("author without tree access", func(t *testing.T) {
		e := newEnv(t)
		e.open(t)
		e.authz.grants["alice"] = nil
		if _, err := e.svc.Close(ctx, alice, e.tree, 1); !errors.Is(err, domain.ErrForbidden) {
			t.Fatalf("err = %v, want ErrForbidden", err)
		}
	})
}

func TestMergeFastForward(t *testing.T) {
	e := newEnv(t)
	e.open(t)
	mr, err := e.svc.Merge(ctx, carol, e.tree, 1)
	if err != nil {
		t.Fatal(err)
	}
	if mr.State != domain.MRStateMerged || mr.MergedSnapshot != s3 || mr.MergedAt == nil || !mr.MergedAt.Equal(fixedNow) {
		t.Fatalf("merged mr = %+v", mr)
	}
	if tip, _ := e.store.tip(e.tree.ID, "main"); tip != s3 {
		t.Fatalf("main tip = %s, want %s", tip, s3)
	}
	if stored := e.store.mr(t, e.tree.ID, 1); !reflect.DeepEqual(stored, mr) {
		t.Fatalf("stored = %+v\nreturned = %+v", stored, mr)
	}
	wantEvents := []string{domain.EventMROpened, domain.EventMRMerged, domain.EventBranchPushed}
	if got := e.bus.types(); !reflect.DeepEqual(got, wantEvents) {
		t.Fatalf("events = %v, want %v", got, wantEvents)
	}
	push := e.bus.events[2]
	wantPayload := map[string]any{"branch": "main", "old_tip": s2, "new_tip": s3, "merge_request": 1}
	if !reflect.DeepEqual(push.Payload, wantPayload) || push.Actor != "carol" || push.TreeID != e.tree.ID {
		t.Fatalf("branch.pushed event = %+v", push)
	}
	if got := e.audit.actions(); !reflect.DeepEqual(got, []string{"mr.open", "mr.merge"}) {
		t.Fatalf("audit = %v", got)
	}
	if d := e.audit.events[1].Details; d["old_tip"] != s2 || d["new_tip"] != s3 || d["up_to_date"] != "false" {
		t.Fatalf("audit details = %v", d)
	}
	// Merging again fails: the merge request is no longer open.
	if _, err := e.svc.Merge(ctx, carol, e.tree, 1); !errors.Is(err, domain.ErrConflict) {
		t.Fatalf("second Merge err = %v, want ErrConflict", err)
	}
	if _, err := e.svc.Approve(ctx, bob, e.tree, 1); !errors.Is(err, domain.ErrConflict) {
		t.Fatalf("Approve merged err = %v, want ErrConflict", err)
	}
}

func TestMergeErrors(t *testing.T) {
	tests := []struct {
		name     string
		p        *domain.Principal
		number   int
		setup    func(t *testing.T, e *env)
		wantErr  error
		authzErr bool
		contains string
	}{
		{name: "anonymous", p: nil, wantErr: domain.ErrUnauthenticated, authzErr: true},
		{name: "developer lacks mr:merge", p: alice, wantErr: domain.ErrForbidden, authzErr: true},
		{name: "not found", p: carol, number: 5, wantErr: domain.ErrNotFound},
		{
			name: "diverged", p: carol, wantErr: domain.ErrConflict,
			contains: "target has diverged; merge target into source locally and push",
			setup:    func(t *testing.T, e *env) { e.store.setBranch(e.tree.ID, "main", s4) },
		},
		{
			name: "sibling tip", p: carol, wantErr: domain.ErrConflict,
			contains: "target has diverged",
			setup:    func(t *testing.T, e *env) { e.store.setBranch(e.tree.ID, "main", s5) },
		},
		{
			name: "source deleted", p: carol, wantErr: domain.ErrNotFound,
			setup: func(t *testing.T, e *env) {
				e.store.mu.Lock()
				delete(e.store.branches, bkey(e.tree.ID, "feature"))
				e.store.mu.Unlock()
			},
		},
		{
			name: "source emptied", p: carol, wantErr: domain.ErrInvalid,
			setup: func(t *testing.T, e *env) { e.store.setBranch(e.tree.ID, "feature", "") },
		},
		{
			name: "target moved concurrently", p: carol, wantErr: domain.ErrConflict,
			contains: "moved during the merge",
			setup: func(t *testing.T, e *env) {
				e.store.beforeCAS = func() { e.store.setBranch(e.tree.ID, "main", s4) }
			},
		},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			e := newEnv(t)
			e.open(t)
			if tt.setup != nil {
				tt.setup(t, e)
			}
			number := tt.number
			if number == 0 {
				number = 1
			}
			tipBefore, _ := e.store.tip(e.tree.ID, "main")
			mr, err := e.svc.Merge(ctx, tt.p, e.tree, number)
			if !errors.Is(err, tt.wantErr) || mr != nil {
				t.Fatalf("Merge = %v, %v; want %v", mr, err, tt.wantErr)
			}
			if tt.authzErr && err != e.authz.last() {
				t.Fatalf("authorizer error not returned unchanged: %v", err)
			}
			if tt.contains != "" && !strings.Contains(err.Error(), tt.contains) {
				t.Fatalf("error %q does not mention %q", err, tt.contains)
			}
			if s := e.store.mr(t, e.tree.ID, 1).State; s != domain.MRStateOpen {
				t.Fatalf("state = %s after failed merge", s)
			}
			if e.store.beforeCAS == nil {
				if tip, _ := e.store.tip(e.tree.ID, "main"); tip != tipBefore {
					t.Fatalf("main moved to %s on failed merge", tip)
				}
			}
			if got := e.bus.types(); !reflect.DeepEqual(got, []string{domain.EventMROpened}) {
				t.Fatalf("events = %v", got)
			}
		})
	}
}

func TestMergeApprovals(t *testing.T) {
	tests := []struct {
		name      string
		rules     []domain.BranchProtection
		approvers []*domain.Principal
		// extra approvals injected straight into the store (bypassing
		// Approve), e.g. a stale self-approval or a duplicate record.
		injected []domain.Approval
		reason   string // "" = merge succeeds
	}{
		{name: "no protection"},
		{
			name:   "none of one",
			rules:  []domain.BranchProtection{{Pattern: "main", RequiredApprovals: 1}},
			reason: "0 of 1 approvals",
		},
		{
			name:      "one of two",
			rules:     []domain.BranchProtection{{Pattern: "main", RequiredApprovals: 2}},
			approvers: []*domain.Principal{bob},
			reason:    "1 of 2 approvals",
		},
		{
			name:      "two of two",
			rules:     []domain.BranchProtection{{Pattern: "main", RequiredApprovals: 2}},
			approvers: []*domain.Principal{bob, carol},
		},
		{
			name: "most restrictive rule wins",
			rules: []domain.BranchProtection{
				{Pattern: "main", RequiredApprovals: 1},
				{Pattern: "m*", RequiredApprovals: 3},
				{Pattern: "**", RequiredApprovals: 2},
			},
			approvers: []*domain.Principal{bob, carol},
			reason:    "2 of 3 approvals",
		},
		{
			name:      "rules on other branches ignored",
			rules:     []domain.BranchProtection{{Pattern: "release/*", RequiredApprovals: 5}},
			approvers: nil,
		},
		{
			name:     "author and duplicate approvals do not count",
			rules:    []domain.BranchProtection{{Pattern: "main", RequiredApprovals: 2}},
			injected: []domain.Approval{{AccountID: alice.AccountID}, {AccountID: bob.AccountID}, {AccountID: bob.AccountID}, {}},
			reason:   "1 of 2 approvals",
		},
		{
			name:      "no direct push does not block merges",
			rules:     []domain.BranchProtection{{Pattern: "main", NoDirectPush: true, RequiredApprovals: 1, AllowedPushers: []string{"zed"}}},
			approvers: []*domain.Principal{dave},
		},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			e := newEnv(t)
			e.protect(tt.rules...)
			e.open(t)
			for _, p := range tt.approvers {
				if _, err := e.svc.Approve(ctx, p, e.tree, 1); err != nil {
					t.Fatalf("Approve(%s): %v", p.Username, err)
				}
			}
			if len(tt.injected) > 0 {
				mr := e.store.mr(t, e.tree.ID, 1)
				mr.Approvals = append(mr.Approvals, tt.injected...)
				if err := e.store.UpdateMergeRequest(ctx, mr); err != nil {
					t.Fatal(err)
				}
			}
			mr, err := e.svc.Merge(ctx, carol, e.tree, 1)
			if tt.reason == "" {
				if err != nil || mr.State != domain.MRStateMerged {
					t.Fatalf("Merge = %+v, %v", mr, err)
				}
				return
			}
			if mr != nil {
				t.Fatalf("Merge returned %+v with error", mr)
			}
			if pd := pushDenied(t, err, "review_required"); pd.Reason != tt.reason {
				t.Fatalf("reason = %q, want %q", pd.Reason, tt.reason)
			}
			if tip, _ := e.store.tip(e.tree.ID, "main"); tip != s2 {
				t.Fatalf("main moved to %s", tip)
			}
		})
	}
}

func TestMergeUpToDate(t *testing.T) {
	tests := []struct {
		name      string
		targetTip string
	}{
		{"same tip", s3},          // e.g. source pushed to main directly meanwhile
		{"target contains", "s6"}, // main moved past the source tip
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			e := newEnv(t)
			e.hist.parents["s6"] = []string{s3}
			e.open(t)
			e.store.setBranch(e.tree.ID, "main", tt.targetTip)
			mr, err := e.svc.Merge(ctx, carol, e.tree, 1)
			if err != nil {
				t.Fatal(err)
			}
			if mr.State != domain.MRStateMerged || mr.MergedSnapshot != s3 {
				t.Fatalf("mr = %+v", mr)
			}
			if tip, _ := e.store.tip(e.tree.ID, "main"); tip != tt.targetTip || e.store.casCalls != 0 {
				t.Fatalf("main moved to %s (%d CAS calls) for an up-to-date merge", tip, e.store.casCalls)
			}
			if got := e.bus.types(); !reflect.DeepEqual(got, []string{domain.EventMROpened, domain.EventMRMerged}) {
				t.Fatalf("events = %v (no branch.pushed expected)", got)
			}
			if d := e.audit.events[len(e.audit.events)-1].Details; d["up_to_date"] != "true" {
				t.Fatalf("audit details = %v", d)
			}
		})
	}
}

func TestMergeRecreatesDeletedTarget(t *testing.T) {
	e := newEnv(t)
	e.open(t)
	e.store.mu.Lock()
	delete(e.store.branches, bkey(e.tree.ID, "main"))
	e.store.mu.Unlock()
	mr, err := e.svc.Merge(ctx, carol, e.tree, 1)
	if err != nil {
		t.Fatal(err)
	}
	if tip, ok := e.store.tip(e.tree.ID, "main"); !ok || tip != s3 || mr.MergedSnapshot != s3 {
		t.Fatalf("main = %q (exists %v), merged %q", tip, ok, mr.MergedSnapshot)
	}
	if p := e.bus.events[len(e.bus.events)-1].Payload.(map[string]any); p["old_tip"] != "" {
		t.Fatalf("branch.pushed payload = %v", p)
	}
}

func TestMergeEmptyTarget(t *testing.T) {
	e := newEnv(t)
	e.open(t)
	e.store.setBranch(e.tree.ID, "main", "")
	if _, err := e.svc.Merge(ctx, carol, e.tree, 1); err != nil {
		t.Fatal(err)
	}
	if tip, _ := e.store.tip(e.tree.ID, "main"); tip != s3 {
		t.Fatalf("main tip = %s", tip)
	}
}

func TestMergeUpdateFailureIsRetryable(t *testing.T) {
	e := newEnv(t)
	e.open(t)
	boom := errors.New("disk full")
	e.store.updateErr = boom
	if _, err := e.svc.Merge(ctx, carol, e.tree, 1); !errors.Is(err, boom) {
		t.Fatalf("err = %v, want %v", err, boom)
	}
	e.store.updateErr = nil
	mr, err := e.svc.Merge(ctx, carol, e.tree, 1)
	if err != nil || mr.State != domain.MRStateMerged || mr.MergedSnapshot != s3 {
		t.Fatalf("retry = %+v, %v", mr, err)
	}
}

func TestMergeParallelSameMR(t *testing.T) {
	e := newEnv(t)
	e.protect(domain.BranchProtection{Pattern: "main", RequiredApprovals: 1})
	e.open(t)
	if _, err := e.svc.Approve(ctx, bob, e.tree, 1); err != nil {
		t.Fatal(err)
	}
	const n = 64
	errs := make([]error, n)
	var wg sync.WaitGroup
	start := make(chan struct{})
	for i := range n {
		wg.Go(func() {
			<-start
			_, errs[i] = e.svc.Merge(ctx, carol, e.tree, 1)
		})
	}
	close(start)
	wg.Wait()
	ok := 0
	for _, err := range errs {
		switch {
		case err == nil:
			ok++
		case !errors.Is(err, domain.ErrConflict):
			t.Errorf("unexpected error: %v", err)
		}
	}
	if ok != 1 {
		t.Fatalf("%d merges succeeded, want exactly 1", ok)
	}
	if got := e.bus.types(); !reflect.DeepEqual(got, []string{
		domain.EventMROpened, domain.EventMRApproved, domain.EventMRMerged, domain.EventBranchPushed,
	}) {
		t.Fatalf("events = %v", got)
	}
}

// Two merge requests into the same target whose sources diverge: whichever
// merges first wins and the other must be rebased locally.
func TestMergeParallelCompetingMRs(t *testing.T) {
	for range 20 {
		e := newEnv(t)
		e.open(t) // !1 feature (s3) -> main (s2)
		if _, err := e.svc.Open(ctx, bob, e.tree, "Other", "", "other", "main"); err != nil {
			t.Fatal(err) // !2 other (s5) -> main (s2)
		}
		var wg sync.WaitGroup
		errs := make([]error, 2)
		for i := range 2 {
			wg.Go(func() { _, errs[i] = e.svc.Merge(ctx, carol, e.tree, i+1) })
		}
		wg.Wait()
		ok := 0
		for _, err := range errs {
			if err == nil {
				ok++
			} else if !errors.Is(err, domain.ErrConflict) {
				t.Fatalf("unexpected error: %v", err)
			}
		}
		if ok != 1 {
			t.Fatalf("%d merges succeeded, want exactly 1 (%v)", ok, errs)
		}
		tip, _ := e.store.tip(e.tree.ID, "main")
		if (errs[0] == nil && tip != s3) || (errs[1] == nil && tip != s5) {
			t.Fatalf("main tip %s does not match the winning merge", tip)
		}
	}
}

// Merges in different service instances (e.g. two server processes sharing
// a store) are serialized only by the store's CAS; exactly one may win.
func TestMergeParallelAcrossInstances(t *testing.T) {
	e := newEnv(t)
	e.open(t)
	if _, err := e.svc.Open(ctx, bob, e.tree, "Other", "", "other", "main"); err != nil {
		t.Fatal(err)
	}
	other := New(e.store, e.hist, e.authz, e.bus, e.audit)
	var wg sync.WaitGroup
	errs := make([]error, 2)
	gate := make(chan struct{})
	var once sync.Once
	// Make both merges read main's tip before either swaps it.
	var arrived sync.WaitGroup
	arrived.Add(2)
	e.store.beforeCAS = func() {
		arrived.Done()
		once.Do(func() { go func() { arrived.Wait(); close(gate) }() })
		<-gate
	}
	wg.Go(func() { _, errs[0] = e.svc.Merge(ctx, carol, e.tree, 1) })
	wg.Go(func() { _, errs[1] = other.Merge(ctx, carol, e.tree, 2) })
	wg.Wait()
	if (errs[0] == nil) == (errs[1] == nil) {
		t.Fatalf("want exactly one success, got %v", errs)
	}
	for _, err := range errs {
		if err != nil && !errors.Is(err, domain.ErrConflict) {
			t.Fatalf("unexpected error: %v", err)
		}
	}
}

func TestNilBusAndAudit(t *testing.T) {
	e := newEnv(t)
	svc := New(e.store, e.hist, e.authz, nil, nil)
	mr, err := svc.Open(ctx, alice, e.tree, "x", "", "feature", "main")
	if err != nil {
		t.Fatal(err)
	}
	if _, err := svc.Approve(ctx, bob, e.tree, mr.Number); err != nil {
		t.Fatal(err)
	}
	if _, err := svc.Merge(ctx, carol, e.tree, mr.Number); err != nil {
		t.Fatal(err)
	}
}

func TestEventPayloadIsDetached(t *testing.T) {
	e := newEnv(t)
	e.open(t)
	mr, err := e.svc.Approve(ctx, bob, e.tree, 1)
	if err != nil {
		t.Fatal(err)
	}
	mr.Approvals[0].Username = "mutated"
	p := e.bus.events[1].Payload.(domain.MergeRequest)
	if p.Approvals[0].Username != "bob" {
		t.Fatal("event payload aliases the returned merge request")
	}
}
