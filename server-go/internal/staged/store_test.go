package staged

import (
	"context"
	"errors"
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
		if _, err := store.Add(ctx, snapshot); err != nil {
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

func TestFileStoreAddIsIdempotent(t *testing.T) {
	store := NewFileStore(t.TempDir())
	snap := Snapshot{SnapshotID: "snap-1", Tenant: "acme", Worktree: "api", TreeID: "tree-1", Branch: "main", Objects: []ObjectRef{{Path: "a.rs", Hash: "abc", Size: 3}}}

	first, err := store.Add(context.Background(), snap)
	if err != nil {
		t.Fatalf("first Add() error = %v", err)
	}
	second, err := store.Add(context.Background(), snap)
	if err != nil {
		t.Fatalf("second Add() error = %v", err)
	}
	if first.Status != AddStatusCreated {
		t.Fatalf("first status = %s", first.Status)
	}
	if second.Status != AddStatusAlreadyExists {
		t.Fatalf("second status = %s", second.Status)
	}
	all, err := store.List(context.Background(), ListFilter{})
	if err != nil {
		t.Fatalf("List() error = %v", err)
	}
	if len(all) != 1 {
		t.Fatalf("len(all) = %d, want 1", len(all))
	}
}

func TestFileStoreAddRejectsConflictingReplay(t *testing.T) {
	store := NewFileStore(t.TempDir())
	snap := Snapshot{SnapshotID: "snap-1", Tenant: "acme", Worktree: "api", TreeID: "tree-1", Branch: "main", Objects: []ObjectRef{{Path: "a.rs", Hash: "abc", Size: 3}}}
	conflict := Snapshot{SnapshotID: "snap-1", Tenant: "acme", Worktree: "api", TreeID: "tree-1", Branch: "main", Objects: []ObjectRef{{Path: "a.rs", Hash: "def", Size: 3}}}

	if _, err := store.Add(context.Background(), snap); err != nil {
		t.Fatalf("Add() error = %v", err)
	}
	if _, err := store.Add(context.Background(), conflict); !errors.Is(err, ErrConflict) {
		t.Fatalf("conflicting Add() error = %v, want ErrConflict", err)
	}
}

func TestIsValidRelativePath(t *testing.T) {
	valid := []string{"src/lib.rs", "README.md", "dir/sub/file.txt"}
	for _, path := range valid {
		if !IsValidRelativePath(path) {
			t.Fatalf("path %q should be valid", path)
		}
	}
	invalid := []string{"", "../secret", "src/../../secret", "/absolute/path"}
	for _, path := range invalid {
		if IsValidRelativePath(path) {
			t.Fatalf("path %q should be invalid", path)
		}
	}
}
