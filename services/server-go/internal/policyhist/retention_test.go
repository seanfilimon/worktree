package policyhist

// server-ops (docs/ENGINEERING_PLAN.md §16.11): policy version retention.

import (
	"context"
	"testing"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
)

func versionsOf(t *testing.T, r *Recorder, tenant, policy string) []int {
	t.Helper()
	vs, err := r.Versions(context.Background(), tenant, policy)
	if err != nil {
		t.Fatal(err)
	}
	var out []int
	for _, v := range vs {
		out = append(out, v.Version)
	}
	return out
}

func TestPrune(t *testing.T) {
	ctx := context.Background()
	store := meta.NewMemory()
	r := New(store, nil, nil)
	now := time.Date(2026, 10, 1, 0, 0, 0, 0, time.UTC)
	r.now = func() time.Time { return now }

	// p1: five versions, one per day; p2 (tenant t2): one old version.
	p := pol("p1", "guard", st("a", "deny", "tree:write"))
	for i := 0; i < 5; i++ {
		change := ChangeUpdate
		if i == 0 {
			change = ChangeCreate
		}
		if _, err := r.Record(ctx, Entry{Change: change, Actor: "ada", Policy: p}); err != nil {
			t.Fatal(err)
		}
		now = now.Add(24 * time.Hour)
	}
	q := pol("p2", "other", st("b", "allow", "tree:read"))
	q.TenantID = "t2"
	now = now.Add(-30 * 24 * time.Hour)
	if _, err := r.Record(ctx, Entry{Change: ChangeCreate, Actor: "ada", Policy: q}); err != nil {
		t.Fatal(err)
	}
	now = time.Date(2026, 10, 6, 0, 0, 0, 0, time.UTC) // versions are 1..5 days old

	if n, err := r.Prune(ctx, 0, 0); err != nil || n != 0 {
		t.Fatalf("no limits = %d, %v", n, err)
	}
	// Count: keep the newest 3 of p1.
	n, err := r.Prune(ctx, 0, 3)
	if err != nil || n != 2 {
		t.Fatalf("keep 3 = %d, %v", n, err)
	}
	if got := versionsOf(t, r, "t1", "p1"); len(got) != 3 || got[0] != 3 || got[2] != 5 {
		t.Fatalf("p1 after keep 3 = %v", got)
	}
	// Age: older than 60 h goes, but the latest version always stays (p2's
	// only version is a month old).
	n, err = r.Prune(ctx, 60*time.Hour, 0)
	if err != nil || n != 1 {
		t.Fatalf("max age = %d, %v", n, err)
	}
	if got := versionsOf(t, r, "t1", "p1"); len(got) != 2 || got[0] != 4 || got[1] != 5 {
		t.Fatalf("p1 after max age = %v", got)
	}
	if got := versionsOf(t, r, "t2", "p2"); len(got) != 1 {
		t.Fatalf("p2 = %v", got)
	}
	// New versions continue the numbering after the latest.
	v, err := r.Record(ctx, Entry{Change: ChangeUpdate, Actor: "ada", Policy: p})
	if err != nil || v.Version != 6 {
		t.Fatalf("record after prune = %+v, %v", v, err)
	}
	// keep 1 leaves only the latest of each policy.
	if n, err := r.Prune(ctx, 0, 1); err != nil || n != 2 {
		t.Fatalf("keep 1 = %d, %v", n, err)
	}
	if got := versionsOf(t, r, "t1", "p1"); len(got) != 1 || got[0] != 6 {
		t.Fatalf("p1 after keep 1 = %v", got)
	}
}
