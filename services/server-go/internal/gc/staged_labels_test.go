package gc

// staged-v2 (§16.8): retention removes exactly the expired labeled slot.

import (
	"testing"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

func TestStagedRetentionIsLabelAware(t *testing.T) {
	f := newFixture(t)
	put := func(label string, age time.Duration) {
		must(t, f.store.PutStaged(f.ctx, &domain.StagedSnapshot{SnapshotID: "s-" + label, TreeID: f.tree.ID,
			TenantID: f.tree.TenantID, AccountID: "acct-a", BranchName: "main", Label: label, StagedAt: t0.Add(-age)}))
	}
	put("", time.Hour)
	put("old", 200*time.Hour)
	put("fresh", time.Hour)
	rep := f.run(false)
	if rep.StagedDeleted != 1 {
		t.Fatalf("staged deleted = %d", rep.StagedDeleted)
	}
	left, err := f.store.ListStaged(f.ctx, f.tree.ID, "")
	must(t, err)
	got := map[string]bool{}
	for _, s := range left {
		got[s.Label] = true
	}
	if len(left) != 2 || !got[""] || !got["fresh"] {
		t.Fatalf("left = %+v", left)
	}
}
