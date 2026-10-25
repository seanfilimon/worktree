package domain

import (
	"context"
	"time"
)

// MetaStore persists all server metadata. Implementations: internal/meta
// (bbolt-backed NewBolt, and NewMemory for tests). All methods return
// ErrNotFound / ErrAlreadyExists / ErrConflict (wrapped) where applicable.
// List methods return an empty (non-nil) slice when nothing matches.
type MetaStore interface {
	IAMStore // v0.4 (iam.go)
	DocStore // v0.5 (v05.go)

	// Tenants
	CreateTenant(ctx context.Context, t *Tenant) error
	GetTenant(ctx context.Context, id string) (*Tenant, error)
	GetTenantBySlug(ctx context.Context, slug string) (*Tenant, error)
	ListTenants(ctx context.Context) ([]Tenant, error)
	UpdateTenant(ctx context.Context, t *Tenant) error

	// Accounts
	CreateAccount(ctx context.Context, a *Account) error
	GetAccount(ctx context.Context, id string) (*Account, error)
	GetAccountByUsername(ctx context.Context, username string) (*Account, error)
	ListAccounts(ctx context.Context) ([]Account, error)
	UpdateAccount(ctx context.Context, a *Account) error

	// Memberships
	PutMembership(ctx context.Context, m *Membership) error
	GetMembership(ctx context.Context, tenantID, accountID string) (*Membership, error)
	ListTenantMembers(ctx context.Context, tenantID string) ([]Membership, error)
	ListAccountMemberships(ctx context.Context, accountID string) ([]Membership, error)
	DeleteMembership(ctx context.Context, tenantID, accountID string) error

	// API tokens
	CreateToken(ctx context.Context, t *APIToken) error
	GetTokenByHash(ctx context.Context, tokenHash string) (*APIToken, error)
	ListTokens(ctx context.Context, accountID string) ([]APIToken, error)
	DeleteToken(ctx context.Context, id string) error
	TouchToken(ctx context.Context, id string, at time.Time) error

	// Trees
	CreateTree(ctx context.Context, t *Tree) error
	GetTree(ctx context.Context, id string) (*Tree, error)
	GetTreeByName(ctx context.Context, tenantID, name string) (*Tree, error)
	ListTrees(ctx context.Context, tenantID string) ([]Tree, error) // "" = all tenants
	UpdateTree(ctx context.Context, t *Tree) error
	DeleteTree(ctx context.Context, id string) error

	// Branches
	GetBranch(ctx context.Context, treeID, name string) (*Branch, error)
	ListBranches(ctx context.Context, treeID string) ([]Branch, error)
	PutBranch(ctx context.Context, b *Branch) error
	// CompareAndSwapBranchTip atomically sets the tip to newTip iff the current
	// tip equals oldTip ("" means the branch must not exist or be empty; the
	// branch is created in that case). Returns ErrConflict otherwise.
	CompareAndSwapBranchTip(ctx context.Context, treeID, name, oldTip, newTip string) error
	DeleteBranch(ctx context.Context, treeID, name string) error

	// Snapshots (immutable once written; PutSnapshot of an existing id with
	// identical content is a no-op, different content is ErrConflict)
	PutSnapshot(ctx context.Context, s *Snapshot) error
	GetSnapshot(ctx context.Context, id string) (*Snapshot, error)
	HasSnapshot(ctx context.Context, id string) (bool, error)
	CountSnapshots(ctx context.Context, treeID string) (int, error) // "" = all

	// Tags
	PutTag(ctx context.Context, t *Tag) error // ErrAlreadyExists if name taken
	GetTag(ctx context.Context, treeID, name string) (*Tag, error)
	ListTags(ctx context.Context, treeID string) ([]Tag, error)
	DeleteTag(ctx context.Context, treeID, name string) error

	// Staged snapshots (keyed by tree+account+branch; Put replaces)
	PutStaged(ctx context.Context, s *StagedSnapshot) error
	ListStaged(ctx context.Context, treeID, branch string) ([]StagedSnapshot, error) // branch "" = all
	DeleteStaged(ctx context.Context, treeID, accountID, branch string) error
	CountStaged(ctx context.Context) (int, error)

	// Branch protection (keyed by tree+pattern)
	PutProtection(ctx context.Context, p *BranchProtection) error
	ListProtections(ctx context.Context, treeID string) ([]BranchProtection, error)
	DeleteProtection(ctx context.Context, treeID, pattern string) error

	// Merge requests. CreateMergeRequest assigns ID (if empty) and the next
	// per-tree Number.
	CreateMergeRequest(ctx context.Context, mr *MergeRequest) error
	GetMergeRequest(ctx context.Context, treeID string, number int) (*MergeRequest, error)
	ListMergeRequests(ctx context.Context, treeID, state string) ([]MergeRequest, error) // state "" = all
	UpdateMergeRequest(ctx context.Context, mr *MergeRequest) error

	// Audit (append-only)
	AppendAudit(ctx context.Context, e *AuditEvent) error
	ListAudit(ctx context.Context, f AuditFilter) ([]AuditEvent, error) // newest first
	// PruneAudit deletes every audit event recorded before the given
	// instant and returns how many were removed (v0.5 §13.3 retention).
	PruneAudit(ctx context.Context, before time.Time) (int, error)

	// License rules (keyed by tree+path glob)
	PutLicenseRule(ctx context.Context, r *LicenseRule) error
	ListLicenseRules(ctx context.Context, treeID string) ([]LicenseRule, error)
	DeleteLicenseRule(ctx context.Context, treeID, pathGlob string) error

	// Raw access-config documents synced from clients (roles, policies...).
	PutAccessConfig(ctx context.Context, tenantID, kind string, content []byte) error
	GetAccessConfig(ctx context.Context, tenantID, kind string) ([]byte, error)

	Close() error
}

// Authenticator handles credentials and tokens. Implementation: internal/iam.
type Authenticator interface {
	// Register creates an account plus its personal tenant (slug = username)
	// with the account as owner.
	Register(ctx context.Context, username, email, password string) (*Account, error)
	// Login verifies the password and issues a new session token.
	Login(ctx context.Context, username, password string) (token string, acct *Account, err error)
	// Authenticate resolves a raw bearer token. ErrUnauthenticated if invalid.
	Authenticate(ctx context.Context, token string) (*Principal, error)
	CreateToken(ctx context.Context, accountID, name string, ttl time.Duration) (token string, t *APIToken, err error)
	RevokeToken(ctx context.Context, accountID, tokenID string) error
	SetPassword(ctx context.Context, accountID, password string) error
}

// Action is an authorization verb.
type Action string

const (
	ActTreeRead     Action = "tree:read"
	ActTreeWrite    Action = "tree:write" // push, stage, tag
	ActTreeCreate   Action = "tree:create"
	ActTreeAdmin    Action = "tree:admin" // protections, licenses, settings, delete
	ActBranchCreate Action = "branch:create"
	ActBranchDelete Action = "branch:delete"
	ActMRCreate     Action = "mr:create"
	ActMRApprove    Action = "mr:approve"
	ActMRMerge      Action = "mr:merge"
	ActTenantRead   Action = "tenant:read"
	ActTenantAdmin  Action = "tenant:admin" // members, settings
	ActServerAdmin  Action = "server:admin"
)

// Authorizer decides whether a principal may perform an action. tree may be
// nil for tenant-level actions. p may be nil (anonymous). Returns nil or an
// error wrapping ErrForbidden / ErrUnauthenticated. Implementation: internal/iam.
type Authorizer interface {
	Authorize(ctx context.Context, p *Principal, act Action, tenantID string, tree *Tree) error
	RoleOf(ctx context.Context, accountID, tenantID string) (string, error) // "" if none
}

// EventBus fans out events to in-process subscribers. Implementation:
// internal/events. Publish never blocks (slow subscribers drop events).
type EventBus interface {
	Publish(e Event)
	Subscribe(f EventFilter) (ch <-chan Event, cancel func())
}

// AuditLog records security-relevant actions. Implementation: internal/audit.
type AuditLog interface {
	Record(ctx context.Context, e AuditEvent) // never fails the caller; logs errors
	Query(ctx context.Context, f AuditFilter) ([]AuditEvent, error)
}

// PushDenied is returned by policy checks to reject a push; syncsvc maps
// Kind to the matching PushRejection oneof.
type PushDenied struct {
	Kind    string // "branch_protection" | "access_denied" | "license_violation" | "review_required" | "quota_exceeded" | "signature_required" | "ci_checks_failed"
	Reason  string
	Path    string // license_violation
	License string // license_violation
}

func (e *PushDenied) Error() string { return "push denied (" + e.Kind + "): " + e.Reason }

// LicenseChecker enforces per-path SPDX rules. Implementation: internal/license.
type LicenseChecker interface {
	// CheckPush returns *PushDenied when the principal's tenant is not allowed
	// to write the given paths of tree.
	CheckPush(ctx context.Context, p *Principal, tree *Tree, changedPaths []string) error
	// CheckRead returns *PushDenied (Kind "license_violation") when the paths
	// may not be read/exported by the principal.
	CheckRead(ctx context.Context, p *Principal, tree *Tree, paths []string) error
}

// ReviewService handles branch protection and merge requests.
// Implementation: internal/review.
type ReviewService interface {
	// CheckPush enforces branch protection for a direct push.
	CheckPush(ctx context.Context, p *Principal, tree *Tree, branch string) error
	CheckDeleteBranch(ctx context.Context, p *Principal, tree *Tree, branch string) error
	Open(ctx context.Context, p *Principal, tree *Tree, title, description, source, target string) (*MergeRequest, error)
	Approve(ctx context.Context, p *Principal, tree *Tree, number int) (*MergeRequest, error)
	Close(ctx context.Context, p *Principal, tree *Tree, number int) (*MergeRequest, error)
	// Merge merges the request with the default method (fast-forward when
	// possible, else a server-side merge snapshot; see review.MergeWith for
	// explicit methods). Fails with ErrConflict when no method can merge, or
	// *PushDenied{Kind:"review_required"} without enough approvals.
	Merge(ctx context.Context, p *Principal, tree *Tree, number int) (*MergeRequest, error)
}

// StagedService manages team-visible WIP snapshots. Implementation:
// internal/staged. It publishes EventSnapshotStaged / EventStagedCleared.
type StagedService interface {
	Stage(ctx context.Context, p *Principal, tree *Tree, s *StagedSnapshot) error
	List(ctx context.Context, p *Principal, tree *Tree, branch string) ([]StagedSnapshot, error)
	// ClearOnPush removes the pusher's staged entry for branch after a push.
	ClearOnPush(ctx context.Context, p *Principal, tree *Tree, branch string) error
}

// History walks snapshot ancestry stored in a MetaStore. Implementation:
// internal/meta (NewHistory). Used by syncsvc, review and httpapi.
type History interface {
	// IsAncestor reports whether ancestor is reachable from descendant
	// (a snapshot is its own ancestor).
	IsAncestor(ctx context.Context, ancestor, descendant string) (bool, error)
	// Log returns snapshots reachable from tip, newest first (by created_at,
	// then id), at most limit (0 = no limit).
	Log(ctx context.Context, tip string, limit int) ([]Snapshot, error)
	// Missing returns snapshots reachable from tip but not from any of haves,
	// oldest first (parents before children).
	Missing(ctx context.Context, tip string, haves []string) ([]Snapshot, error)
}
