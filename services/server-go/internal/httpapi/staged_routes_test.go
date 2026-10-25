package httpapi

import (
	"context"
	"net/http"
	"sort"
	"testing"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
	"github.com/seanfilimon/worktree/services/server-go/internal/staged"
)

// unstageFixture backs Deps.Staged with the real staged service over an
// in-memory store holding: bob@main, bob@feature, alice@main (tree web).
func unstageFixture(t *testing.T) (*fixture, *meta.Memory) {
	t.Helper()
	mem := meta.NewMemory()
	f := newFixture(t, func(d *Deps) {
		d.Staged = staged.New(mem, d.Authz, d.Bus)
	})
	ctx := context.Background()
	for _, e := range []domain.StagedSnapshot{
		{SnapshotID: "wip-b1", AccountID: "acct-bob", Author: "bob", BranchName: "main"},
		{SnapshotID: "wip-b2", AccountID: "acct-bob", Author: "bob", BranchName: "feature"},
		{SnapshotID: "wip-a1", AccountID: "acct-alice", Author: "alice", BranchName: "main"},
	} {
		e.TreeID, e.TenantID, e.StagedAt = f.web.ID, f.acme.ID, t0
		must(t, mem.PutStaged(ctx, &e))
	}
	return f, mem
}

// left lists the remaining entries as "author@branch", sorted.
func left(t *testing.T, mem *meta.Memory, treeID string) []string {
	t.Helper()
	all, err := mem.ListStaged(context.Background(), treeID, "")
	must(t, err)
	out := []string{}
	for _, e := range all {
		out = append(out, e.Author+"@"+e.BranchName)
	}
	sort.Strings(out)
	return out
}

func TestUnstageEndpoint(t *testing.T) {
	const path = "/api/v1/trees/acme/web/staged"
	tests := []struct {
		name        string
		as          string
		query       string
		status      int
		code        string
		wantRemoved []string // branches
		wantLeft    []string
	}{
		{name: "own branch", as: "bob", query: "?branch=main", status: 200, wantRemoved: []string{"main"}, wantLeft: []string{"alice@main", "bob@feature"}},
		{name: "own entries on every branch", as: "bob", status: 200, wantRemoved: []string{"feature", "main"}, wantLeft: []string{"alice@main"}},
		{name: "nothing staged", as: "carol", status: 200, wantRemoved: []string{}, wantLeft: []string{"alice@main", "bob@feature", "bob@main"}},
		{name: "tree admin removes another account", as: "alice", query: "?account=bob&branch=feature", status: 200, wantRemoved: []string{"feature"}, wantLeft: []string{"alice@main", "bob@main"}},
		{name: "account by id", as: "mallory", query: "?account=acct-alice", status: 200, wantRemoved: []string{"main"}, wantLeft: []string{"bob@feature", "bob@main"}},
		{name: "other account needs tree admin", as: "bob", query: "?account=alice", status: 403, code: "forbidden"},
		{name: "unknown account", as: "alice", query: "?account=nobody", status: 404, code: "not_found"},
		{name: "invalid branch", as: "bob", query: "?branch=a%20b", status: 400, code: "bad_request"},
		{name: "anonymous", status: 401, code: "unauthenticated"},
		{name: "non-member", as: "dave", query: "?branch=main"},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			f, mem := unstageFixture(t)
			rec := f.do(t, http.MethodDelete, path+tc.query, tc.as, nil)
			if tc.status == 0 {
				// Non-members are refused (403 or a hiding 404); nothing changes.
				if rec.Code == http.StatusOK {
					t.Fatalf("non-member unstage succeeded: %s", rec.Body.String())
				}
				if got := left(t, mem, f.web.ID); len(got) != 3 {
					t.Fatalf("entries changed: %v", got)
				}
				return
			}
			if rec.Code != tc.status {
				t.Fatalf("status = %d, want %d; body %s", rec.Code, tc.status, rec.Body.String())
			}
			if tc.code != "" {
				if got := errCode(rec); got != tc.code {
					t.Fatalf("code = %q, want %q", got, tc.code)
				}
				if got := left(t, mem, f.web.ID); len(got) != 3 {
					t.Fatalf("entries changed on error: %v", got)
				}
				return
			}
			body := decode[unstageResponse](t, rec)
			var got []string
			for _, r := range body.Removed {
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
			gotLeft := left(t, mem, f.web.ID)
			if len(gotLeft) != len(tc.wantLeft) {
				t.Fatalf("left %v, want %v", gotLeft, tc.wantLeft)
			}
			for i := range gotLeft {
				if gotLeft[i] != tc.wantLeft[i] {
					t.Fatalf("left %v, want %v", gotLeft, tc.wantLeft)
				}
			}
			if recs, _ := f.audit.Query(context.Background(), domain.AuditFilter{Action: "staged.unstage"}); len(recs) != len(tc.wantRemoved) {
				t.Fatalf("%d audit records, want %d", len(recs), len(tc.wantRemoved))
			}
		})
	}
}

func TestUnstageEndpointNotImplemented(t *testing.T) {
	f := newFixture(t) // fakeStaged does not implement staged.Unstager
	rec := f.do(t, http.MethodDelete, "/api/v1/trees/acme/web/staged?branch=main", "bob", nil)
	if rec.Code != http.StatusNotImplemented || errCode(rec) != "not_implemented" {
		t.Fatalf("status = %d, body %s", rec.Code, rec.Body.String())
	}
}
