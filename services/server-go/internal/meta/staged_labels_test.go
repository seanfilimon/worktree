package meta

// staged-v2 (§16.8): labeled staged slots, part of the MetaStore
// conformance suite.

import (
	"context"
	"testing"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

func testStagedLabels(t *testing.T, s domain.MetaStore) {
	ctx := context.Background()
	put := func(id, label string, sec int) {
		must(t, s.PutStaged(ctx, &domain.StagedSnapshot{SnapshotID: id, TreeID: "tr", AccountID: "a1",
			BranchName: "main", Label: label, StagedAt: at(sec)}))
	}
	put("d1", "", 1)
	put("x1", "exp", 2)
	put("y1", "other", 3)
	put("x2", "exp", 4) // replaces x1 only
	put("d2", "", 5)    // replaces d1 only

	labels := func() map[string]string {
		all, err := s.ListStaged(ctx, "tr", "main")
		must(t, err)
		out := map[string]string{}
		for _, e := range all {
			out[e.Label] = e.SnapshotID
		}
		return out
	}
	wantEqual(t, "slots", labels(), map[string]string{"": "d2", "exp": "x2", "other": "y1"})
	if n, err := s.CountStaged(ctx); err != nil || n != 3 {
		t.Fatalf("CountStaged = %d, %v", n, err)
	}

	// Ties in staged_at sort by label.
	put("t1", "b", 10)
	put("t2", "a", 10)
	all, err := s.ListStaged(ctx, "tr", "")
	must(t, err)
	if all[0].Label != "a" || all[1].Label != "b" {
		t.Fatalf("tie order = %q, %q", all[0].Label, all[1].Label)
	}

	// DeleteStaged removes the default slot; DeleteStagedLabel one label.
	must(t, s.DeleteStaged(ctx, "tr", "a1", "main"))
	must(t, s.DeleteStagedLabel(ctx, "tr", "a1", "main", "exp"))
	must(t, s.DeleteStagedLabel(ctx, "tr", "a1", "main", "exp")) // idempotent
	must(t, s.DeleteStagedLabel(ctx, "tr", "a1", "main", "nope"))
	wantEqual(t, "after deletes", labels(), map[string]string{"other": "y1", "a": "t2", "b": "t1"})
	must(t, s.DeleteStagedLabel(ctx, "tr", "a1", "main", ""))

	// A label must not contain the key separator.
	wantErr(t, s.PutStaged(ctx, &domain.StagedSnapshot{TreeID: "tr", AccountID: "a1", BranchName: "main", Label: "a\x00b"}), domain.ErrInvalid)
}
