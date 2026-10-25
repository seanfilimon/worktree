package syncsvc

import (
	"context"
	"fmt"
	"testing"

	"google.golang.org/protobuf/proto"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
)

// review-ci (§13.4): PushTag enforces tag protections through tagPolicy.

func (f *fakeStore) GetTag(_ context.Context, treeID, name string) (*domain.Tag, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	t, ok := f.tags[key(treeID, name)]
	if !ok {
		return nil, fmt.Errorf("tag %s: %w", name, domain.ErrNotFound)
	}
	c := *t
	return &c, nil
}

// tagReview is a review fake that also implements tagPolicy.
type tagReview struct {
	*fakeReview
	calls []string
	deny  map[string]string // "name/create|move" -> reason
}

func (r *tagReview) CheckTagPush(_ context.Context, _ *domain.Principal, _ *domain.Tree, name string, replace bool) error {
	op := "create"
	if replace {
		op = "move"
	}
	r.calls = append(r.calls, name+"/"+op)
	if reason, ok := r.deny[name+"/"+op]; ok {
		return &domain.PushDenied{Kind: "tag_protection", Reason: reason}
	}
	return nil
}

func TestPushTagProtections(t *testing.T) {
	h := newHarness(t)
	ctx := h.as("alice")
	tree := h.newTree("web")
	c1 := makeCommit(tree, map[string]string{"a": "1"})
	h.upload(ctx, c1)
	h.mustPush(ctx, pushReq(tree, "main", "", c1))
	rv := &tagReview{fakeReview: h.review, deny: map[string]string{
		"v1/move":    "tag 'v1' is protected and cannot be moved",
		"rel/create": "alice may not create it",
	}}
	h.svc.d.Review = rv

	tagReq := func(name string) *syncpb.PushTagRequest {
		return &syncpb.PushTagRequest{TreeId: &syncpb.TreeId{Value: tree}, Tag: &syncpb.TagRecord{
			Name: name, Target: &syncpb.SnapshotId{Value: c1.id}, Message: proto.String("m"),
		}}
	}
	if resp, err := h.client.PushTag(ctx, tagReq("v1")); err != nil || !resp.GetAccepted() {
		t.Fatalf("first push = %v, %v", resp, err)
	}
	resp, err := h.client.PushTag(ctx, tagReq("v1"))
	if err != nil || resp.GetAccepted() || resp.GetError() != "tag 'v1' is protected and cannot be moved" {
		t.Fatalf("move = %v, %v", resp, err)
	}
	resp, err = h.client.PushTag(ctx, tagReq("rel"))
	if err != nil || resp.GetAccepted() || resp.GetError() != "alice may not create it" {
		t.Fatalf("create denied = %v, %v", resp, err)
	}
	if fmt.Sprint(rv.calls) != "[v1/create v1/move rel/create]" {
		t.Fatalf("calls = %v", rv.calls)
	}
	if len(h.bus.ofType(domain.EventTagPushed)) != 1 {
		t.Fatalf("tag.pushed events = %d", len(h.bus.ofType(domain.EventTagPushed)))
	}
}
