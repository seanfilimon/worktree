package staged

// v0.8 (docs/ENGINEERING_PLAN.md §16.8, workstream staged-v2): status
// lifecycle and history of staged snapshots.
//
// Live entries (status "staged") stay in the MetaStore staged bucket. When
// an entry ends it is copied into the DocStore collection CollHistory with
// its final status and kept for Service.HistoryRetention.

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// Statuses of the staged lifecycle.
const (
	StatusStaged  = "staged"  // live
	StatusPushed  = "pushed"  // the author pushed the work
	StatusCleared = "cleared" // removed with `wt unstage` (or by an admin)
	StatusExpired = "expired" // older than the TTL (--staged-ttl)
)

const (
	// CollHistory is the DocStore collection of ended entries, keyed
	// DocKey(treeID, endedAt as historyTimeLayout, id).
	CollHistory = "staged_history"

	// DefaultHistoryRetention is how long ended entries are kept.
	DefaultHistoryRetention = 30 * 24 * time.Hour

	// DefaultSweepInterval is the period of the expiry/prune ticker.
	DefaultSweepInterval = time.Hour

	// DefaultHistoryLimit and MaxHistoryLimit bound History results.
	DefaultHistoryLimit = 100
	MaxHistoryLimit     = 1000

	// historyTimeLayout is fixed width so keys sort chronologically.
	historyTimeLayout = "2006-01-02T15:04:05.000000000Z"
)

// HistoryEntry is an ended staged snapshot. The embedded snapshot's fields
// are flattened in JSON.
type HistoryEntry struct {
	ID string `json:"id"`
	domain.StagedSnapshot
	// Status is the final status: pushed, cleared or expired.
	Status  string    `json:"status"`
	EndedAt time.Time `json:"ended_at"`
	// EndedBy is the username that pushed or unstaged, or "system".
	EndedBy string `json:"ended_by"`
	// PushedTip is the branch tip after the push (status pushed).
	PushedTip string `json:"pushed_tip,omitempty"`
}

// HistoryFilter selects History entries; zero values match everything.
type HistoryFilter struct {
	Branch    string
	AccountID string
	Status    string
	Limit     int // <= 0: DefaultHistoryLimit; capped at MaxHistoryLimit
}

func newHistoryID() string {
	var b [8]byte
	_, _ = rand.Read(b[:])
	return hex.EncodeToString(b[:])
}

func historyKey(treeID string, at time.Time, id string) string {
	return domain.DocKey(treeID, at.UTC().Format(historyTimeLayout), id)
}

// recordHistory stores e as ended with status (best effort: failures are
// logged, never returned, so pushes and unstages do not fail on history).
func (s *Service) recordHistory(ctx context.Context, e domain.StagedSnapshot, status, endedBy, pushedTip string) {
	if s.HistoryRetention <= 0 {
		return
	}
	h := HistoryEntry{
		ID:             newHistoryID(),
		StagedSnapshot: e,
		Status:         status,
		EndedAt:        s.now().UTC(),
		EndedBy:        endedBy,
		PushedTip:      pushedTip,
	}
	h.FilesChanged = append([]string{}, e.FilesChanged...)
	if err := s.store.PutDoc(context.WithoutCancel(ctx), CollHistory, historyKey(e.TreeID, h.EndedAt, h.ID), h); err != nil {
		s.logger().WarnContext(ctx, "staged: recording history failed", "tree", e.TreeID, "branch", e.BranchName, "err", err)
	}
}

// History lists tree's ended staged snapshots within HistoryRetention,
// newest first. It requires domain.ActTreeRead; anonymous callers get an
// empty list (staged metadata is hidden from anonymous readers).
func (s *Service) History(ctx context.Context, p *domain.Principal, tree *domain.Tree, f HistoryFilter) ([]HistoryEntry, error) {
	if tree == nil {
		return nil, fmt.Errorf("staged: history: nil tree: %w", domain.ErrInvalid)
	}
	if err := s.authz.Authorize(ctx, p, domain.ActTreeRead, tree.TenantID, tree); err != nil {
		return nil, err
	}
	out := []HistoryEntry{}
	if p == nil {
		return out, nil
	}
	limit := f.Limit
	if limit <= 0 {
		limit = DefaultHistoryLimit
	}
	limit = min(limit, MaxHistoryLimit)
	raws, err := s.store.ListDocs(ctx, CollHistory, domain.DocKey(tree.ID)+"\x00")
	if err != nil {
		return nil, fmt.Errorf("staged: history: %w", err)
	}
	cutoff := s.historyCutoff()
	for i := len(raws) - 1; i >= 0 && len(out) < limit; i-- {
		var h HistoryEntry
		if err := json.Unmarshal(raws[i], &h); err != nil {
			continue
		}
		if h.EndedAt.Before(cutoff) {
			break // keys are chronological: everything older is too
		}
		if (f.Branch != "" && h.BranchName != f.Branch) ||
			(f.AccountID != "" && h.AccountID != f.AccountID) ||
			(f.Status != "" && h.Status != f.Status) {
			continue
		}
		if h.FilesChanged == nil {
			h.FilesChanged = []string{}
		}
		out = append(out, h)
	}
	return out, nil
}

func (s *Service) historyCutoff() time.Time {
	if s.HistoryRetention <= 0 {
		return s.now().Add(time.Hour) // nothing is retained
	}
	return s.now().Add(-s.HistoryRetention)
}

// pruneHistory deletes history entries older than HistoryRetention in every
// tree and returns how many were removed.
func (s *Service) pruneHistory(ctx context.Context) (int, error) {
	raws, err := s.store.ListDocs(ctx, CollHistory, "")
	if err != nil {
		return 0, err
	}
	cutoff := s.historyCutoff()
	pruned := 0
	for _, raw := range raws {
		var h HistoryEntry
		if err := json.Unmarshal(raw, &h); err != nil || !h.EndedAt.Before(cutoff) {
			continue
		}
		if err := s.store.DeleteDoc(ctx, CollHistory, historyKey(h.TreeID, h.EndedAt, h.ID)); err == nil {
			pruned++
		}
	}
	return pruned, nil
}
