package snapcopy

import (
	"context"
	"errors"
	"regexp"
	"testing"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
)

var t0 = time.Date(2026, 10, 1, 0, 0, 0, 0, time.UTC)

func put(t *testing.T, st *meta.Memory, id, tree string, parents ...string) {
	t.Helper()
	if err := st.PutSnapshot(context.Background(), &domain.Snapshot{ID: id, TreeID: tree, Parents: parents, ManifestHash: hash.Sum([]byte(id)),
		Author: "a", Message: id, CreatedAt: t0, Signature: []byte("s"), SigningKeyID: "k"}); err != nil {
		t.Fatal(err)
	}
}

func TestDerivedID(t *testing.T) {
	a := DerivedID("tree-1", "snap")
	if a != DerivedID("tree-1", "snap") || a == DerivedID("tree-2", "snap") || a == DerivedID("tree-1", "snap2") {
		t.Fatal("derived ids must be deterministic per (tree, snapshot)")
	}
	if !regexp.MustCompile(`^[0-9a-f]{8}-[0-9a-f]{4}-5[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$`).MatchString(a) {
		t.Fatalf("%s is not a UUID v5", a)
	}
}

func TestCopyRoundTrip(t *testing.T) {
	ctx := context.Background()
	st := meta.NewMemory()
	// up: a <- b <- d, a <- c, d merges b and c; d reverts c.
	put(t, st, "a", "up")
	put(t, st, "b", "up", "a")
	put(t, st, "c", "up", "a")
	put(t, st, "d", "up", "b", "c")

	got, err := Copy(ctx, st, "up", "fork", "d")
	if err != nil {
		t.Fatal(err)
	}
	if got != DerivedID("fork", "d") {
		t.Fatalf("copy of d = %s", got)
	}
	d, err := st.GetSnapshot(ctx, got)
	if err != nil {
		t.Fatal(err)
	}
	if d.TreeID != "fork" || len(d.Parents) != 2 || d.Parents[0] != DerivedID("fork", "b") || d.Parents[1] != DerivedID("fork", "c") ||
		d.Signature != nil || d.ManifestHash != hash.Sum([]byte("d")) || d.Message != "d" {
		t.Fatalf("copy = %+v", d)
	}
	if n, _ := st.CountSnapshots(ctx, "fork"); n != 4 {
		t.Fatalf("fork has %d snapshots", n)
	}
	// Idempotent.
	if again, err := Copy(ctx, st, "up", "fork", "d"); err != nil || again != got {
		t.Fatalf("second copy = %s, %v", again, err)
	}
	// New work in the fork goes back upstream under derived ids, and
	// copied snapshots map back to their originals.
	put(t, st, "e", "fork", got)
	up, err := Copy(ctx, st, "fork", "up", "e")
	if err != nil {
		t.Fatal(err)
	}
	e, _ := st.GetSnapshot(ctx, up)
	if up != DerivedID("up", "e") || e.TreeID != "up" || len(e.Parents) != 1 || e.Parents[0] != "d" {
		t.Fatalf("copy of e upstream = %+v", e)
	}
	if back, err := Copy(ctx, st, "up", "fork", up); err != nil || back != "e" {
		t.Fatalf("copy of e back = %s, %v", back, err)
	}
	// A fork of the fork maps through the chain.
	g, err := Copy(ctx, st, "fork", "grand", "e")
	if err != nil {
		t.Fatal(err)
	}
	if m, err := MappedID(ctx, st, "grand", "up", g); err != nil || m != up {
		t.Fatalf("grand -> up = %s, %v; want %s", m, err, up)
	}
	if m, err := MappedID(ctx, st, "grand", "up", DerivedID("grand", got)); err != nil || m != "d" {
		t.Fatalf("grand copy of d -> up = %s, %v", m, err)
	}
	if id, ok, err := Existing(ctx, st, "up", "fork", "d"); err != nil || !ok || id != got {
		t.Fatalf("Existing = %s %v %v", id, ok, err)
	}
	if _, ok, _ := Existing(ctx, st, "up", "other", "d"); ok {
		t.Fatal("Existing in a tree without copies")
	}
}

func TestCopyRefusesForeignSnapshots(t *testing.T) {
	ctx := context.Background()
	st := meta.NewMemory()
	put(t, st, "x", "elsewhere")
	if _, err := Copy(ctx, st, "up", "fork", "x"); !errors.Is(err, domain.ErrInvalid) {
		t.Fatalf("foreign snapshot: %v", err)
	}
	if _, err := Copy(ctx, st, "up", "fork", "missing"); !errors.Is(err, domain.ErrNotFound) {
		t.Fatalf("missing snapshot: %v", err)
	}
	if got, err := Copy(ctx, st, "up", "fork", ""); got != "" || err != nil {
		t.Fatalf("empty tip = %q, %v", got, err)
	}
}

func TestCopyLongHistory(t *testing.T) {
	ctx := context.Background()
	st := meta.NewMemory()
	prev := ""
	for i := range 5000 {
		id := DerivedID("seed", string(rune('a'+i%26))+time.Duration(i).String())
		if prev == "" {
			put(t, st, id, "up")
		} else {
			put(t, st, id, "up", prev)
		}
		prev = id
	}
	if _, err := Copy(ctx, st, "up", "fork", prev); err != nil {
		t.Fatal(err)
	}
	if n, _ := st.CountSnapshots(ctx, "fork"); n != 5000 {
		t.Fatalf("copied %d", n)
	}
}
