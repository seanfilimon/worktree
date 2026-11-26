package meta

import (
	"context"
	"errors"
	"testing"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// TestPutSnapshotsIsAtomic: a batch stores every snapshot, is idempotent,
// and stores none of them when one conflicts.
func TestPutSnapshotsIsAtomic(t *testing.T) {
	ctx := context.Background()
	b := newBoltForTest(t).(*Bolt)
	mk := func(id, msg string) *domain.Snapshot {
		return &domain.Snapshot{ID: id, TreeID: "tree-1", Message: msg}
	}
	must(t, b.PutSnapshots(ctx, []*domain.Snapshot{mk("a", "one"), mk("b", "two")}))
	for _, id := range []string{"a", "b"} {
		if _, err := b.GetSnapshot(ctx, id); err != nil {
			t.Fatalf("snapshot %s: %v", id, err)
		}
	}
	// Same content again is fine.
	must(t, b.PutSnapshots(ctx, []*domain.Snapshot{mk("a", "one"), mk("b", "two")}))
	// "a" with different content conflicts; "c" must not be stored.
	err := b.PutSnapshots(ctx, []*domain.Snapshot{mk("c", "three"), mk("a", "changed")})
	if !errors.Is(err, domain.ErrConflict) {
		t.Fatalf("err = %v, want ErrConflict", err)
	}
	if _, err := b.GetSnapshot(ctx, "c"); !errors.Is(err, domain.ErrNotFound) {
		t.Fatalf("snapshot c after a failed batch: err = %v, want ErrNotFound", err)
	}
}
