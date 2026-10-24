// Package staged implements domain.StagedService: team-visible
// work-in-progress ("staged") snapshots.
//
// A staged snapshot is uploaded by a developer's client (manually with
// `wt stage` or automatically by worktree-bg) so that teammates can see what
// is being worked on before it is pushed. Staged snapshots are not part of
// branch history; at most one exists per (tree, account, branch) and a newer
// stage replaces the older one. Entries are removed when the author pushes
// the branch (ClearOnPush) or when they become older than Service.MaxAge.
//
// See crates/worktree-protocol/specs/visibility/StagedVisibility.md.
package staged

import (
	"context"
	"errors"
	"fmt"
	"hash/fnv"
	"path"
	"sort"
	"strings"
	"sync"
	"time"
	"unicode/utf8"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

const (
	// DefaultMaxAge is the default retention of a staged snapshot that is
	// never pushed or re-staged.
	DefaultMaxAge = 7 * 24 * time.Hour

	// MaxFilesListed is the maximum number of changed paths a client may list
	// in one staged snapshot; larger requests are rejected with ErrInvalid.
	MaxFilesListed = 10000

	// MaxFilesStored is the number of changed paths retained in storage.
	// Longer lists are truncated (after normalization and sorting); the
	// FilesAdded/FilesModified/FilesDeleted counters are kept as reported.
	MaxFilesStored = 1000

	// MaxMessageBytes caps the stored message length (truncated on a UTF-8
	// boundary).
	MaxMessageBytes = 4096

	// lockStripes is the number of mutexes used to serialize writes per
	// (tree, account, branch) key.
	lockStripes = 64
)

// Reasons carried in Cleared.Reason.
const (
	ReasonPushed  = "pushed"
	ReasonExpired = "expired"
)

// Cleared is the payload of domain.EventStagedCleared.
type Cleared struct {
	TreeID     string `json:"tree_id"`
	TenantID   string `json:"tenant_id"`
	AccountID  string `json:"account_id"`
	Author     string `json:"author"`
	BranchName string `json:"branch_name"`
	SnapshotID string `json:"snapshot_id"`
	Reason     string `json:"reason"` // ReasonPushed | ReasonExpired
}

// Service manages team-visible staged snapshots. It implements
// domain.StagedService. All methods are safe for concurrent use.
type Service struct {
	store domain.MetaStore
	authz domain.Authorizer
	bus   domain.EventBus // may be nil: events are then not published

	// MaxAge is the retention window: entries whose StagedAt is older than
	// MaxAge are hidden from List/Summary and lazily deleted. A value <= 0
	// disables expiry. It defaults to DefaultMaxAge and must only be changed
	// before the Service is used concurrently.
	MaxAge time.Duration

	now func() time.Time

	// locks serializes the store writes for a given (tree, account, branch)
	// so that lazy expiry can never delete an entry re-staged concurrently.
	locks [lockStripes]sync.Mutex
}

// Compile-time interface check.
var _ domain.StagedService = (*Service)(nil)

// New returns a staged-snapshot service. store and authz are required; bus
// may be nil, in which case no events are published.
func New(store domain.MetaStore, authz domain.Authorizer, bus domain.EventBus) *Service {
	return &Service{
		store:  store,
		authz:  authz,
		bus:    bus,
		MaxAge: DefaultMaxAge,
		now:    time.Now,
	}
}

func (s *Service) lockFor(treeID, accountID, branch string) *sync.Mutex {
	h := fnv.New32a()
	h.Write([]byte(treeID))
	h.Write([]byte{0})
	h.Write([]byte(accountID))
	h.Write([]byte{0})
	h.Write([]byte(branch))
	return &s.locks[h.Sum32()%lockStripes]
}

func (s *Service) publish(e domain.Event) {
	if s.bus != nil {
		s.bus.Publish(e)
	}
}

// Stage records st as the principal's staged snapshot for st.BranchName in
// tree, replacing any previous entry for the same (tree, account, branch).
//
// It requires domain.ActTreeWrite (the authorizer's error is returned as is).
// st.SnapshotID and st.BranchName must be non-empty and at most
// MaxFilesListed paths may be listed (ErrInvalid otherwise). Paths are
// normalized ('/'-separated, cleaned, de-duplicated, sorted) and truncated to
// MaxFilesStored; counters are kept. TreeID, TenantID, AccountID, Author and
// StagedAt are set by the server, overriding client values. On success st is
// updated to the stored record and domain.EventSnapshotStaged is published
// with the domain.StagedSnapshot as payload. On error st is left untouched.
func (s *Service) Stage(ctx context.Context, p *domain.Principal, tree *domain.Tree, st *domain.StagedSnapshot) error {
	if tree == nil {
		return fmt.Errorf("staged: stage: nil tree: %w", domain.ErrInvalid)
	}
	if err := s.authz.Authorize(ctx, p, domain.ActTreeWrite, tree.TenantID, tree); err != nil {
		return err
	}
	if p == nil {
		return fmt.Errorf("staged: stage: anonymous principal: %w", domain.ErrUnauthenticated)
	}
	if st == nil {
		return fmt.Errorf("staged: stage: nil snapshot: %w", domain.ErrInvalid)
	}
	if strings.TrimSpace(st.SnapshotID) == "" {
		return fmt.Errorf("staged: stage: snapshot id is required: %w", domain.ErrInvalid)
	}
	if strings.TrimSpace(st.BranchName) == "" {
		return fmt.Errorf("staged: stage: branch is required: %w", domain.ErrInvalid)
	}
	if n := len(st.FilesChanged); n > MaxFilesListed {
		return fmt.Errorf("staged: stage: %d files listed (max %d): %w", n, MaxFilesListed, domain.ErrInvalid)
	}

	rec := *st
	rec.TreeID = tree.ID
	rec.TenantID = tree.TenantID
	rec.AccountID = p.AccountID
	rec.Author = p.Username
	rec.FilesChanged = normalizePaths(st.FilesChanged, MaxFilesStored)
	rec.Message = truncateUTF8(st.Message, MaxMessageBytes)
	rec.StagedAt = s.now().UTC()

	mu := s.lockFor(rec.TreeID, rec.AccountID, rec.BranchName)
	mu.Lock()
	err := s.store.PutStaged(ctx, &rec)
	mu.Unlock()
	if err != nil {
		return fmt.Errorf("staged: stage: %w", err)
	}

	*st = rec
	payload := rec
	payload.FilesChanged = append([]string(nil), rec.FilesChanged...) // isolate from caller's st
	s.publish(domain.Event{
		Type:     domain.EventSnapshotStaged,
		TenantID: rec.TenantID,
		TreeID:   rec.TreeID,
		Actor:    rec.Author,
		Payload:  payload,
		At:       rec.StagedAt,
	})
	return nil
}

// List returns the live staged snapshots of tree, optionally restricted to
// branch ("" = all branches), newest first. It requires domain.ActTreeRead.
// Entries older than MaxAge are omitted and lazily deleted during the call
// (best effort; deletion errors are ignored), publishing
// domain.EventStagedCleared with Reason "expired".
func (s *Service) List(ctx context.Context, p *domain.Principal, tree *domain.Tree, branch string) ([]domain.StagedSnapshot, error) {
	if tree == nil {
		return nil, fmt.Errorf("staged: list: nil tree: %w", domain.ErrInvalid)
	}
	if err := s.authz.Authorize(ctx, p, domain.ActTreeRead, tree.TenantID, tree); err != nil {
		return nil, err
	}
	live, err := s.listLive(ctx, tree, branch)
	if err != nil {
		return nil, fmt.Errorf("staged: list: %w", err)
	}
	sortNewestFirst(live)
	return live, nil
}

// ClearOnPush removes the principal's staged entry for branch after a
// successful push. It requires domain.ActTreeWrite. If an entry existed it
// publishes domain.EventStagedCleared (payload Cleared, Reason "pushed");
// if none existed it is a no-op and returns nil.
func (s *Service) ClearOnPush(ctx context.Context, p *domain.Principal, tree *domain.Tree, branch string) error {
	if tree == nil {
		return fmt.Errorf("staged: clear: nil tree: %w", domain.ErrInvalid)
	}
	if err := s.authz.Authorize(ctx, p, domain.ActTreeWrite, tree.TenantID, tree); err != nil {
		return err
	}
	if p == nil {
		return fmt.Errorf("staged: clear: anonymous principal: %w", domain.ErrUnauthenticated)
	}
	if branch == "" {
		return fmt.Errorf("staged: clear: branch is required: %w", domain.ErrInvalid)
	}

	mu := s.lockFor(tree.ID, p.AccountID, branch)
	mu.Lock()
	existing, err := s.find(ctx, tree.ID, p.AccountID, branch)
	if err == nil && existing != nil {
		err = s.store.DeleteStaged(ctx, tree.ID, p.AccountID, branch)
		if errors.Is(err, domain.ErrNotFound) {
			err = nil
		}
	}
	mu.Unlock()
	if err != nil {
		return fmt.Errorf("staged: clear: %w", err)
	}
	if existing == nil {
		return nil
	}
	s.publish(domain.Event{
		Type:     domain.EventStagedCleared,
		TenantID: tree.TenantID,
		TreeID:   tree.ID,
		Actor:    p.Username,
		Payload: Cleared{
			TreeID:     tree.ID,
			TenantID:   tree.TenantID,
			AccountID:  p.AccountID,
			Author:     p.Username,
			BranchName: branch,
			SnapshotID: existing.SnapshotID,
			Reason:     ReasonPushed,
		},
	})
	return nil
}

// find returns the stored entry for (tree, account, branch) or nil.
func (s *Service) find(ctx context.Context, treeID, accountID, branch string) (*domain.StagedSnapshot, error) {
	all, err := s.store.ListStaged(ctx, treeID, branch)
	if err != nil {
		return nil, err
	}
	for i := range all {
		if all[i].AccountID == accountID && all[i].BranchName == branch {
			return &all[i], nil
		}
	}
	return nil, nil
}

// listLive lists tree's staged entries, filters out expired ones and lazily
// deletes them.
func (s *Service) listLive(ctx context.Context, tree *domain.Tree, branch string) ([]domain.StagedSnapshot, error) {
	all, err := s.store.ListStaged(ctx, tree.ID, branch)
	if err != nil {
		return nil, err
	}
	if s.MaxAge <= 0 {
		if all == nil {
			all = []domain.StagedSnapshot{}
		}
		return all, nil
	}
	cutoff := s.now().Add(-s.MaxAge)
	live := make([]domain.StagedSnapshot, 0, len(all))
	for _, e := range all {
		if e.StagedAt.Before(cutoff) {
			s.expire(ctx, tree, e)
			continue
		}
		live = append(live, e)
	}
	return live, nil
}

// expire deletes stale entry e, unless it was replaced since it was read.
// Errors are ignored: expiry is best effort and retried on the next read.
func (s *Service) expire(ctx context.Context, tree *domain.Tree, e domain.StagedSnapshot) {
	mu := s.lockFor(tree.ID, e.AccountID, e.BranchName)
	mu.Lock()
	cur, err := s.find(ctx, tree.ID, e.AccountID, e.BranchName)
	deleted := false
	if err == nil && cur != nil && cur.SnapshotID == e.SnapshotID && cur.StagedAt.Equal(e.StagedAt) {
		err = s.store.DeleteStaged(ctx, tree.ID, e.AccountID, e.BranchName)
		deleted = err == nil
	}
	mu.Unlock()
	if !deleted {
		return
	}
	s.publish(domain.Event{
		Type:     domain.EventStagedCleared,
		TenantID: tree.TenantID,
		TreeID:   tree.ID,
		Actor:    "system",
		Payload: Cleared{
			TreeID:     tree.ID,
			TenantID:   tree.TenantID,
			AccountID:  e.AccountID,
			Author:     e.Author,
			BranchName: e.BranchName,
			SnapshotID: e.SnapshotID,
			Reason:     ReasonExpired,
		},
	})
}

// normalizePaths converts paths to clean '/'-separated relative form, drops
// empty entries and duplicates, sorts them and keeps at most max entries.
// The result is never nil.
func normalizePaths(in []string, max int) []string {
	seen := make(map[string]struct{}, len(in))
	out := make([]string, 0, min(len(in), max))
	for _, raw := range in {
		p := strings.TrimSpace(strings.ReplaceAll(raw, `\`, "/"))
		if p == "" {
			continue
		}
		p = strings.TrimPrefix(path.Clean("/"+p), "/")
		if p == "" {
			continue
		}
		if _, dup := seen[p]; dup {
			continue
		}
		seen[p] = struct{}{}
		out = append(out, p)
	}
	sort.Strings(out)
	if len(out) > max {
		out = out[:max:max]
	}
	return out
}

// truncateUTF8 returns s cut to at most n bytes without splitting a rune.
func truncateUTF8(s string, n int) string {
	if len(s) <= n {
		return s
	}
	for n > 0 && !utf8.RuneStart(s[n]) {
		n--
	}
	return s[:n]
}

// sortNewestFirst orders entries by StagedAt descending, then branch, then
// author, for deterministic output.
func sortNewestFirst(es []domain.StagedSnapshot) {
	sort.SliceStable(es, func(i, j int) bool {
		a, b := es[i], es[j]
		if !a.StagedAt.Equal(b.StagedAt) {
			return a.StagedAt.After(b.StagedAt)
		}
		if a.BranchName != b.BranchName {
			return a.BranchName < b.BranchName
		}
		return a.Author < b.Author
	})
}
