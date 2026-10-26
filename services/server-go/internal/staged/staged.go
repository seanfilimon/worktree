// Package staged implements domain.StagedService: team-visible
// work-in-progress ("staged") snapshots.
//
// A staged snapshot is uploaded by a developer's client (manually with
// `wt stage` or automatically by worktree-bg) so that teammates can see what
// is being worked on before it is pushed. Staged snapshots are not part of
// branch history.
//
// v0.8 (docs/ENGINEERING_PLAN.md §16.8): every (tree, account, branch) has up
// to MaxLabelsPerBranch named slots keyed by a label ("" is the default slot,
// the only one before v0.8, so older entries need no migration). A newer
// stage of the same slot replaces the older one. An entry ends when its
// author pushes it (status "pushed"), removes it (`wt unstage`, status
// "cleared") or when it is older than Service.MaxAge (the --staged-ttl,
// status "expired"); ended entries are kept as history for
// Service.HistoryRetention (collection "staged_history").
//
// See crates/worktree-protocol/specs/visibility/StagedVisibility.md.
package staged

import (
	"context"
	"errors"
	"fmt"
	"hash/fnv"
	"log/slog"
	"path"
	"sort"
	"strings"
	"sync"
	"time"
	"unicode/utf8"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

const (
	// DefaultMaxAge is the default time-to-live of a staged snapshot that is
	// never pushed or re-staged (server flag --staged-ttl).
	DefaultMaxAge = 30 * 24 * time.Hour

	// MaxFilesListed is the maximum number of changed paths a client may list
	// in one staged snapshot; larger requests are rejected with ErrInvalid.
	MaxFilesListed = 100_000 // matches syncsvc's Stage request limit

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
	Reason     string `json:"reason"` // ReasonPushed | ReasonExpired | ReasonUnstaged
	// v0.8 (§16.8).
	Label  string `json:"label,omitempty"`
	Status string `json:"status"` // StatusPushed | StatusExpired | StatusCleared
}

// Service manages team-visible staged snapshots. It implements
// domain.StagedService. All methods are safe for concurrent use.
type Service struct {
	store domain.MetaStore
	authz domain.Authorizer
	bus   domain.EventBus // may be nil: events are then not published

	// MaxAge is the time-to-live: entries whose StagedAt is older than
	// MaxAge are hidden from List/Summary, expired by the sweeper (Start)
	// and lazily on reads. A value <= 0 disables expiry. It defaults to
	// DefaultMaxAge and must only be changed before the Service is used
	// concurrently.
	MaxAge time.Duration

	// HistoryRetention is how long ended entries stay in the history
	// (default DefaultHistoryRetention; <= 0 keeps none).
	HistoryRetention time.Duration

	// SweepInterval is the period of the Start ticker (default
	// DefaultSweepInterval).
	SweepInterval time.Duration

	// Logger receives sweeper warnings (default slog.Default()).
	Logger *slog.Logger

	now func() time.Time

	// locks serializes the store writes for a given (tree, account, branch)
	// so that lazy expiry can never delete an entry re-staged concurrently
	// and the per-branch label limit holds.
	locks [lockStripes]sync.Mutex
}

// Compile-time interface check.
var _ domain.StagedService = (*Service)(nil)

// New returns a staged-snapshot service. store and authz are required; bus
// may be nil, in which case no events are published.
func New(store domain.MetaStore, authz domain.Authorizer, bus domain.EventBus) *Service {
	return &Service{
		store:            store,
		authz:            authz,
		bus:              bus,
		MaxAge:           DefaultMaxAge,
		HistoryRetention: DefaultHistoryRetention,
		SweepInterval:    DefaultSweepInterval,
		now:              time.Now,
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

func (s *Service) logger() *slog.Logger {
	if s.Logger != nil {
		return s.Logger
	}
	return slog.Default()
}

// Stage records st as the principal's staged snapshot for (st.BranchName,
// st.Label) in tree, replacing any previous entry of the same slot.
//
// It requires domain.ActTreeWrite (the authorizer's error is returned as is).
// st.SnapshotID and st.BranchName must be non-empty, st.Label must be ""
// or match LabelPattern, at most MaxFilesListed paths may be listed and a
// new label is refused when the branch already has MaxLabelsPerBranch
// entries of the principal (ErrInvalid otherwise). Paths are normalized
// ('/'-separated, cleaned, de-duplicated, sorted) and truncated to
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
	if err := ValidateLabel(st.Label); err != nil {
		return fmt.Errorf("staged: stage: %w", err)
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
	err := s.checkLabelLimit(ctx, &rec)
	if err == nil {
		err = s.store.PutStaged(ctx, &rec)
	}
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

// checkLabelLimit refuses a new slot when the account already has
// MaxLabelsPerBranch live entries on the branch (caller holds the lock).
// Replacing an existing slot is always allowed.
func (s *Service) checkLabelLimit(ctx context.Context, rec *domain.StagedSnapshot) error {
	mine, err := s.findAll(ctx, rec.TreeID, rec.AccountID, rec.BranchName)
	if err != nil {
		return err
	}
	live := 0
	for _, e := range mine {
		if e.Label == rec.Label {
			return nil
		}
		if !s.isExpired(e) {
			live++
		}
	}
	if live >= MaxLabelsPerBranch {
		return fmt.Errorf("%d staged snapshots on branch %q already (max %d per branch); unstage one first: %w",
			live, rec.BranchName, MaxLabelsPerBranch, domain.ErrInvalid)
	}
	return nil
}

// List returns the live staged snapshots of tree, optionally restricted to
// branch ("" = all branches), newest first. It requires domain.ActTreeRead.
// Anonymous callers (readers of public trees) get an empty list: staged
// metadata is only shown to signed-in readers (§16.8). Entries older than
// MaxAge are omitted and lazily expired during the call (best effort).
func (s *Service) List(ctx context.Context, p *domain.Principal, tree *domain.Tree, branch string) ([]domain.StagedSnapshot, error) {
	if tree == nil {
		return nil, fmt.Errorf("staged: list: nil tree: %w", domain.ErrInvalid)
	}
	if err := s.authz.Authorize(ctx, p, domain.ActTreeRead, tree.TenantID, tree); err != nil {
		return nil, err
	}
	if p == nil {
		return []domain.StagedSnapshot{}, nil
	}
	live, err := s.listLive(ctx, tree, branch)
	if err != nil {
		return nil, fmt.Errorf("staged: list: %w", err)
	}
	sortNewestFirst(live)
	return live, nil
}

// ClearOnPush ends the principal's staged entries of branch after a
// successful push: the default slot always, and every labeled slot whose
// snapshot is the new branch tip or whose working manifest is the tip's
// manifest (that work is now pushed). It requires domain.ActTreeWrite. Each
// ended entry is recorded in the history with status "pushed" and publishes
// domain.EventStagedCleared (payload Cleared, Reason "pushed"); nothing to
// clear is a no-op returning nil.
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
	tipID, tipManifest := s.branchTip(ctx, tree.ID, branch)

	mu := s.lockFor(tree.ID, p.AccountID, branch)
	mu.Lock()
	mine, err := s.findAll(ctx, tree.ID, p.AccountID, branch)
	var ended []domain.StagedSnapshot
	if err == nil {
		for _, e := range mine {
			pushed := e.Label == "" ||
				(tipID != "" && e.SnapshotID == tipID) ||
				(tipManifest != "" && e.ManifestHash == tipManifest)
			if !pushed {
				continue
			}
			if derr := s.store.DeleteStagedLabel(ctx, tree.ID, p.AccountID, branch, e.Label); derr != nil && !errors.Is(derr, domain.ErrNotFound) {
				err = derr
				break
			}
			ended = append(ended, e)
		}
	}
	mu.Unlock()
	for _, e := range ended {
		s.ended(ctx, tree, e, StatusPushed, ReasonPushed, p.Username, tipID)
	}
	if err != nil {
		return fmt.Errorf("staged: clear: %w", err)
	}
	return nil
}

// branchTip returns the tip snapshot id and its manifest hash ("" when
// unknown; best effort).
func (s *Service) branchTip(ctx context.Context, treeID, branch string) (string, string) {
	b, err := s.store.GetBranch(ctx, treeID, branch)
	if err != nil || b == nil || b.Tip == "" {
		return "", ""
	}
	snap, err := s.store.GetSnapshot(ctx, b.Tip)
	if err != nil || snap == nil {
		return b.Tip, ""
	}
	return b.Tip, snap.ManifestHash.Hex()
}

// ended records e in the history and publishes domain.EventStagedCleared.
func (s *Service) ended(ctx context.Context, tree *domain.Tree, e domain.StagedSnapshot, status, reason, actor, pushedTip string) {
	s.recordHistory(ctx, e, status, actor, pushedTip)
	s.publish(domain.Event{
		Type:     domain.EventStagedCleared,
		TenantID: tree.TenantID,
		TreeID:   tree.ID,
		Actor:    actor,
		Payload: Cleared{
			TreeID:     tree.ID,
			TenantID:   tree.TenantID,
			AccountID:  e.AccountID,
			Author:     e.Author,
			BranchName: e.BranchName,
			SnapshotID: e.SnapshotID,
			Reason:     reason,
			Label:      e.Label,
			Status:     status,
		},
	})
}

// find returns the stored entry for (tree, account, branch, label) or nil.
func (s *Service) find(ctx context.Context, treeID, accountID, branch, label string) (*domain.StagedSnapshot, error) {
	all, err := s.findAll(ctx, treeID, accountID, branch)
	if err != nil {
		return nil, err
	}
	for i := range all {
		if all[i].Label == label {
			return &all[i], nil
		}
	}
	return nil, nil
}

// findAll returns every stored entry (any label) of (tree, account, branch).
func (s *Service) findAll(ctx context.Context, treeID, accountID, branch string) ([]domain.StagedSnapshot, error) {
	all, err := s.store.ListStaged(ctx, treeID, branch)
	if err != nil {
		return nil, err
	}
	out := all[:0]
	for _, e := range all {
		if e.AccountID == accountID && e.BranchName == branch {
			out = append(out, e)
		}
	}
	return out, nil
}

func (s *Service) isExpired(e domain.StagedSnapshot) bool {
	return s.MaxAge > 0 && e.StagedAt.Before(s.now().Add(-s.MaxAge))
}

// listLive lists tree's staged entries, filters out expired ones and lazily
// expires them.
func (s *Service) listLive(ctx context.Context, tree *domain.Tree, branch string) ([]domain.StagedSnapshot, error) {
	all, err := s.store.ListStaged(ctx, tree.ID, branch)
	if err != nil {
		return nil, err
	}
	live := make([]domain.StagedSnapshot, 0, len(all))
	for _, e := range all {
		if s.isExpired(e) {
			s.expire(ctx, tree, e)
			continue
		}
		live = append(live, e)
	}
	return live, nil
}

// expire deletes stale entry e (recording it as "expired"), unless it was
// replaced since it was read. It reports whether e was removed. Errors are
// ignored: expiry is best effort and retried on the next read or sweep.
func (s *Service) expire(ctx context.Context, tree *domain.Tree, e domain.StagedSnapshot) bool {
	mu := s.lockFor(tree.ID, e.AccountID, e.BranchName)
	mu.Lock()
	cur, err := s.find(ctx, tree.ID, e.AccountID, e.BranchName, e.Label)
	deleted := false
	if err == nil && cur != nil && cur.SnapshotID == e.SnapshotID && cur.StagedAt.Equal(e.StagedAt) {
		err = s.store.DeleteStagedLabel(ctx, tree.ID, e.AccountID, e.BranchName, e.Label)
		deleted = err == nil
	}
	mu.Unlock()
	if deleted {
		s.ended(ctx, tree, e, StatusExpired, ReasonExpired, "system", "")
	}
	return deleted
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

// sortNewestFirst orders entries by StagedAt descending, then branch,
// author and label, for deterministic output.
func sortNewestFirst(es []domain.StagedSnapshot) {
	sort.SliceStable(es, func(i, j int) bool {
		a, b := es[i], es[j]
		if !a.StagedAt.Equal(b.StagedAt) {
			return a.StagedAt.After(b.StagedAt)
		}
		if a.BranchName != b.BranchName {
			return a.BranchName < b.BranchName
		}
		if a.Author != b.Author {
			return a.Author < b.Author
		}
		return a.Label < b.Label
	})
}
