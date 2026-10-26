package staged

// v0.8 (docs/ENGINEERING_PLAN.md §16.8, workstream staged-v2): push-time
// advisory. `wt push` asks which of the paths it pushes other people have
// staged changes to and prints "alice has staged changes to ..." (it never
// blocks the push).

import (
	"context"
	"fmt"
	"sort"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// MaxOverlapPaths bounds the number of paths one Overlaps call checks.
const MaxOverlapPaths = 5000

// StagedOverlap is one live staged snapshot of another account that touches
// some of the queried paths.
type StagedOverlap struct {
	AccountID  string    `json:"account_id"`
	Author     string    `json:"author"`
	Branch     string    `json:"branch"`
	Label      string    `json:"label"`
	SnapshotID string    `json:"snapshot_id"`
	Message    string    `json:"message"`
	StagedAt   time.Time `json:"staged_at"`
	// Paths are the queried paths this entry has staged changes to, sorted.
	Paths []string `json:"paths"`
}

// Overlaps returns the live staged snapshots of accounts other than the
// caller, on any branch of tree, whose changed files intersect paths
// (normalized like staged paths; at most MaxOverlapPaths). Sorted by author,
// branch, label. It requires domain.ActTreeRead and a signed-in caller
// (staged metadata is hidden from anonymous readers).
func (s *Service) Overlaps(ctx context.Context, p *domain.Principal, tree *domain.Tree, paths []string) ([]StagedOverlap, error) {
	if tree == nil {
		return nil, fmt.Errorf("staged: overlaps: nil tree: %w", domain.ErrInvalid)
	}
	if err := s.authz.Authorize(ctx, p, domain.ActTreeRead, tree.TenantID, tree); err != nil {
		return nil, err
	}
	if p == nil {
		return nil, fmt.Errorf("staged: overlaps: sign in to see staged work: %w", domain.ErrUnauthenticated)
	}
	if len(paths) > MaxOverlapPaths {
		return nil, fmt.Errorf("staged: overlaps: %d paths (max %d): %w", len(paths), MaxOverlapPaths, domain.ErrInvalid)
	}
	want := make(map[string]struct{}, len(paths))
	for _, q := range normalizePaths(paths, MaxOverlapPaths) {
		want[q] = struct{}{}
	}
	out := []StagedOverlap{}
	if len(want) == 0 {
		return out, nil
	}
	live, err := s.listLive(ctx, tree, "")
	if err != nil {
		return nil, fmt.Errorf("staged: overlaps: %w", err)
	}
	for _, e := range live {
		if e.AccountID == p.AccountID {
			continue
		}
		var hit []string
		for _, f := range e.FilesChanged {
			if _, ok := want[f]; ok {
				hit = append(hit, f)
			}
		}
		if len(hit) == 0 {
			continue
		}
		out = append(out, StagedOverlap{
			AccountID: e.AccountID, Author: e.Author, Branch: e.BranchName, Label: e.Label,
			SnapshotID: e.SnapshotID, Message: e.Message, StagedAt: e.StagedAt, Paths: hit,
		})
	}
	sort.Slice(out, func(i, j int) bool {
		a, b := out[i], out[j]
		if a.Author != b.Author {
			return a.Author < b.Author
		}
		if a.Branch != b.Branch {
			return a.Branch < b.Branch
		}
		return a.Label < b.Label
	})
	return out, nil
}
