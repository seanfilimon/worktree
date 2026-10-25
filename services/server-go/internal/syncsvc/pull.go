package syncsvc

import (
	"context"
	"errors"
	"fmt"
	"sort"

	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/objstore"
	"github.com/seanfilimon/worktree/services/server-go/internal/pathacl"
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
// Restricted paths (docs/ENGINEERING_PLAN.md §13.1): a path is restricted
// for the caller when the tree's license rules do not let it read the path
// (LicenseChecker.CheckRead) or an access policy explicitly denies it
// tree:read on that path. The blobs of restricted paths are left out of
// objects_to_fetch (unless a readable path shares the same content), and
// the restricted paths of the new tip's manifest are returned, sorted, in
// stubbed_paths: the client keeps them out of its working copy. License
// restrictions are audited as "license.denied" once per pull.
//
// A current_tip the server does not know (e.g. local-only snapshots) is
// ignored, so the full history is sent. Pulling the default branch of an
// empty tree yields has_updates=false; any other unknown branch is
// NotFound.
func (s *Service) Pull(ctx context.Context, req *syncpb.PullRequest) (*syncpb.PullResponse, error) {
	t, p, err := s.readableTree(ctx, req.GetTreeId())
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

	// Load every manifest once, keeping the order in which they appear.
	type loaded struct {
		hash    hash.Hash
		entries []*syncpb.ManifestEntry
	}
	var manifests []loaded
	byHash := make(map[hash.Hash]int)
	tipManifest := -1
	for i := range missing {
		snap := &missing[i]
		resp.SnapshotChain = append(resp.SnapshotChain, &syncpb.SnapshotId{Value: snap.ID})
		resp.Snapshots = append(resp.Snapshots, snapshotToPB(snap))
		mh := snap.ManifestHash
		idx, done := byHash[mh]
		if !done {
			m, err := s.loadManifest(ctx, mh)
			if err != nil {
				if errors.Is(err, objstore.ErrNotFound) || errors.Is(err, errInvalidManifest) {
					s.log.ErrorContext(ctx, "pull: stored snapshot has unreadable manifest",
						"tree", t.ID, "snapshot", snap.ID, "manifest", mh.Hex(), "err", err)
					return nil, status.Error(codes.Internal, "object store is inconsistent: a snapshot manifest is unavailable")
				}
				return nil, s.toStatus(ctx, err, "read manifest")
			}
			idx = len(manifests)
			byHash[mh] = idx
			manifests = append(manifests, loaded{hash: mh, entries: m.GetEntries()})
		}
		if snap.ID == tip || (tipManifest < 0 && i == len(missing)-1) {
			tipManifest = idx
		}
	}

	// Restricted paths across every returned manifest.
	pathSet := make(map[string]struct{})
	var paths []string
	for _, m := range manifests {
		for _, e := range m.entries {
			if _, ok := pathSet[e.GetPath()]; !ok {
				pathSet[e.GetPath()] = struct{}{}
				paths = append(paths, e.GetPath())
			}
		}
	}
	sort.Strings(paths)
	restricted, licensed, err := s.restrictedPaths(ctx, p, t, paths)
	if err != nil {
		return nil, err
	}
	if len(licensed) > 0 {
		s.audit(ctx, p, domain.AuditEvent{
			TenantID:     t.TenantID,
			Action:       "license.denied",
			ResourceType: "tree",
			ResourceID:   t.ID,
			Details: map[string]string{
				"tree":   t.Name,
				"branch": branch,
				"op":     "pull",
				"count":  fmt.Sprint(len(licensed)),
				"path":   licensed[0],
			},
		})
	}

	seen := make(map[hash.Hash]struct{})
	var objects []hash.Hash
	add := func(h hash.Hash) {
		if _, ok := seen[h]; !ok {
			seen[h] = struct{}{}
			objects = append(objects, h)
		}
	}
	for _, m := range manifests {
		add(m.hash)
		for _, e := range m.entries {
			if _, r := restricted[e.GetPath()]; r {
				continue
			}
			add(hash.Hash(e.GetBlobHash().GetValue()))
		}
	}
	resp.ObjectsToFetch = hashesToPB(objects)

	if len(restricted) > 0 && tipManifest >= 0 {
		stubbed := make(map[string]struct{})
		for _, e := range manifests[tipManifest].entries {
			if _, r := restricted[e.GetPath()]; r {
				stubbed[e.GetPath()] = struct{}{}
			}
		}
		for path := range stubbed {
			resp.StubbedPaths = append(resp.StubbedPaths, path)
		}
		sort.Strings(resp.StubbedPaths)
	}
	return resp, nil
}

// unreadableLister is the optional bulk API of the license checker
// (implemented by internal/license.Checker).
type unreadableLister interface {
	UnreadablePaths(ctx context.Context, p *domain.Principal, tree *domain.Tree, paths []string) ([]string, error)
}

// restrictedPaths returns the paths (a subset of paths) p may not read:
// all of them as a set, and the license-restricted ones in input order.
func (s *Service) restrictedPaths(ctx context.Context, p *domain.Principal, t *domain.Tree, paths []string) (map[string]struct{}, []string, error) {
	if len(paths) == 0 {
		return nil, nil, nil
	}
	var licensed []string
	if s.d.License != nil {
		var err error
		if licensed, err = s.licenseUnreadable(ctx, p, t, paths); err != nil {
			return nil, nil, s.toStatus(ctx, err, "license check")
		}
	}
	var denied map[string]string
	if ev, ok := s.d.Authz.(domain.AccessEvaluator); ok {
		var err error
		if denied, err = pathacl.DeniedPaths(ctx, s.d.Store, ev, p, t, domain.ActTreeRead, paths); err != nil {
			return nil, nil, s.toStatus(ctx, err, "path policies")
		}
	}
	if len(licensed) == 0 && len(denied) == 0 {
		return nil, nil, nil
	}
	out := make(map[string]struct{}, len(licensed)+len(denied))
	for _, path := range licensed {
		out[path] = struct{}{}
	}
	for path := range denied {
		out[path] = struct{}{}
	}
	return out, licensed, nil
}

// licenseUnreadable lists the paths the license checker refuses to let p
// read, using its bulk API when available. The common all-readable case
// costs one check.
func (s *Service) licenseUnreadable(ctx context.Context, p *domain.Principal, t *domain.Tree, paths []string) ([]string, error) {
	if u, ok := s.d.License.(unreadableLister); ok {
		return u.UnreadablePaths(ctx, p, t, paths)
	}
	err := s.d.License.CheckRead(ctx, p, t, paths)
	var denied *domain.PushDenied
	if err == nil || !errors.As(err, &denied) {
		return nil, err
	}
	var out []string
	for _, path := range paths {
		err := s.d.License.CheckRead(ctx, p, t, []string{path})
		switch {
		case err == nil:
		case errors.As(err, &denied):
			out = append(out, path)
		default:
			return nil, err
		}
	}
	return out, nil
}
