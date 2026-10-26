package domain

import (
	"context"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
)

// v0.5 spec-completeness features (docs/ENGINEERING_PLAN.md §13).

// v0.5 actions. Role minimums: release:create and ci:report developer,
// release:delete maintainer.
const (
	ActReleaseCreate Action = "release:create" // create/edit releases and upload assets
	ActReleaseDelete Action = "release:delete" // delete releases and assets
	ActCIReport      Action = "ci:report"      // report CI check results for snapshots
)

// Tenant status (Tenant.Status; "" means TenantActive).
const (
	TenantActive      = "active"
	TenantSuspended   = "suspended"   // read-only: every write is denied
	TenantDeactivated = "deactivated" // all access denied (server admins excepted)
)

// TenantLimits are per-tenant quotas. Zero values mean "server default"
// (which is itself unlimited unless configured).
type TenantLimits struct {
	MaxTrees           int   `json:"max_trees,omitempty"`
	MaxStorageBytes    int64 `json:"max_storage_bytes,omitempty"`
	MaxBranchesPerTree int   `json:"max_branches_per_tree,omitempty"`
	MaxMembers         int   `json:"max_members,omitempty"`
	RequestsPerMinute  int   `json:"requests_per_minute,omitempty"`
}

// Dependency is declared on a snapshot (wire: worktree.sync.v1.Dependency).
// TenantSlug "" = the tenant of the dependent tree; Branch "" = default.
type Dependency struct {
	TenantSlug string `json:"tenant_slug,omitempty"`
	Tree       string `json:"tree"`
	Branch     string `json:"branch,omitempty"`
	Title      string `json:"title"`
	Details    string `json:"details,omitempty"`
	Priority   string `json:"priority,omitempty"` // TodoPriority*; "" = medium
	Blocking   bool   `json:"blocking,omitempty"`
	Linked     bool   `json:"linked,omitempty"`
}

// TODO statuses and priorities.
const (
	TodoOpen       = "open"
	TodoInProgress = "in_progress"
	TodoDone       = "done"
	TodoCancelled  = "cancelled"

	TodoPriorityLow      = "low"
	TodoPriorityMedium   = "medium"
	TodoPriorityHigh     = "high"
	TodoPriorityCritical = "critical"
)

// Todo is work requested from a tree by a dependency declared in another
// tree's snapshot. It lives in the *target* tree (TreeID).
type Todo struct {
	ID           string `json:"id"`
	TenantID     string `json:"tenant_id"` // target tree's tenant
	TreeID       string `json:"tree_id"`   // target tree
	Number       int    `json:"number"`    // per target tree, from 1
	TargetBranch string `json:"target_branch"`
	Title        string `json:"title"`
	Details      string `json:"details,omitempty"`
	Priority     string `json:"priority"`
	Blocking     bool   `json:"blocking"`
	Linked       bool   `json:"linked"`
	Status       string `json:"status"`

	SourceTenantID   string `json:"source_tenant_id"`
	SourceTreeID     string `json:"source_tree_id"`
	SourceBranch     string `json:"source_branch"`
	SourceSnapshotID string `json:"source_snapshot_id"`

	CreatedBy  string     `json:"created_by"` // account id of the pusher
	Author     string     `json:"author"`     // username of the pusher
	AssigneeID string     `json:"assignee_id,omitempty"`
	Assignee   string     `json:"assignee,omitempty"`
	ResolvedBy string     `json:"resolved_by,omitempty"` // snapshot that completed it
	CreatedAt  time.Time  `json:"created_at"`
	UpdatedAt  time.Time  `json:"updated_at"`
	ClosedAt   *time.Time `json:"closed_at,omitempty"`
}

// CI check states.
const (
	CIPending = "pending"
	CISuccess = "success"
	CIFailure = "failure"
	CIError   = "error"
)

// CIStatus is one check result for a snapshot (keyed by tree, snapshot,
// context; a newer report replaces the older one).
type CIStatus struct {
	TreeID      string    `json:"tree_id"`
	SnapshotID  string    `json:"snapshot_id"`
	Context     string    `json:"context"` // check name, e.g. "ci/build"
	State       string    `json:"state"`
	Description string    `json:"description,omitempty"`
	TargetURL   string    `json:"target_url,omitempty"`
	Reporter    string    `json:"reporter"` // username or "sa:<tenant>/<name>"
	UpdatedAt   time.Time `json:"updated_at"`
}

// Review decisions (Approval.Decision; "" means ReviewApproved).
const (
	ReviewApproved         = "approved"
	ReviewChangesRequested = "changes_requested"
	ReviewCommented        = "commented"
)

// ReviewComment is a comment on a merge request, optionally anchored to a
// file line of the snapshot it was written against.
type ReviewComment struct {
	ID         string    `json:"id"`
	TreeID     string    `json:"tree_id"`
	MRNumber   int       `json:"mr_number"`
	AuthorID   string    `json:"author_id"`
	Author     string    `json:"author"`
	Body       string    `json:"body"`
	Path       string    `json:"path,omitempty"`
	Line       int       `json:"line,omitempty"`
	SnapshotID string    `json:"snapshot_id,omitempty"`
	Resolved   bool      `json:"resolved,omitempty"`
	CreatedAt  time.Time `json:"created_at"`
	UpdatedAt  time.Time `json:"updated_at"`
}

// LinkedGroup ties merge requests of several trees together: they merge
// atomically (all or none).
type LinkedGroup struct {
	ID        string              `json:"id"`
	TenantID  string              `json:"tenant_id"`
	Name      string              `json:"name"`
	Members   []LinkedGroupMember `json:"members"`
	State     string              `json:"state"` // MRState*
	CreatedBy string              `json:"created_by"`
	CreatedAt time.Time           `json:"created_at"`
	MergedAt  *time.Time          `json:"merged_at,omitempty"`
}

type LinkedGroupMember struct {
	TreeID   string `json:"tree_id"`
	MRNumber int    `json:"mr_number"`
}

// TagProtection applies to tags matching Pattern (path.Match glob).
type TagProtection struct {
	TreeID         string   `json:"tree_id"`
	Pattern        string   `json:"pattern"`
	NoDelete       bool     `json:"no_delete"`
	NoMove         bool     `json:"no_move"`
	AllowedTaggers []string `json:"allowed_taggers,omitempty"` // usernames; empty = tree:write

	// v0.7 trust (§15.8): matching tags must carry a verified signature.
	RequireSigned bool `json:"require_signed,omitempty"`
}

// Release is a named, annotated version of a tree pointing at a tag.
type Release struct {
	ID          string         `json:"id"`
	TreeID      string         `json:"tree_id"`
	Tag         string         `json:"tag"` // unique per tree
	SnapshotID  string         `json:"snapshot_id"`
	Name        string         `json:"name"`
	Notes       string         `json:"notes,omitempty"`
	Draft       bool           `json:"draft,omitempty"`
	Prerelease  bool           `json:"prerelease,omitempty"`
	Assets      []ReleaseAsset `json:"assets"`
	CreatedBy   string         `json:"created_by"`
	Author      string         `json:"author"`
	CreatedAt   time.Time      `json:"created_at"`
	PublishedAt *time.Time     `json:"published_at,omitempty"`

	// v0.7 trust (§15.8), computed on read (never stored): a release is
	// verified when its tag carries a verified signature.
	Verified     bool   `json:"verified"`
	SigningKeyID string `json:"signing_key_id,omitempty"`
	Signer       string `json:"signer,omitempty"`
}

// ReleaseAsset is a file attached to a release (bytes in the object store).
type ReleaseAsset struct {
	Name        string    `json:"name"`
	Size        int64     `json:"size"`
	Hash        hash.Hash `json:"hash"`
	ContentType string    `json:"content_type"`
	UploadedAt  time.Time `json:"uploaded_at"`

	// v0.7 trust (§15.8): hex SHA-256 of the asset bytes, computed on
	// upload (filled in on read for assets uploaded before v0.7).
	SHA256 string `json:"sha256,omitempty"`
}

// SigningKey is an account's registered Ed25519 public key.
type SigningKey struct {
	ID          string     `json:"id"`
	AccountID   string     `json:"account_id"`
	Name        string     `json:"name"`
	Algorithm   string     `json:"algorithm"`   // "ed25519"
	PublicKey   []byte     `json:"public_key"`  // 32 bytes (base64 in JSON)
	Fingerprint string     `json:"fingerprint"` // "SHA256:" + base64 of sha256(public key), unpadded
	CreatedAt   time.Time  `json:"created_at"`
	LastUsedAt  *time.Time `json:"last_used_at,omitempty"`
	RevokedAt   *time.Time `json:"revoked_at,omitempty"`
}

// AccessConfigDoc is the last applied declarative access document of a
// tenant (TreeID "") or tree.
type AccessConfigDoc struct {
	TenantID  string    `json:"tenant_id"`
	TreeID    string    `json:"tree_id,omitempty"`
	Kind      string    `json:"kind"` // "roles" | "policies" | "license" | "combined"
	Content   []byte    `json:"content"`
	Hash      string    `json:"hash"` // hex BLAKE3 of Content
	UpdatedBy string    `json:"updated_by"`
	UpdatedAt time.Time `json:"updated_at"`
}

// GCReport is the result of one server garbage-collection run.
type GCReport struct {
	DryRun      bool      `json:"dry_run"`
	Scanned     int       `json:"scanned"`
	Reachable   int       `json:"reachable"`
	Unreachable int       `json:"unreachable"`
	Marked      int       `json:"marked"`  // newly marked for deletion (grace period starts)
	Deleted     int       `json:"deleted"` // past the grace period and removed
	FreedBytes  int64     `json:"freed_bytes"`
	StartedAt   time.Time `json:"started_at"`
	FinishedAt  time.Time `json:"finished_at"`

	// Retention (§13.3): staged snapshots older than the staged retention
	// and audit events older than the audit retention removed by the run
	// (dry runs count what would be removed).
	StagedDeleted int `json:"staged_deleted"`
	AuditPruned   int `json:"audit_pruned"`
	// Error is set when the run stopped early.
	Error string `json:"error,omitempty"`
}

// v0.5 events.
const (
	EventTreeDeleted    = "tree.deleted"
	EventBranchDeleted  = "branch.deleted"
	EventTagDeleted     = "tag.deleted"
	EventTodoCreated    = "todo.created"
	EventTodoUpdated    = "todo.updated"
	EventCIStatus       = "ci.status"
	EventMRReviewed     = "mr.reviewed"
	EventMRCommented    = "mr.commented"
	EventGroupMerged    = "group.merged"
	EventReleaseCreated = "release.created"
	EventReleaseDeleted = "release.deleted"
	EventAccessApplied  = "access.applied"
)

// PushPlan describes a validated push to PushCheckers. NewSnapshots are the
// snapshots not yet stored (parents first); ChangedPaths differ between the
// server tip and the new tip (sorted).
type PushPlan struct {
	Branch       string
	OldTip       string // "" for a new branch
	NewTip       string
	NewSnapshots []*Snapshot
	ChangedPaths []string
	// ObjectSize returns the stored size of an object (0 if unknown); lets
	// quota checks size the new content without re-reading it.
	ObjectSize func(ctx context.Context, h hash.Hash) int64
	// NewObjects are the manifest and blob hashes referenced by
	// NewSnapshots (deduplicated, order unspecified).
	NewObjects []hash.Hash
}

// PushChecker is a pluggable push policy (path ACLs, quotas, signatures).
// It runs after the license check and before anything is stored. A
// *PushDenied rejects the push; any other error fails it.
type PushChecker interface {
	CheckPushPlan(ctx context.Context, p *Principal, tree *Tree, plan *PushPlan) error
}

// DocStore holds JSON documents in named collections for the v0.5
// features. Keys are ordered byte-wise; composite keys join components
// with DocKey. All methods are atomic.
type DocStore interface {
	// PutDoc creates or replaces a document.
	PutDoc(ctx context.Context, coll, key string, v any) error
	// CreateDoc fails with ErrAlreadyExists when the key exists.
	CreateDoc(ctx context.Context, coll, key string, v any) error
	// GetDoc decodes the document into v; ErrNotFound when missing.
	GetDoc(ctx context.Context, coll, key string, v any) error
	// ListDocs returns the raw JSON documents whose key has prefix, in key
	// order (prefix "" = all).
	ListDocs(ctx context.Context, coll, prefix string) ([][]byte, error)
	// DeleteDoc removes a document; ErrNotFound when missing.
	DeleteDoc(ctx context.Context, coll, key string) error
	// DeleteDocs removes every document whose key has prefix and returns
	// how many were removed.
	DeleteDocs(ctx context.Context, coll, prefix string) (int, error)
	// UpdateDoc atomically reads (raw nil when missing), transforms and
	// writes a document. fn returning nil bytes deletes it; an error aborts.
	UpdateDoc(ctx context.Context, coll, key string, fn func(raw []byte) ([]byte, error)) error
	// NextSeq increments and returns the named counter (first value 1).
	NextSeq(ctx context.Context, name string) (int64, error)
}

// DocKey joins key components with a NUL byte (sorts like a tuple).
func DocKey(parts ...string) string {
	n := 0
	for _, p := range parts {
		n += len(p) + 1
	}
	b := make([]byte, 0, n)
	for i, p := range parts {
		if i > 0 {
			b = append(b, 0)
		}
		b = append(b, p...)
	}
	return string(b)
}
