package syncsvc

import (
	"context"
	"errors"

	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/objstore"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
)

// Pull returns the snapshots of a branch that the client is missing.
//
// The branch is branch_id.value (the tree's default branch when empty).
// Snapshots reachable from the server tip but not from current_tip are
// returned oldest first; a positive depth keeps only the newest `depth` of
// them (shallow pull). objects_to_fetch lists, deduplicated, the manifest
// and blob hashes referenced by the returned snapshots; the client fetches
// those it lacks with DownloadObjects.
//
// A current_tip the server does not know (e.g. local-only snapshots) is
// ignored, so the full history is sent. Pulling the default branch of an
// empty tree yields has_updates=false; any other unknown branch is
// NotFound.
func (s *Service) Pull(ctx context.Context, req *syncpb.PullRequest) (*syncpb.PullResponse, error) {
	t, _, err := s.readableTree(ctx, req.GetTreeId())
	if err != nil {
		return nil, err
	}
	branch := req.GetBranchId().GetValue()
	if branch == "" {
		branch = t.DefaultBranch
	}
	if err := validateRefName("branch", branch); err != nil {
		return nil, err
	}
	current := req.GetCurrentTip().GetValue()
	if current != "" && !validID(current) {
		return nil, invalidf("current_tip is malformed")
	}

	b, err := s.d.Store.GetBranch(ctx, t.ID, branch)
	if errors.Is(err, domain.ErrNotFound) {
		if branch == t.DefaultBranch {
			return &syncpb.PullResponse{HasUpdates: false}, nil
		}
		return nil, status.Error(codes.NotFound, "branch: not found")
	}
	if err != nil {
		return nil, s.toStatus(ctx, err, "branch")
	}
	tip := b.Tip
	resp := &syncpb.PullResponse{NewTip: snapshotIDToPB(tip)}
	if tip == "" || tip == current {
		return resp, nil
	}

	var haves []string
	if current != "" {
		snap, err := s.d.Store.GetSnapshot(ctx, current)
		switch {
		case err == nil && snap.TreeID == t.ID:
			haves = []string{current}
		case err == nil, errors.Is(err, domain.ErrNotFound):
			// Unknown to this tree: send everything.
		default:
			return nil, s.toStatus(ctx, err, "snapshot")
		}
	}

	missing, err := s.d.History.Missing(ctx, tip, haves)
	if err != nil {
		return nil, s.toStatus(ctx, err, "history")
	}
	if d := req.GetDepth(); d > 0 && len(missing) > int(d) {
		missing = missing[len(missing)-int(d):]
	}
	if len(missing) == 0 {
		return resp, nil
	}

	resp.HasUpdates = true
	resp.SnapshotChain = make([]*syncpb.SnapshotId, 0, len(missing))
	resp.Snapshots = make([]*syncpb.SnapshotRecord, 0, len(missing))
	seen := make(map[hash.Hash]struct{})
	var objects []hash.Hash
	add := func(h hash.Hash) {
		if _, ok := seen[h]; !ok {
			seen[h] = struct{}{}
			objects = append(objects, h)
		}
	}
	manifestsRead := make(map[hash.Hash]struct{})
	for i := range missing {
		snap := &missing[i]
		resp.SnapshotChain = append(resp.SnapshotChain, &syncpb.SnapshotId{Value: snap.ID})
		resp.Snapshots = append(resp.Snapshots, snapshotToPB(snap))
		mh := snap.ManifestHash
		add(mh)
		if _, done := manifestsRead[mh]; done {
			continue
		}
		manifestsRead[mh] = struct{}{}
		m, err := s.loadManifest(ctx, mh)
		if err != nil {
			if errors.Is(err, objstore.ErrNotFound) || errors.Is(err, errInvalidManifest) {
				s.log.ErrorContext(ctx, "pull: stored snapshot has unreadable manifest",
					"tree", t.ID, "snapshot", snap.ID, "manifest", mh.Hex(), "err", err)
				return nil, status.Error(codes.Internal, "object store is inconsistent: a snapshot manifest is unavailable")
			}
			return nil, s.toStatus(ctx, err, "read manifest")
		}
		for _, e := range m.GetEntries() {
			add(hash.Hash(e.GetBlobHash().GetValue()))
		}
	}
	resp.ObjectsToFetch = hashesToPB(objects)
	return resp, nil
}
