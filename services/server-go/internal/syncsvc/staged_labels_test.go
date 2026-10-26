package syncsvc

// staged-v2 (§16.8): StageUploadRequest.label reaches the staged service
// and StagedInfo.label carries it back (slot semantics are tested in
// internal/staged).

import (
	"testing"

	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
)

func TestStageLabelOverGRPC(t *testing.T) {
	h := newHarness(t)
	ctx := h.as("alice")
	tree := h.newTree("web")
	resp, err := h.client.Stage(ctx, &syncpb.StageUploadRequest{
		TreeId:     &syncpb.TreeId{Value: tree},
		BranchName: "main",
		SnapshotId: &syncpb.SnapshotId{Value: newID()},
		Label:      "exp",
	})
	if err != nil || !resp.GetAccepted() {
		t.Fatalf("stage = %v, %v", resp, err)
	}
	list, err := h.client.ListStaged(h.as("bob"), &syncpb.ListStagedRequest{TreeId: &syncpb.TreeId{Value: tree}})
	if err != nil || len(list.GetStaged()) != 1 {
		t.Fatalf("list = %v, %v", list, err)
	}
	if got := list.GetStaged()[0].GetLabel(); got != "exp" {
		t.Fatalf("label = %q", got)
	}
}
