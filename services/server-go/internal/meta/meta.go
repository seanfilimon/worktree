// Package meta implements [domain.MetaStore] and [domain.History].
//
// Two stores with identical observable semantics are provided:
//
//   - [Bolt] ([NewBolt]) persists everything in a single bbolt file using
//     one bucket per entity, JSON-encoded values and secondary index
//     buckets for unique lookups.
//   - [Memory] ([NewMemory]) keeps everything in mutex-guarded maps and is
//     intended for tests and ephemeral servers.
//
// Shared semantics (enforced by the package's conformance suite):
//
//   - Errors wrap [domain.ErrNotFound], [domain.ErrAlreadyExists],
//     [domain.ErrConflict] or [domain.ErrInvalid] so callers can use
//     errors.Is. Operations on a closed store fail with [ErrClosed].
//   - Create methods assign a UUID when the ID is empty and fill zero
//     CreatedAt/UpdatedAt/StagedAt/At timestamps with the current time;
//     generated values are written back into the argument. Snapshots are
//     the exception for timestamps: they are client-authored, immutable and
//     stored verbatim.
//   - Update and replace methods set UpdatedAt (where the type has one) to
//     the current time and preserve the stored CreatedAt when the argument's
//     is zero.
//   - Stored values are copies: mutating an argument after a call, or a
//     returned value, never affects the store. Times are normalized to UTC.
//     Nil slices are stored as empty slices, so JSON renders them as [].
//   - List methods return empty non-nil slices and use deterministic
//     ordering: tenants by slug, accounts by username, trees by name (then
//     tenant id), branches/tags by name, protections by pattern, license
//     rules by path glob, memberships by the other side's id, tokens by
//     creation time, merge requests by number descending, staged snapshots
//     by staged_at descending and audit events newest first.
//   - Delete methods return ErrNotFound when the record does not exist,
//     except DeleteStaged, which is idempotent (it is called on every push).
//   - Key components (ids, names, slugs, patterns) must be non-empty and
//     must not contain NUL bytes; violations wrap [domain.ErrInvalid].
package meta

import (
	"context"
	"errors"
	"fmt"
	"maps"
	"slices"
	"strings"
	"time"

	"github.com/google/uuid"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// ErrClosed is returned by every operation on a store after Close.
var ErrClosed = errors.New("meta: store is closed")

// defaultAuditLimit is the page size used when AuditFilter.Limit <= 0.
const defaultAuditLimit = 100

// SnapshotViewer is an optional interface implemented by [Bolt] and
// [Memory]. It serves many snapshot reads from one consistent view (a
// single read transaction / read lock), which makes history walks over
// large DAGs much cheaper than one GetSnapshot call per node. [NewHistory]
// uses it automatically when available.
type SnapshotViewer interface {
	// ViewSnapshots calls fn with a lookup function valid only for the
	// duration of fn. get returns an error wrapping domain.ErrNotFound for
	// unknown ids. fn must not call other methods of the store.
	ViewSnapshots(ctx context.Context, fn func(get func(id string) (*domain.Snapshot, error)) error) error
}

// now returns the current time in UTC (without a monotonic reading).
func now() time.Time { return time.Now().UTC() }

// newID returns a random (v4) UUID string.
func newID() string { return uuid.NewString() }

// sep separates the components of composite keys. Key components are
// validated not to contain it, so prefix scans are unambiguous.
const sep = "\x00"

// key joins key components with sep.
func key(parts ...string) string { return strings.Join(parts, sep) }

// prefix returns the scan prefix for keys whose first component is first.
func prefix(first string) string { return first + sep }

// checkKey validates one key component.
func checkKey(field, v string) error {
	if v == "" {
		return fmt.Errorf("meta: %s is required: %w", field, domain.ErrInvalid)
	}
	if strings.Contains(v, sep) {
		return fmt.Errorf("meta: %s contains a NUL byte: %w", field, domain.ErrInvalid)
	}
	return nil
}

// checkKeys validates pairs of (field name, value).
func checkKeys(pairs ...string) error {
	for i := 0; i+1 < len(pairs); i += 2 {
		if err := checkKey(pairs[i], pairs[i+1]); err != nil {
			return err
		}
	}
	return nil
}

// checkOptionalKey validates a key component that may be empty.
func checkOptionalKey(field, v string) error {
	if v == "" {
		return nil
	}
	return checkKey(field, v)
}

func errNil(what string) error {
	return fmt.Errorf("meta: nil %s: %w", what, domain.ErrInvalid)
}

func notFound(kind string, id ...string) error {
	return fmt.Errorf("meta: %s %q: %w", kind, strings.Join(id, "/"), domain.ErrNotFound)
}

func alreadyExists(kind string, id ...string) error {
	return fmt.Errorf("meta: %s %q: %w", kind, strings.Join(id, "/"), domain.ErrAlreadyExists)
}

// ---- normalization / cloning ------------------------------------------
//
// norm* functions return a deep copy of their argument with times in UTC
// and nil slices replaced by empty ones. They are applied on the way into
// both stores, and (as plain deep copies) on the way out of Memory, so
// that Bolt (JSON round trip) and Memory return identical values.

func utc(t time.Time) time.Time {
	if t.IsZero() {
		return time.Time{}
	}
	return t.UTC()
}

func utcPtr(t *time.Time) *time.Time {
	if t == nil {
		return nil
	}
	v := utc(*t)
	return &v
}

func cloneStrings(s []string) []string {
	if s == nil {
		return []string{}
	}
	return slices.Clone(s)
}

func normTenant(t domain.Tenant) domain.Tenant {
	t.CreatedAt = utc(t.CreatedAt)
	return t
}

func normAccount(a domain.Account) domain.Account {
	a.CreatedAt = utc(a.CreatedAt)
	return a
}

func normMembership(m domain.Membership) domain.Membership {
	m.CreatedAt = utc(m.CreatedAt)
	return m
}

func normToken(t domain.APIToken) domain.APIToken {
	t.CreatedAt = utc(t.CreatedAt)
	t.ExpiresAt = utcPtr(t.ExpiresAt)
	t.LastUsedAt = utcPtr(t.LastUsedAt)
	return t
}

func normTree(t domain.Tree) domain.Tree {
	t.CreatedAt = utc(t.CreatedAt)
	t.UpdatedAt = utc(t.UpdatedAt)
	return t
}

func normBranch(b domain.Branch) domain.Branch {
	b.CreatedAt = utc(b.CreatedAt)
	b.UpdatedAt = utc(b.UpdatedAt)
	return b
}

func normSnapshot(s domain.Snapshot) domain.Snapshot {
	s.Parents = cloneStrings(s.Parents)
	s.CreatedAt = utc(s.CreatedAt)
	s.PushedAt = utc(s.PushedAt)
	return s
}

func normTag(t domain.Tag) domain.Tag {
	t.CreatedAt = utc(t.CreatedAt)
	return t
}

func normStaged(s domain.StagedSnapshot) domain.StagedSnapshot {
	s.FilesChanged = cloneStrings(s.FilesChanged)
	s.StagedAt = utc(s.StagedAt)
	return s
}

func normProtection(p domain.BranchProtection) domain.BranchProtection {
	p.AllowedPushers = cloneStrings(p.AllowedPushers)
	return p
}

func normMR(mr domain.MergeRequest) domain.MergeRequest {
	approvals := make([]domain.Approval, len(mr.Approvals))
	for i, a := range mr.Approvals {
		a.At = utc(a.At)
		approvals[i] = a
	}
	mr.Approvals = approvals
	mr.CreatedAt = utc(mr.CreatedAt)
	mr.UpdatedAt = utc(mr.UpdatedAt)
	mr.MergedAt = utcPtr(mr.MergedAt)
	return mr
}

func normAudit(e domain.AuditEvent) domain.AuditEvent {
	if len(e.Details) == 0 {
		e.Details = nil
	} else {
		e.Details = maps.Clone(e.Details)
	}
	e.At = utc(e.At)
	return e
}

// snapshotContentEqual reports whether a and b describe the same immutable
// snapshot. Push metadata (BranchName, PushedBy, PushedAt) is excluded: the
// same snapshot legitimately reaches the server via different branches and
// pushes, and re-pushing it must be a no-op rather than a conflict.
func snapshotContentEqual(a, b *domain.Snapshot) bool {
	return a.ID == b.ID &&
		a.TreeID == b.TreeID &&
		slices.Equal(a.Parents, b.Parents) &&
		a.ManifestHash == b.ManifestHash &&
		a.Author == b.Author &&
		a.Message == b.Message &&
		a.CreatedAt.Equal(b.CreatedAt) &&
		a.AutoGenerated == b.AutoGenerated
}

// auditMatches reports whether e passes the non-limit parts of f.
func auditMatches(f *domain.AuditFilter, e *domain.AuditEvent) bool {
	if f.TenantID != "" && e.TenantID != f.TenantID {
		return false
	}
	if f.ActorID != "" && e.ActorID != f.ActorID {
		return false
	}
	if f.Action != "" && e.Action != f.Action {
		return false
	}
	return true
}

func auditLimit(f *domain.AuditFilter) int {
	if f.Limit <= 0 {
		return defaultAuditLimit
	}
	return f.Limit
}

// ---- deterministic orderings ------------------------------------------

func sortTrees(ts []domain.Tree) {
	slices.SortFunc(ts, func(a, b domain.Tree) int {
		if c := strings.Compare(a.Name, b.Name); c != 0 {
			return c
		}
		if c := strings.Compare(a.TenantID, b.TenantID); c != 0 {
			return c
		}
		return strings.Compare(a.ID, b.ID)
	})
}

func sortTokens(ts []domain.APIToken) {
	slices.SortFunc(ts, func(a, b domain.APIToken) int {
		if c := a.CreatedAt.Compare(b.CreatedAt); c != 0 {
			return c
		}
		return strings.Compare(a.ID, b.ID)
	})
}

// sortStaged orders newest staged_at first, ties broken by account id then
// branch name (ascending).
func sortStaged(ss []domain.StagedSnapshot) {
	slices.SortFunc(ss, func(a, b domain.StagedSnapshot) int {
		if c := b.StagedAt.Compare(a.StagedAt); c != 0 {
			return c
		}
		if c := strings.Compare(a.AccountID, b.AccountID); c != 0 {
			return c
		}
		return strings.Compare(a.BranchName, b.BranchName)
	})
}

// prepareMR validates and fills defaults for a new merge request. number is
// the per-tree number to assign.
func prepareMR(mr *domain.MergeRequest, number int) {
	if mr.ID == "" {
		mr.ID = newID()
	}
	mr.Number = number
	if mr.State == "" {
		mr.State = domain.MRStateOpen
	}
	if mr.CreatedAt.IsZero() {
		mr.CreatedAt = now()
	}
	if mr.UpdatedAt.IsZero() {
		mr.UpdatedAt = mr.CreatedAt
	}
	if mr.Approvals == nil {
		mr.Approvals = []domain.Approval{}
	}
}

// checkMRUpdate validates an update against the stored record and fills
// the store-managed fields of mr.
func checkMRUpdate(mr, stored *domain.MergeRequest) error {
	if mr.ID == "" {
		mr.ID = stored.ID
	} else if mr.ID != stored.ID {
		return fmt.Errorf("meta: merge request %s/%d has id %q, not %q: %w",
			mr.TreeID, mr.Number, stored.ID, mr.ID, domain.ErrInvalid)
	}
	if mr.CreatedAt.IsZero() {
		mr.CreatedAt = stored.CreatedAt
	}
	mr.UpdatedAt = now()
	if mr.Approvals == nil {
		mr.Approvals = []domain.Approval{}
	}
	return nil
}
