package syncsvc

// Incremental object checks of Push (hardening, docs/ENGINEERING_PLAN.md
// §15.10): blobs of a parent that is a branch tip are not re-checked, every
// other referenced object still is.

import (
	"context"
	"strings"
	"testing"

	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"

	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
)

// uploadExcept uploads the objects of c except those with content in skip.
func (h *harness) uploadExcept(ctx context.Context, c commit, skip ...string) {
	h.t.Helper()
	drop := map[hash.Hash]bool{}
	for _, s := range skip {
		drop[hash.Sum([]byte(s))] = true
	}
	var objs []*syncpb.ObjectData
	for _, o := range c.objects {
		if !drop[hash.Hash(o.GetHash().GetValue())] {
			objs = append(objs, o)
		}
	}
	if _, err := h.uploadObjects(ctx, objs...); err != nil {
		h.t.Fatalf("upload: %v", err)
	}
}

func TestPushOnTipStillChecksNewObjects(t *testing.T) {
	h := newHarness(t)
	ctx := h.as("alice")
	treeID := h.newTree("repo")
	c1 := makeCommit(treeID, map[string]string{"a.txt": "one", "b.txt": "two"})
	h.upload(ctx, c1)
	h.mustPush(ctx, pushReq(treeID, "main", "", c1))

	// c2 changes b and adds c; c's blob is never uploaded.
	c2 := makeCommit(treeID, map[string]string{"a.txt": "one", "b.txt": "two v2", "c.txt": "three"}, c1)
	h.uploadExcept(ctx, c2, "three")
	_, err := h.client.Push(ctx, pushReq(treeID, "main", c1.id, c2))
	wantCode(t, err, codes.FailedPrecondition)
	if want := hash.Sum([]byte("three")).Hex(); !strings.Contains(status.Convert(err).Message(), want) {
		t.Fatalf("error does not name the missing blob %s: %v", want, err)
	}
	if got := h.tip(treeID, "main"); got != c1.id {
		t.Fatalf("tip moved to %s", got)
	}

	// Uploading it makes the same push succeed.
	h.upload(ctx, c2)
	h.mustPush(ctx, pushReq(treeID, "main", c1.id, c2))
}

func TestPushTrustsOnlyBranchTipParents(t *testing.T) {
	h := newHarness(t)
	ctx := h.as("alice")
	treeID := h.newTree("repo")
	c1 := makeCommit(treeID, map[string]string{"a.txt": "one", "b.txt": "two"})
	h.upload(ctx, c1)
	h.mustPush(ctx, pushReq(treeID, "main", "", c1))
	c2 := makeCommit(treeID, map[string]string{"a.txt": "one", "b.txt": "two v2"}, c1)
	h.upload(ctx, c2)
	h.mustPush(ctx, pushReq(treeID, "main", c1.id, c2))

	// The blob of a.txt disappears from the store (e.g. corruption).
	if err := h.objects.Delete(context.Background(), hash.Sum([]byte("one"))); err != nil {
		t.Fatal(err)
	}

	// A branch from c1, which is no longer a tip: a.txt is checked and
	// found missing.
	f1 := makeCommit(treeID, map[string]string{"a.txt": "one", "b.txt": "feature"}, c1)
	h.uploadExcept(ctx, f1, "one")
	_, err := h.client.Push(ctx, pushReq(treeID, "feature", "", f1))
	wantCode(t, err, codes.FailedPrecondition)

	// On top of the tip c2, unchanged a.txt is trusted (the tip was
	// accepted with it present), so only the new blob is checked.
	c3 := makeCommit(treeID, map[string]string{"a.txt": "one", "b.txt": "two v3"}, c2)
	h.uploadExcept(ctx, c3, "one")
	h.mustPush(ctx, pushReq(treeID, "main", c2.id, c3))
	if got := h.tip(treeID, "main"); got != c3.id {
		t.Fatalf("tip = %s, want %s", got, c3.id)
	}
}
