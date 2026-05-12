package canonical

import (
	"context"
	"errors"
	"fmt"
	"time"

	"github.com/ramizik/worktree/server-go/internal/storage"
)

// Service orchestrates canonical push/pull operations over a Store and an
// ObjectStore. It is transport-agnostic: HTTP handlers (and a future gRPC
// surface) call its methods after auth/IAM checks.
type Service struct {
	store   Store
	objects storage.ObjectStore
}

func NewService(store Store, objects storage.ObjectStore) *Service {
	return &Service{store: store, objects: objects}
}

// PushInput is the validated payload of POST /api/push.
type PushInput struct {
	Tenant        string
	Worktree      string
	TreeID        string
	Branch        string
	ExpectedTip   string // empty == "branch has never been pushed"
	NewTip        string
	SnapshotChain []Snapshot // each MUST already have Payload set
	Objects       []ObjectRef
}

// PushResult is returned to the client. Conflict carries the actual tip.
type PushResult struct {
	Status            string
	NewTip            string
	SnapshotsAccepted int
	ActualTip         string   // populated on conflict
	MissingObjects    []string // populated on missing_objects precondition
}

// MissingObjects: pre-flight check that the object set has been uploaded.
// Push refuses to advance the tip if any referenced blob is absent.
func (s *Service) MissingObjects(ctx context.Context, hashes []string) ([]string, error) {
	missing := make([]string, 0)
	for _, h := range hashes {
		if !storage.IsValidHash(h) {
			return nil, fmt.Errorf("invalid hash: %s", h)
		}
		ok, err := s.objects.Exists(ctx, h)
		if err != nil {
			return nil, err
		}
		if !ok {
			missing = append(missing, h)
		}
	}
	return missing, nil
}

// PutObject writes a single blob to ObjectStore. BLAKE3 verification happens
// inside ObjectStore.Put — bytes mismatching the declared hash are rejected.
func (s *Service) PutObject(ctx context.Context, hash string, data []byte) error {
	return s.objects.Put(ctx, hash, data)
}

// GetObject reads a blob. Returns storage.ErrObjectMissing when absent.
func (s *Service) GetObject(ctx context.Context, hash string) ([]byte, error) {
	return s.objects.Get(ctx, hash)
}

// Push promotes a snapshot chain onto the canonical branch tip via CAS.
//
// Order of operations (single logical txn from the client's point of view):
//  1. Verify every referenced object blob already lives in ObjectStore.
//  2. Insert snapshot rows + object refs (idempotent).
//  3. CAS-advance the branch tip.
//
// If step 1 fails: returns Status="missing_objects" with the missing list and
// no rows written. If step 3 fails: returns Status="conflict" with the actual
// current tip; snapshot rows from step 2 remain but are harmless (idempotent
// reuse on a retried push, or sweepable by a janitor).
func (s *Service) Push(ctx context.Context, in PushInput) (PushResult, error) {
	hashes := uniqueObjectHashes(in.SnapshotChain, in.Objects)
	missing, err := s.MissingObjects(ctx, hashes)
	if err != nil {
		return PushResult{}, fmt.Errorf("check objects: %w", err)
	}
	if len(missing) > 0 {
		return PushResult{Status: "missing_objects", MissingObjects: missing}, nil
	}

	now := time.Now().UTC()
	for i := range in.SnapshotChain {
		if in.SnapshotChain[i].CommittedAt.IsZero() {
			in.SnapshotChain[i].CommittedAt = now
		}
		if in.SnapshotChain[i].Tenant == "" {
			in.SnapshotChain[i].Tenant = in.Tenant
		}
		if in.SnapshotChain[i].Worktree == "" {
			in.SnapshotChain[i].Worktree = in.Worktree
		}
		if in.SnapshotChain[i].TreeID == "" {
			in.SnapshotChain[i].TreeID = in.TreeID
		}
		if in.SnapshotChain[i].Branch == "" {
			in.SnapshotChain[i].Branch = in.Branch
		}
	}
	if err := s.store.InsertSnapshots(ctx, in.SnapshotChain); err != nil {
		return PushResult{}, fmt.Errorf("insert snapshots: %w", err)
	}

	key := BranchKey{Tenant: in.Tenant, Worktree: in.Worktree, TreeID: in.TreeID, Name: in.Branch}
	if err := s.store.AdvanceTipCAS(ctx, key, in.ExpectedTip, in.NewTip); err != nil {
		if errors.Is(err, ErrConflict) {
			actual, _ := s.store.GetBranch(ctx, key)
			return PushResult{Status: "conflict", ActualTip: actual.TipSnapshotID}, nil
		}
		return PushResult{}, fmt.Errorf("advance tip: %w", err)
	}

	return PushResult{
		Status:            "accepted",
		NewTip:            in.NewTip,
		SnapshotsAccepted: len(in.SnapshotChain),
	}, nil
}

// PullInput is the validated payload of POST /api/pull.
type PullInput struct {
	Tenant       string
	Worktree     string
	TreeID       string
	Branch       string
	LastKnownTip string // empty == "client has nothing for this branch yet"
}

// PullResult is returned to the client.
type PullResult struct {
	NewTip         string
	Snapshots      []Snapshot
	ObjectsNeeded  []string
	UpToDate       bool
	BranchNotFound bool
}

// Pull walks the canonical chain from the current tip back to last_known_tip
// and returns the snapshots and the union of their referenced object hashes.
// The client downloads those objects via GET /api/objects/{hash} before
// applying the snapshots locally.
func (s *Service) Pull(ctx context.Context, in PullInput) (PullResult, error) {
	key := BranchKey{Tenant: in.Tenant, Worktree: in.Worktree, TreeID: in.TreeID, Name: in.Branch}
	branch, err := s.store.GetBranch(ctx, key)
	if err != nil {
		if errors.Is(err, ErrBranchNotFound) {
			return PullResult{BranchNotFound: true}, nil
		}
		return PullResult{}, fmt.Errorf("get branch: %w", err)
	}
	if branch.TipSnapshotID == "" {
		return PullResult{UpToDate: true}, nil
	}
	if branch.TipSnapshotID == in.LastKnownTip {
		return PullResult{NewTip: branch.TipSnapshotID, UpToDate: true}, nil
	}
	snapshots, objects, err := s.store.WalkSnapshots(ctx, key, branch.TipSnapshotID, in.LastKnownTip)
	if err != nil {
		return PullResult{}, fmt.Errorf("walk snapshots: %w", err)
	}
	return PullResult{
		NewTip:        branch.TipSnapshotID,
		Snapshots:     snapshots,
		ObjectsNeeded: objects,
	}, nil
}

// ListRefs returns every canonical branch for a tenant/worktree[/tree_id].
func (s *Service) ListRefs(ctx context.Context, tenant, worktree, treeID string) ([]Branch, error) {
	return s.store.ListBranches(ctx, tenant, worktree, treeID)
}

func uniqueObjectHashes(chain []Snapshot, extra []ObjectRef) []string {
	set := map[string]struct{}{}
	for _, snap := range chain {
		for _, obj := range snap.Objects {
			set[obj.Hash] = struct{}{}
		}
	}
	for _, obj := range extra {
		set[obj.Hash] = struct{}{}
	}
	out := make([]string, 0, len(set))
	for h := range set {
		out = append(out, h)
	}
	return out
}
