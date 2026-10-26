package staged

import (
	"context"
	"fmt"
	"sort"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// BranchActivity aggregates the live staged snapshots of one branch. It
// answers "what is everyone working on?" for a tree and carries early
// conflict warnings (see FileOverlap).
type BranchActivity struct {
	Branch string `json:"branch"`
	// Authors holds one entry per account with a staged snapshot on the
	// branch, newest first.
	Authors []AuthorActivity `json:"authors"`
	// FilesAdded/FilesModified/FilesDeleted sum the authors' reported
	// counters (which are not affected by list truncation).
	FilesAdded    int `json:"files_added"`
	FilesModified int `json:"files_modified"`
	FilesDeleted  int `json:"files_deleted"`
	// DistinctFiles is the number of distinct listed paths across authors.
	DistinctFiles int `json:"distinct_files"`
	// LatestAt is the most recent StagedAt on the branch.
	LatestAt time.Time `json:"latest_at"`
	// Overlaps lists files that an author on this branch has staged and that
	// at least one *different* account has also staged, on this or any other
	// branch of the tree: potential future merge conflicts. Sorted by path.
	Overlaps []FileOverlap `json:"overlaps"`
}

// AuthorActivity is one author's staged snapshot on a branch.
type AuthorActivity struct {
	AccountID     string    `json:"account_id"`
	Author        string    `json:"author"`
	SnapshotID    string    `json:"snapshot_id"`
	Message       string    `json:"message"`
	FilesAdded    uint32    `json:"files_added"`
	FilesModified uint32    `json:"files_modified"`
	FilesDeleted  uint32    `json:"files_deleted"`
	FilesListed   int       `json:"files_listed"`    // len(FilesChanged) as stored
	Label         string    `json:"label,omitempty"` // v0.8 (§16.8)
	StagedAt      time.Time `json:"staged_at"`
}

// FileOverlap reports a path staged by two or more distinct accounts.
type FileOverlap struct {
	Path string `json:"path"`
	// Contributors are every (account, branch) that staged Path, sorted by
	// branch then author. It always names at least two distinct accounts.
	Contributors []Contributor `json:"contributors"`
}

// Contributor identifies who staged a path, and on which branch.
type Contributor struct {
	AccountID string `json:"account_id"`
	Author    string `json:"author"`
	Branch    string `json:"branch"`
	Label     string `json:"label,omitempty"` // v0.8 (§16.8)
}

// Summary aggregates the tree's live staged snapshots per branch, sorted by
// most recent activity first (then branch name). It requires
// domain.ActTreeRead and, like List, hides and lazily deletes expired
// entries.
//
// Overlap detection compares the stored FilesChanged lists, so paths beyond
// MaxFilesStored in a very large staged snapshot are not considered. An
// overlap is advisory: it means two people are editing the same file and
// should coordinate, not that a merge will necessarily conflict.
func (s *Service) Summary(ctx context.Context, p *domain.Principal, tree *domain.Tree) ([]BranchActivity, error) {
	if tree == nil {
		return nil, fmt.Errorf("staged: summary: nil tree: %w", domain.ErrInvalid)
	}
	if err := s.authz.Authorize(ctx, p, domain.ActTreeRead, tree.TenantID, tree); err != nil {
		return nil, err
	}
	if p == nil { // v0.8 (§16.8): hidden from anonymous readers
		return []BranchActivity{}, nil
	}
	live, err := s.listLive(ctx, tree, "")
	if err != nil {
		return nil, fmt.Errorf("staged: summary: %w", err)
	}
	return summarize(live), nil
}

// summarize is the pure aggregation behind Summary. The result is never nil.
func summarize(entries []domain.StagedSnapshot) []BranchActivity {
	// Index: path -> contributors.
	byPath := make(map[string][]Contributor)
	byBranch := make(map[string]*BranchActivity)
	var order []string

	for _, e := range entries {
		ba := byBranch[e.BranchName]
		if ba == nil {
			ba = &BranchActivity{Branch: e.BranchName, Authors: []AuthorActivity{}, Overlaps: []FileOverlap{}}
			byBranch[e.BranchName] = ba
			order = append(order, e.BranchName)
		}
		ba.Authors = append(ba.Authors, AuthorActivity{
			AccountID:     e.AccountID,
			Author:        e.Author,
			SnapshotID:    e.SnapshotID,
			Message:       e.Message,
			FilesAdded:    e.FilesAdded,
			FilesModified: e.FilesModified,
			FilesDeleted:  e.FilesDeleted,
			FilesListed:   len(e.FilesChanged),
			StagedAt:      e.StagedAt,
			Label:         e.Label,
		})
		ba.FilesAdded += int(e.FilesAdded)
		ba.FilesModified += int(e.FilesModified)
		ba.FilesDeleted += int(e.FilesDeleted)
		if e.StagedAt.After(ba.LatestAt) {
			ba.LatestAt = e.StagedAt
		}
		c := Contributor{AccountID: e.AccountID, Author: e.Author, Branch: e.BranchName, Label: e.Label}
		seen := make(map[string]struct{}, len(e.FilesChanged))
		for _, f := range e.FilesChanged {
			if _, dup := seen[f]; dup {
				continue
			}
			seen[f] = struct{}{}
			byPath[f] = append(byPath[f], c)
		}
	}

	// Distinct files per branch and overlaps.
	paths := make([]string, 0, len(byPath))
	for f := range byPath {
		paths = append(paths, f)
	}
	sort.Strings(paths)
	for _, f := range paths {
		cs := byPath[f]
		branches := make(map[string]struct{}, len(cs))
		accounts := make(map[string]struct{}, len(cs))
		for _, c := range cs {
			branches[c.Branch] = struct{}{}
			accounts[c.AccountID] = struct{}{}
		}
		for b := range branches {
			byBranch[b].DistinctFiles++
		}
		if len(accounts) < 2 {
			continue
		}
		sorted := append([]Contributor(nil), cs...)
		sort.Slice(sorted, func(i, j int) bool {
			if sorted[i].Branch != sorted[j].Branch {
				return sorted[i].Branch < sorted[j].Branch
			}
			if sorted[i].Author != sorted[j].Author {
				return sorted[i].Author < sorted[j].Author
			}
			return sorted[i].AccountID < sorted[j].AccountID
		})
		for b := range branches {
			// Each branch gets its own copy so callers may mutate results.
			ov := FileOverlap{Path: f, Contributors: append([]Contributor(nil), sorted...)}
			byBranch[b].Overlaps = append(byBranch[b].Overlaps, ov)
		}
	}

	out := make([]BranchActivity, 0, len(order))
	for _, name := range order {
		ba := byBranch[name]
		sort.SliceStable(ba.Authors, func(i, j int) bool {
			a, b := ba.Authors[i], ba.Authors[j]
			if !a.StagedAt.Equal(b.StagedAt) {
				return a.StagedAt.After(b.StagedAt)
			}
			return a.Author < b.Author
		})
		out = append(out, *ba)
	}
	sort.SliceStable(out, func(i, j int) bool {
		if !out[i].LatestAt.Equal(out[j].LatestAt) {
			return out[i].LatestAt.After(out[j].LatestAt)
		}
		return out[i].Branch < out[j].Branch
	})
	return out
}
