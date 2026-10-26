package review

// v0.8 forks (§16.2): merge requests whose source branch lives in a fork.

import (
	"context"
	"errors"
	"testing"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/fork/snapcopy"
)

// forkEnv is a merge env plus a fork of its tree in another tenant.
type forkEnv struct {
	*menv
	up, fork *domain.Tree
	base     string // upstream main tip at fork time
	feat     string // fork feat tip (a fork snapshot)
}

func newForkEnv(t *testing.T) *forkEnv {
	t.Helper()
	ctx := context.Background()
	e := newMergeEnv(t)
	up := e.tree
	base := e.commit(t, map[string]string{"README.md": "# proj\n", "a.txt": "a\n"})
	e.branch(t, "main", base)

	fork := &domain.Tree{ID: "tree-fork", TenantID: "ten-bob", Name: "proj", DefaultBranch: "main", ForkOf: up.ID}
	if err := e.store.CreateTree(ctx, fork); err != nil {
		t.Fatal(err)
	}
	copied, err := snapcopy.Copy(ctx, e.store, up.ID, fork.ID, base)
	if err != nil {
		t.Fatal(err)
	}
	// Commit in the fork: e.commit writes snapshots of e.tree.
	e.tree = fork
	e.branch(t, "main", copied)
	feat := e.commit(t, map[string]string{"README.md": "# proj\n", "a.txt": "a\n", "fork.txt": "from the fork\n"}, copied)
	e.branch(t, "feat", feat)
	e.tree = up
	return &forkEnv{menv: e, up: up, fork: fork, base: base, feat: feat}
}

func (e *forkEnv) openFromFork(t *testing.T, p *domain.Principal) (*domain.MergeRequest, error) {
	t.Helper()
	return e.svc.OpenWith(context.Background(), p, e.up, OpenInput{
		Title: "From fork", Source: "feat", Target: "main", SourceTree: e.fork,
	})
}

func TestForkMergeRequestOpenAndMerge(t *testing.T) {
	ctx := context.Background()
	e := newForkEnv(t)

	mr, err := e.openFromFork(t, alice)
	if err != nil {
		t.Fatalf("open from fork: %v", err)
	}
	if mr.SourceTreeID != e.fork.ID || mr.SourceBranch != "feat" || mr.TreeID != e.up.ID {
		t.Fatalf("merge request = %+v", mr)
	}
	// A second identical request is refused; the same branch name of the
	// upstream itself is another source.
	if _, err := e.openFromFork(t, alice); !errors.Is(err, domain.ErrAlreadyExists) {
		t.Fatalf("duplicate open: %v, want ErrAlreadyExists", err)
	}

	src, err := e.svc.MergeRequestSource(ctx, alice, e.up, mr.Number)
	if err != nil {
		t.Fatal(err)
	}
	want := snapcopy.DerivedID(e.up.ID, e.feat)
	if !src.CrossTree || src.TreeID != e.fork.ID || src.Tip != e.feat || src.TargetTip != want || src.Tree != "proj" || src.Branch != "feat" {
		t.Fatalf("source = %+v (want target tip %s)", src, want)
	}
	if s, err := e.store.GetSnapshot(ctx, want); err != nil || s.TreeID != e.up.ID {
		t.Fatalf("copied source tip = %+v, %v", s, err)
	}

	m, err := e.svc.Mergeability(ctx, alice, e.up, mr.Number)
	if err != nil {
		t.Fatal(err)
	}
	if !m.FastForward {
		t.Fatalf("mergeability = %+v, want fast-forward", m)
	}

	// Approvals record the copied tip; the merge needs upstream rights.
	if _, err := e.svc.Approve(ctx, carol, e.up, mr.Number); err != nil {
		t.Fatal(err)
	}
	if _, err := e.svc.Merge(ctx, bob, e.up, mr.Number); !errors.Is(err, domain.ErrForbidden) {
		t.Fatalf("developer merge: %v, want ErrForbidden", err)
	}
	merged, err := e.svc.Merge(ctx, carol, e.up, mr.Number)
	if err != nil {
		t.Fatalf("merge: %v", err)
	}
	if merged.State != domain.MRStateMerged || e.tip(t, "main") != want {
		t.Fatalf("merged = %+v, main = %s, want %s", merged, e.tip(t, "main"), want)
	}
	if got := e.files(t, e.tip(t, "main"))["fork.txt"]; got != "from the fork\n" {
		t.Fatalf("fork.txt = %q", got)
	}
	// Copying the merged tip back into the fork yields the fork's own id.
	back, err := snapcopy.Copy(ctx, e.store, e.up.ID, e.fork.ID, want)
	if err != nil || back != e.feat {
		t.Fatalf("copy back = %s, %v; want %s", back, err, e.feat)
	}
}

func TestForkMergeRequestServerMerge(t *testing.T) {
	ctx := context.Background()
	e := newForkEnv(t)
	// The upstream moves on: the fork's request needs a merge snapshot.
	next := e.commit(t, map[string]string{"README.md": "# proj\n", "a.txt": "a2\n"}, e.base)
	e.branch(t, "main", next)
	mr, err := e.openFromFork(t, alice)
	if err != nil {
		t.Fatal(err)
	}
	merged, err := e.svc.MergeWith(ctx, carol, e.up, mr.Number, MergeOptions{Method: domain.MergeCommit})
	if err != nil {
		t.Fatalf("merge: %v", err)
	}
	files := e.files(t, merged.MergedSnapshot)
	if files["a.txt"] != "a2\n" || files["fork.txt"] != "from the fork\n" {
		t.Fatalf("merged files = %v", files)
	}
	s, err := e.store.GetSnapshot(ctx, merged.MergedSnapshot)
	if err != nil || s.TreeID != e.up.ID || len(s.Parents) != 2 || s.Parents[1] != snapcopy.DerivedID(e.up.ID, e.feat) {
		t.Fatalf("merge snapshot = %+v, %v", s, err)
	}
}

func TestForkMergeRequestAuthorization(t *testing.T) {
	ctx := context.Background()
	e := newForkEnv(t)

	// eve can only read: neither mr:create nor a contributor of the fork.
	if _, err := e.openFromFork(t, eve); !errors.Is(err, domain.ErrForbidden) {
		t.Fatalf("eve open: %v, want ErrForbidden", err)
	}
	// With tree:fork upstream and tree:write on the fork she may open it.
	e.svc.authz = &fakeAuthz{grants: map[string][]domain.Action{
		"eve":   {domain.ActTreeRead, domain.ActTreeFork, domain.ActTreeWrite},
		"carol": maintainer,
	}}
	mr, err := e.openFromFork(t, eve)
	if err != nil {
		t.Fatalf("contributor open: %v", err)
	}
	if mr.Author != "eve" || mr.SourceTreeID != e.fork.ID {
		t.Fatalf("merge request = %+v", mr)
	}

	// A tree outside the fork network is refused.
	other := &domain.Tree{ID: "tree-other", TenantID: "ten-bob", Name: "other", DefaultBranch: "main"}
	if err := e.store.CreateTree(ctx, other); err != nil {
		t.Fatal(err)
	}
	e.store.Memory.PutBranch(ctx, &domain.Branch{TreeID: other.ID, Name: "feat", Tip: e.feat}) //nolint:errcheck // checked by the open below
	_, err = e.svc.OpenWith(ctx, carol, e.up, OpenInput{Title: "x", Source: "feat", Target: "main", SourceTree: other})
	if !errors.Is(err, domain.ErrInvalid) {
		t.Fatalf("unrelated tree: %v, want ErrInvalid", err)
	}

	// Suggestions cannot be applied to a fork's branch from the upstream.
	if _, err := e.svc.ApplySuggestion(ctx, carol, e.up, mr.Number, "c1", ""); !errors.Is(err, domain.ErrConflict) {
		t.Fatalf("apply suggestion: %v, want ErrConflict", err)
	}
}

func TestForkMergeRequestMissingSource(t *testing.T) {
	ctx := context.Background()
	e := newForkEnv(t)
	mr, err := e.openFromFork(t, alice)
	if err != nil {
		t.Fatal(err)
	}
	if err := e.store.DeleteTree(ctx, e.fork.ID); err != nil {
		t.Fatal(err)
	}
	src, err := e.svc.MergeRequestSource(ctx, alice, e.up, mr.Number)
	if err != nil || !src.Missing || !src.CrossTree {
		t.Fatalf("source = %+v, %v; want missing", src, err)
	}
	if _, err := e.svc.Merge(ctx, carol, e.up, mr.Number); err == nil {
		t.Fatal("merging a request whose fork was deleted must fail")
	}
}

func TestForkChainAndNetwork(t *testing.T) {
	ctx := context.Background()
	e := newForkEnv(t)
	// A fork of the fork, and a sibling fork: all in one network.
	grand := &domain.Tree{ID: "tree-grand", TenantID: "ten-dave", Name: "proj", ForkOf: e.fork.ID}
	sib := &domain.Tree{ID: "tree-sib", TenantID: "ten-eve", Name: "proj", ForkOf: e.up.ID}
	for _, tr := range []*domain.Tree{grand, sib} {
		if err := e.store.CreateTree(ctx, tr); err != nil {
			t.Fatal(err)
		}
	}
	if got := e.svc.forkChain(ctx, grand); len(got) != 3 || got[2] != e.up.ID {
		t.Fatalf("chain = %v", got)
	}
	for _, pair := range [][2]*domain.Tree{{e.up, grand}, {grand, e.up}, {sib, grand}, {e.fork, sib}} {
		if err := e.svc.checkNetwork(ctx, pair[0], pair[1]); err != nil {
			t.Errorf("checkNetwork(%s, %s) = %v", pair[0].ID, pair[1].ID, err)
		}
	}
	// A deleted upstream still links its forks.
	gone := &domain.Tree{ID: "tree-x", TenantID: "ten-eve", Name: "x", ForkOf: "tree-deleted"}
	gone2 := &domain.Tree{ID: "tree-y", TenantID: "ten-eve", Name: "y", ForkOf: "tree-deleted"}
	if err := e.svc.checkNetwork(ctx, gone, gone2); err != nil {
		t.Errorf("forks of a deleted upstream: %v", err)
	}
}
