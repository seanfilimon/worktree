package syncsvc

import (
	"testing"

	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/metadata"

	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
)

// TestStagedManifestIsRecordedAndDownloadable: a staged WIP that names its
// (uploaded) manifest is listed with it, and teammates who can read the
// tree may download its objects although no branch references them.
func TestStagedManifestIsRecordedAndDownloadable(t *testing.T) {
	h := newHarness(t)
	ctx := h.as("alice")
	tree := h.newTree("web")
	wip := makeCommit(tree, map[string]string{"wip.txt": "work in progress"})
	h.upload(ctx, wip)

	resp, err := h.client.Stage(ctx, &syncpb.StageUploadRequest{
		TreeId:       &syncpb.TreeId{Value: tree},
		BranchName:   "main",
		SnapshotId:   &syncpb.SnapshotId{Value: wip.id},
		FilesChanged: []string{"wip.txt"},
		FilesAdded:   1,
		ManifestHash: hashToPB(wip.manifest),
	})
	if err != nil || !resp.GetAccepted() {
		t.Fatalf("stage = %v, %v", resp, err)
	}
	list, err := h.client.ListStaged(h.as("bob"), &syncpb.ListStagedRequest{TreeId: &syncpb.TreeId{Value: tree}})
	if err != nil || len(list.GetStaged()) != 1 {
		t.Fatalf("list staged = %v, %v", list, err)
	}
	if got := hash.Hash(list.GetStaged()[0].GetManifestHash().GetValue()); got != wip.manifest {
		t.Fatalf("listed manifest = %s, want %s", got.Hex(), wip.manifest.Hex())
	}

	want := make([]*syncpb.ContentHash, 0, len(wip.objects))
	for _, o := range wip.objects {
		want = append(want, o.GetHash())
	}
	got := h.download(t, metadata.AppendToOutgoingContext(h.as("bob"), TreeIDMetadataKey, tree), want)
	if len(got) != len(want) {
		t.Fatalf("downloaded %d of %d staged objects", len(got), len(want))
	}

	// A manifest that was never uploaded is refused.
	missing := hash.Sum([]byte("not uploaded"))
	_, err = h.client.Stage(ctx, &syncpb.StageUploadRequest{
		TreeId:       &syncpb.TreeId{Value: tree},
		BranchName:   "main",
		SnapshotId:   &syncpb.SnapshotId{Value: newID()},
		ManifestHash: hashToPB(missing),
	})
	wantCode(t, err, codes.FailedPrecondition)
}
