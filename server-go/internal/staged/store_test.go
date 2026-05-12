package staged

import (
	"context"
	"testing"
)

func TestFileStoreListFiltersSnapshots(t *testing.T) {
	store := NewFileStore(t.TempDir())
	ctx := context.Background()
	snapshots := []Snapshot{
		{SnapshotID: "snap-1", Tenant: "acme", Worktree: "api", Branch: "main"},
		{SnapshotID: "snap-2", Tenant: "acme", Worktree: "web", Branch: "main"},
		{SnapshotID: "snap-3", Tenant: "other", Worktree: "api", Branch: "main"},
	}
	for _, snapshot := range snapshots {
		if err := store.Add(ctx, snapshot); err != nil {
			t.Fatalf("Add() error = %v", err)
		}
	}

	got, err := store.List(ctx, ListFilter{Tenant: "acme", Worktree: "api"})
	if err != nil {
		t.Fatalf("List() error = %v", err)
	}
	if len(got) != 1 {
		t.Fatalf("len(got) = %d", len(got))
	}
	if got[0].SnapshotID != "snap-1" {
		t.Fatalf("snapshot = %q", got[0].SnapshotID)
	}
}

func TestFileStoreListMissingIndexReturnsEmpty(t *testing.T) {
	store := NewFileStore(t.TempDir())
	got, err := store.List(context.Background(), ListFilter{})
	if err != nil {
		t.Fatalf("List() error = %v", err)
	}
	if len(got) != 0 {
		t.Fatalf("len(got) = %d", len(got))
	}
}
