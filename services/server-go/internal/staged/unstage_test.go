package staged

import (
	"context"
	"errors"
	"testing"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

func TestUnstage(t *testing.T) {
	ctx := context.Background()
	tests := []struct {
		name        string
		caller      *domain.Principal
		account     string
		branch      string
		deny        domain.Action
		wantErr     error
		wantRemoved []string // branch names
		wantLeft    int
	}{
		{name: "own entry on one branch", caller: alice, branch: "main", wantRemoved: []string{"main"}, wantLeft: 2},
		{name: "own entries on all branches", caller: alice, wantRemoved: []string{"feature", "main"}, wantLeft: 1},
		{name: "nothing to remove", caller: carol, branch: "main", wantRemoved: []string{}, wantLeft: 3},
		{name: "explicit own account", caller: bob, account: bob.AccountID, wantRemoved: []string{"main"}, wantLeft: 2},
		{name: "other account as tree admin", caller: alice, account: bob.AccountID, branch: "main", wantRemoved: []string{"main"}, wantLeft: 2},
		{name: "other account without tree admin", caller: alice, account: bob.AccountID, deny: domain.ActTreeAdmin, wantErr: domain.ErrForbidden, wantLeft: 3},
		{name: "own entry needs read access", caller: alice, deny: domain.ActTreeRead, wantErr: domain.ErrForbidden, wantLeft: 3},
		{name: "anonymous", caller: nil, wantErr: domain.ErrUnauthenticated, wantLeft: 3},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			e := newEnv()
			stage(t, e, alice, "main", "a-main")
			stage(t, e, alice, "feature", "a-feature")
			stage(t, e, bob, "main", "b-main")
			if tc.deny != "" {
				e.authz.deny[tc.deny] = true
			}
			removed, err := e.svc.Unstage(ctx, tc.caller, tree, tc.account, tc.branch)
			if tc.wantErr != nil {
				if !errors.Is(err, tc.wantErr) {
					t.Fatalf("err = %v, want %v", err, tc.wantErr)
				}
			} else {
				if err != nil {
					t.Fatal(err)
				}
				var got []string
				for _, r := range removed {
					got = append(got, r.BranchName)
				}
				if len(got) != len(tc.wantRemoved) {
					t.Fatalf("removed %v, want %v", got, tc.wantRemoved)
				}
				for i := range got {
					if got[i] != tc.wantRemoved[i] {
						t.Fatalf("removed %v, want %v", got, tc.wantRemoved)
					}
				}
				cleared := e.bus.ofType(domain.EventStagedCleared)
				if len(cleared) != len(tc.wantRemoved) {
					t.Fatalf("%d staged.cleared events, want %d", len(cleared), len(tc.wantRemoved))
				}
				for _, ev := range cleared {
					c, ok := ev.Payload.(Cleared)
					if !ok || c.Reason != ReasonUnstaged || ev.TreeID != tree.ID {
						t.Fatalf("bad event %+v", ev)
					}
				}
			}
			if got := e.store.count(); got != tc.wantLeft {
				t.Fatalf("%d entries left, want %d", got, tc.wantLeft)
			}
		})
	}
}

func TestUnstageNilTree(t *testing.T) {
	e := newEnv()
	if _, err := e.svc.Unstage(context.Background(), alice, nil, "", ""); !errors.Is(err, domain.ErrInvalid) {
		t.Fatalf("err = %v", err)
	}
}
