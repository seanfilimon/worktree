// Package canonical implements the server-side canonical push/pull surface.
//
// It owns three pieces of state: the per-branch tip pointer (canonical_branches),
// the immutable snapshot history (canonical_snapshots), and the per-snapshot
// object reference list (canonical_snapshot_objects). Object bytes themselves
// live in storage.ObjectStore so the schema can stay metadata-only and the
// blob backend stays swappable (local disk in v1, S3/MinIO in v2).
package canonical

import (
	"context"
	"errors"
	"time"
)

// ErrConflict is returned when a CAS push fails because the branch tip has
// moved since the client's last sync. The caller must pull, merge, and retry.
var ErrConflict = errors.New("canonical branch tip has advanced; push rejected by CAS")

// ErrBranchNotFound is returned when a pull or ref-list targets a branch that
// has never been pushed. The caller can treat this as an empty remote.
var ErrBranchNotFound = errors.New("canonical branch not found")

// Branch identifies a canonical branch tip.
type Branch struct {
	Tenant        string    `json:"tenant"`
	Worktree      string    `json:"worktree"`
	TreeID        string    `json:"tree_id"`
	Name          string    `json:"name"`
	TipSnapshotID string    `json:"tip_snapshot_id"`
	UpdatedAt     time.Time `json:"updated_at"`
}

// Snapshot is the canonical-history record. Parents is the DAG link used by
// pull walks. Payload carries the full Snapshot protocol object verbatim so
// the client can reconstruct it without renormalisation.
type Snapshot struct {
	SnapshotID   string    `json:"snapshot_id"`
	Tenant       string    `json:"tenant"`
	Worktree     string    `json:"worktree"`
	TreeID       string    `json:"tree_id"`
	Branch       string    `json:"branch"`
	Parents      []string  `json:"parents"`
	ManifestHash string    `json:"manifest_hash"`
	Message      string    `json:"message"`
	Author       string    `json:"author"`
	CommittedAt  time.Time `json:"committed_at"`
	Payload      []byte    `json:"-"` // raw JSON bytes
	Objects      []ObjectRef `json:"objects,omitempty"`
}

// ObjectRef links a snapshot to a content-addressed blob at a given path.
type ObjectRef struct {
	Hash string `json:"hash"`
	Path string `json:"path"`
	Size int64  `json:"size"`
}

// BranchKey identifies a canonical branch uniquely.
type BranchKey struct {
	Tenant   string
	Worktree string
	TreeID   string
	Name     string
}

// Store is the persistence contract for canonical push/pull.
//
// Implementations must be safe for concurrent use and must serialize tip
// advancement on a per-branch basis (the Postgres impl relies on the table's
// PK and a CAS UPDATE).
type Store interface {
	// GetBranch reads the current tip for a branch. Returns ErrBranchNotFound
	// if the branch has never been pushed.
	GetBranch(ctx context.Context, key BranchKey) (Branch, error)

	// ListBranches returns every branch under a tenant/worktree/tree_id.
	// tree_id may be empty to list across all trees in the worktree.
	ListBranches(ctx context.Context, tenant, worktree, treeID string) ([]Branch, error)

	// InsertSnapshots persists a chain of snapshots. Each is idempotent on
	// snapshot_id; reinserting a known snapshot is a no-op. Object refs are
	// upserted on (snapshot_id, object_hash, path).
	InsertSnapshots(ctx context.Context, snapshots []Snapshot) error

	// GetSnapshot loads a single canonical snapshot, including its object refs.
	GetSnapshot(ctx context.Context, snapshotID string) (Snapshot, error)

	// WalkSnapshots walks the parent DAG from fromTip backwards. The walk
	// stops when it visits untilTip (exclusive) or runs out of parents. If
	// untilTip is empty, walks back to genesis. Returns snapshots in oldest-
	// to-newest order along with the union of their referenced object hashes.
	WalkSnapshots(ctx context.Context, key BranchKey, fromTip, untilTip string) ([]Snapshot, []string, error)

	// AdvanceTipCAS atomically advances the branch tip from expectedTip to
	// newTip. Returns ErrConflict if the current tip does not match
	// expectedTip. An empty expectedTip matches a branch that has never been
	// pushed (NULL tip).
	AdvanceTipCAS(ctx context.Context, key BranchKey, expectedTip, newTip string) error
}
