package grpcserver

import (
	"context"
	"encoding/hex"
	"errors"
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
		authorizer = iam.NewDefaultPolicyAuthorizer()
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

	principal, ok := auth.PrincipalFromContext(ctx)
	if !ok || !principal.Authenticated {
		return nil, status.Error(codes.Unauthenticated, "authentication required")
	}
	resource := fmt.Sprintf("%s/%s/%s/%s", req.Tenant, req.Worktree, req.Branch, req.SnapshotId)
	if principal.Tenant != "" && principal.Tenant != req.Tenant {
		s.record(ctx, "staged:create", audit.DecisionDeny, "tenant mismatch", req.Tenant, principal.Account, resource)
		return nil, status.Error(codes.PermissionDenied, "authenticated tenant does not match staged snapshot tenant")
	}
	decision, err := s.authorizer.Authorize(ctx, principal, "staged:create", resource)
	if err != nil || decision == iam.Deny {
		s.record(ctx, "staged:create", audit.DecisionDeny, "iam denied", req.Tenant, principal.Account, resource)
		return nil, status.Error(codes.PermissionDenied, "access denied")
	}

	objectIDs := make([]string, 0, len(req.Objects))
	objectRefs := make([]stagedpkg.ObjectRef, 0, len(req.Objects))
	for _, obj := range req.Objects {
		if !stagedpkg.IsValidRelativePath(obj.Path) {
			return nil, status.Errorf(codes.InvalidArgument, "object path %q must be relative and must not traverse parents", obj.Path)
		}
		if !storage.IsValidHash(obj.Hash) {
			return nil, status.Errorf(codes.InvalidArgument, "object hash %q is not a 64-character BLAKE3 hex digest", obj.Hash)
		}
		if obj.Size < 0 {
			return nil, status.Errorf(codes.InvalidArgument, "object %q: size must be non-negative", obj.Path)
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
		objectRefs = append(objectRefs, stagedpkg.ObjectRef{Path: obj.Path, Hash: obj.Hash, Size: int(obj.Size)})
	}

	snap := stagedpkg.Snapshot{
		SnapshotID: req.SnapshotId,
		Tenant:     req.Tenant,
		Worktree:   req.Worktree,
		TreeID:     req.TreeId,
		Branch:     req.Branch,
		ObjectIDs:  objectIDs,
		Objects:    objectRefs,
		CreatedAt:  time.Now().UTC(),
	}
	if _, err := s.staged.Add(ctx, snap); err != nil {
		if errors.Is(err, stagedpkg.ErrConflict) {
			s.record(ctx, "staged:create", audit.DecisionDeny, "staged idempotency conflict", req.Tenant, principal.Account, resource)
			return nil, status.Error(codes.AlreadyExists, "staged snapshot conflicts with an existing snapshot for the same identity")
		}
		s.record(ctx, "staged:create", audit.DecisionDeny, "staged persistence failed", req.Tenant, principal.Account, resource)
		return nil, status.Errorf(codes.Internal, "failed to persist staged snapshot: %v", err)
	}

	s.record(ctx, "staged:create", audit.DecisionAllow, "", req.Tenant, principal.Account, resource)
	return &worktreepb.StageSnapshotResponse{
		Status:     "staged",
		SnapshotId: req.SnapshotId,
		Objects:    int32(len(objectIDs)),
	}, nil
}

// ListStagedSnapshots returns staged snapshots filtered by tenant, worktree, and/or branch.
func (s *SyncServer) ListStagedSnapshots(ctx context.Context, req *worktreepb.ListStagedSnapshotsRequest) (*worktreepb.ListStagedSnapshotsResponse, error) {
	principal, ok := auth.PrincipalFromContext(ctx)
	if !ok || !principal.Authenticated {
		return nil, status.Error(codes.Unauthenticated, "authentication required")
	}
	if principal.Tenant != "" {
		if req.Tenant != "" && req.Tenant != principal.Tenant {
			s.record(ctx, "staged:list", audit.DecisionDeny, "tenant mismatch", req.Tenant, principal.Account, "staged")
			return nil, status.Error(codes.PermissionDenied, "authenticated tenant does not match requested tenant")
		}
		req.Tenant = principal.Tenant
	}
	resource := "staged"
	if req.Tenant != "" {
		resource = fmt.Sprintf("%s/%s/%s/*", req.Tenant, req.Worktree, req.Branch)
	}
	decision, err := s.authorizer.Authorize(ctx, principal, "staged:list", resource)
	if err != nil || decision == iam.Deny {
		s.record(ctx, "staged:list", audit.DecisionDeny, "iam denied", req.Tenant, principal.Account, resource)
		return nil, status.Error(codes.PermissionDenied, "access denied")
	}

	snaps, err := s.staged.List(ctx, stagedpkg.ListFilter{
		Tenant:   req.Tenant,
		Worktree: req.Worktree,
		Branch:   req.Branch,
	})
	if err != nil {
		s.record(ctx, "staged:list", audit.DecisionDeny, "list failed", req.Tenant, principal.Account, resource)
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
	s.record(ctx, "staged:list", audit.DecisionAllow, "", req.Tenant, principal.Account, resource)
	return &worktreepb.ListStagedSnapshotsResponse{
		Snapshots: records,
		Count:     int32(len(records)),
	}, nil
}

func (s *SyncServer) record(ctx context.Context, action string, decision audit.Decision, reason, tenant, account, resource string) {
	principal, _ := auth.PrincipalFromContext(ctx)
	_ = s.audit.Record(ctx, audit.Event{
		Event:      "access_decision",
		Action:     action,
		Decision:   decision,
		Reason:     reason,
		Tenant:     tenant,
		Account:    account,
		TokenID:    principal.TokenID,
		AuthMethod: principal.AuthMethod,
		Resource:   resource,
	})
}
