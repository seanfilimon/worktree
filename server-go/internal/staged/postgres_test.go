package staged_test

import (
	"context"
	"os"
	"testing"
	"time"

	"github.com/ramizik/worktree/server-go/internal/staged"
)

// Tests require a real Postgres instance.
// Set WT_TEST_DATABASE_URL to run; skip otherwise.
func testDSN(t *testing.T) string {
	t.Helper()
	dsn := os.Getenv("WT_TEST_DATABASE_URL")
	if dsn == "" {
		t.Skip("WT_TEST_DATABASE_URL not set; skipping Postgres tests")
	}
	return dsn
}

func TestPostgresStore_AddAndList(t *testing.T) {
	dsn := testDSN(t)
	ctx := context.Background()

	store, err := staged.NewPostgresStore(ctx, dsn)
	if err != nil {
		t.Fatalf("NewPostgresStore: %v", err)
	}
	defer store.Close()

	hash := "abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890"
	snap := staged.Snapshot{
		SnapshotID: "test-snap-" + t.Name(),
		Tenant:     "tenant-a",
		Worktree:   "wt-1",
		TreeID:     "tree-1",
		Branch:     "main",
		ObjectIDs:  []string{hash},
		CreatedAt:  time.Now().UTC().Truncate(time.Microsecond),
	}

	if err := store.Add(ctx, snap); err != nil {
		t.Fatalf("Add: %v", err)
	}

	// Idempotent second add must not error
	if err := store.Add(ctx, snap); err != nil {
		t.Fatalf("Add (idempotent): %v", err)
	}

	all, err := store.List(ctx, staged.ListFilter{Tenant: "tenant-a"})
	if err != nil {
		t.Fatalf("List: %v", err)
	}
	found := false
	for _, s := range all {
		if s.SnapshotID == snap.SnapshotID {
			found = true
			if s.Branch != "main" {
				t.Errorf("Branch = %q, want main", s.Branch)
			}
			if len(s.ObjectIDs) != 1 {
				t.Errorf("ObjectIDs len = %d, want 1", len(s.ObjectIDs))
			}
		}
	}
	if !found {
		t.Error("snapshot not found after Add")
	}
}

func TestPostgresStore_ListFilter(t *testing.T) {
	dsn := testDSN(t)
	ctx := context.Background()

	store, err := staged.NewPostgresStore(ctx, dsn)
	if err != nil {
		t.Fatalf("NewPostgresStore: %v", err)
	}
	defer store.Close()

	hash := "abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890"
	snapA := staged.Snapshot{SnapshotID: "filter-test-a-" + t.Name(), Tenant: "filter-tenant", Worktree: "wt-a", TreeID: "t", Branch: "feat", ObjectIDs: []string{hash}, CreatedAt: time.Now().UTC()}
	snapB := staged.Snapshot{SnapshotID: "filter-test-b-" + t.Name(), Tenant: "filter-tenant", Worktree: "wt-b", TreeID: "t", Branch: "main", ObjectIDs: []string{hash}, CreatedAt: time.Now().UTC()}

	_ = store.Add(ctx, snapA)
	_ = store.Add(ctx, snapB)

	results, err := store.List(ctx, staged.ListFilter{Tenant: "filter-tenant", Worktree: "wt-a"})
	if err != nil {
		t.Fatalf("List with worktree filter: %v", err)
	}
	for _, s := range results {
		if s.Worktree != "wt-a" {
			t.Errorf("filter returned worktree %q, want wt-a", s.Worktree)
		}
	}

	results, err = store.List(ctx, staged.ListFilter{Tenant: "filter-tenant", Branch: "main"})
	if err != nil {
		t.Fatalf("List with branch filter: %v", err)
	}
	for _, s := range results {
		if s.Branch != "main" {
			t.Errorf("filter returned branch %q, want main", s.Branch)
		}
	}
}
