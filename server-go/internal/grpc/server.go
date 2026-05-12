package grpcserver

import (
	"context"
	"encoding/hex"
	"fmt"
	"time"

	"github.com/zeebo/blake3"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"

	"github.com/ramizik/worktree/server-go/internal/audit"
	"github.com/ramizik/worktree/server-go/internal/auth"
	worktreepb "github.com/ramizik/worktree/server-go/internal/grpc/worktreepb/worktree/v1"
	"github.com/ramizik/worktree/server-go/internal/iam"
	stagedpkg "github.com/ramizik/worktree/server-go/internal/staged"
	"github.com/ramizik/worktree/server-go/internal/storage"
)

// SyncServer implements the gRPC SyncService. It shares the same object store,
// staged store, audit recorder, and IAM authorizer as the REST handlers.
type SyncServer struct {
	worktreepb.UnimplementedSyncServiceServer
	objects    storage.ObjectStore
	staged     stagedpkg.Store
	audit      audit.Recorder
	authorizer iam.Authorizer
}

// NewSyncServer constructs a SyncServer. Nil recorder and authorizer fall back to
// safe no-op/allow-all defaults.
func NewSyncServer(objects storage.ObjectStore, staged stagedpkg.Store, recorder audit.Recorder, authorizer iam.Authorizer) *SyncServer {
	if recorder == nil {
		recorder = audit.NoopRecorder{}
	}
	if authorizer == nil {
		authorizer = iam.AllowAllAuthorizer{}
	}
	return &SyncServer{objects: objects, staged: staged, audit: recorder, authorizer: authorizer}
}

// StageSnapshot validates and stores all objects in the snapshot, then persists
// the staged snapshot record so teammates can see it before `wt push`.
func (s *SyncServer) StageSnapshot(ctx context.Context, req *worktreepb.StageSnapshotRequest) (*worktreepb.StageSnapshotResponse, error) {
	if req.SnapshotId == "" || req.Tenant == "" || req.Worktree == "" || req.TreeId == "" || req.Branch == "" {
		return nil, status.Error(codes.InvalidArgument, "snapshot_id, tenant, worktree, tree_id, branch are required")
	}
	if len(req.Objects) == 0 {
		return nil, status.Error(codes.InvalidArgument, "at least one object is required")
	}

	principal := auth.Principal{}
	resource := fmt.Sprintf("%s/%s/%s/%s", req.Tenant, req.Worktree, req.Branch, req.SnapshotId)
	decision, err := s.authorizer.Authorize(ctx, principal, "staged:create", resource)
	if err != nil || decision == iam.Deny {
		s.record(ctx, "staged:create", audit.DecisionDeny, "iam denied", req.Tenant, "", resource)
		return nil, status.Error(codes.PermissionDenied, "access denied")
	}

	objectIDs := make([]string, 0, len(req.Objects))
	for _, obj := range req.Objects {
		if !storage.IsValidHash(obj.Hash) {
			return nil, status.Errorf(codes.InvalidArgument, "object hash %q is not a 64-character BLAKE3 hex digest", obj.Hash)
		}
		if int64(len(obj.Content)) != obj.Size {
			return nil, status.Errorf(codes.InvalidArgument, "object %q: content length %d does not match size %d", obj.Path, len(obj.Content), obj.Size)
		}
		actual := blake3.Sum256(obj.Content)
		if hex.EncodeToString(actual[:]) != obj.Hash {
			return nil, status.Errorf(codes.InvalidArgument, "object %q: BLAKE3 hash mismatch", obj.Path)
		}
		if err := s.objects.Put(ctx, obj.Hash, obj.Content); err != nil {
			s.record(ctx, "staged:create", audit.DecisionDeny, "object store failed", req.Tenant, "", resource)
			return nil, status.Errorf(codes.Internal, "failed to store object: %v", err)
		}
		objectIDs = append(objectIDs, obj.Hash)
	}

	snap := stagedpkg.Snapshot{
		SnapshotID: req.SnapshotId,
		Tenant:     req.Tenant,
		Worktree:   req.Worktree,
		TreeID:     req.TreeId,
		Branch:     req.Branch,
		ObjectIDs:  objectIDs,
		CreatedAt:  time.Now().UTC(),
	}
	if err := s.staged.Add(ctx, snap); err != nil {
		s.record(ctx, "staged:create", audit.DecisionDeny, "staged persistence failed", req.Tenant, "", resource)
		return nil, status.Errorf(codes.Internal, "failed to persist staged snapshot: %v", err)
	}

	s.record(ctx, "staged:create", audit.DecisionAllow, "", req.Tenant, "", resource)
	return &worktreepb.StageSnapshotResponse{
		Status:     "staged",
		SnapshotId: req.SnapshotId,
		Objects:    int32(len(objectIDs)),
	}, nil
}

// ListStagedSnapshots returns staged snapshots filtered by tenant, worktree, and/or branch.
func (s *SyncServer) ListStagedSnapshots(ctx context.Context, req *worktreepb.ListStagedSnapshotsRequest) (*worktreepb.ListStagedSnapshotsResponse, error) {
	decision, err := s.authorizer.Authorize(ctx, auth.Principal{}, "staged:list", "staged")
	if err != nil || decision == iam.Deny {
		return nil, status.Error(codes.PermissionDenied, "access denied")
	}

	snaps, err := s.staged.List(ctx, stagedpkg.ListFilter{
		Tenant:   req.Tenant,
		Worktree: req.Worktree,
		Branch:   req.Branch,
	})
	if err != nil {
		return nil, status.Errorf(codes.Internal, "list failed: %v", err)
	}

	records := make([]*worktreepb.StagedSnapshotRecord, 0, len(snaps))
	for _, snap := range snaps {
		records = append(records, &worktreepb.StagedSnapshotRecord{
			SnapshotId: snap.SnapshotID,
			Tenant:     snap.Tenant,
			Worktree:   snap.Worktree,
			TreeId:     snap.TreeID,
			Branch:     snap.Branch,
			ObjectIds:  snap.ObjectIDs,
			CreatedAt:  snap.CreatedAt.Format(time.RFC3339),
		})
	}
	return &worktreepb.ListStagedSnapshotsResponse{
		Snapshots: records,
		Count:     int32(len(records)),
	}, nil
}

func (s *SyncServer) record(ctx context.Context, action string, decision audit.Decision, reason, tenant, account, resource string) {
	_ = s.audit.Record(ctx, audit.Event{
		Event:    "access_decision",
		Action:   action,
		Decision: decision,
		Reason:   reason,
		Tenant:   tenant,
		Account:  account,
		Resource: resource,
	})
}
