package codesearch

import (
	"context"
	"errors"
	"sort"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// Index status and rebuilds for server administrators (§15.9).

// IndexStatus describes the whole code index.
type IndexStatus struct {
	FormatVersion int `json:"format_version"`
	// Persistent reports whether indexes are kept on disk.
	Persistent bool `json:"persistent"`
	// MemoryBytes is the size of the decoded segments in memory;
	// MemoryLimit their cap (0 = unlimited).
	MemoryBytes    int64 `json:"memory_bytes"`
	MemoryLimit    int64 `json:"memory_limit"`
	LoadedSegments int   `json:"loaded_segments"`
	// Evictions counts segments dropped from memory since the start.
	Evictions int64 `json:"evictions"`
	DiskBytes int64 `json:"disk_bytes"`
	// MaxBranches is the most branches indexed per tree.
	MaxBranches int `json:"max_branches"`
	// Builds counts segments built since the server started; DiskLoads
	// segments loaded from disk.
	Builds    int64           `json:"builds"`
	DiskLoads int64           `json:"disk_loads"`
	Trees     []CodeIndexTree `json:"trees"`
}

// CodeIndexTree is the index status of one tree.
type CodeIndexTree struct {
	TreeID        string `json:"tree_id"`
	TenantSlug    string `json:"tenant"`
	Tree          string `json:"tree"`
	DefaultBranch string `json:"default_branch"`
	// State is "ready", "building" or "error".
	State string `json:"state"`
	// Files, Bytes, Symbols and Trigrams describe the default branch.
	Files       int       `json:"files"`
	Bytes       int64     `json:"bytes"`
	Symbols     int       `json:"symbols"`
	Trigrams    int       `json:"trigrams"`
	BuiltAt     time.Time `json:"built_at"` // default branch; zero when not built yet
	BranchCount int       `json:"branch_count"`
	// Segments counts distinct manifests (branches with identical tips share one).
	Segments    int               `json:"segments"`
	MemoryBytes int64             `json:"memory_bytes"`
	DiskBytes   int64             `json:"disk_bytes"`
	Branches    []CodeIndexBranch `json:"branches"`
	Error       string            `json:"error,omitempty"`
}

// CodeIndexBranch is the index status of one branch of a tree.
type CodeIndexBranch struct {
	Branch     string    `json:"branch"`
	SnapshotID string    `json:"snapshot_id"`
	Manifest   string    `json:"manifest"`
	Files      int       `json:"files"`
	Bytes      int64     `json:"bytes"`
	Symbols    int       `json:"symbols"`
	Reused     int       `json:"reused"`
	BuiltAt    time.Time `json:"built_at"`
	BuildMS    int64     `json:"build_ms"`
	Loaded     bool      `json:"loaded"` // segment in memory
	Building   bool      `json:"building"`
	Error      string    `json:"error,omitempty"`
}

// RebuildResult is the answer to a rebuild.
type RebuildResult struct {
	Rebuilt int             `json:"rebuilt"`
	Trees   []CodeIndexTree `json:"trees"`
}

// IndexStatus returns the status of every indexed tree (in memory or on disk).
// Indexes of trees that no longer exist are dropped.
func (ix *Index) IndexStatus(ctx context.Context) (*IndexStatus, error) {
	ids := map[string]bool{}
	ix.mu.Lock()
	for id, ts := range ix.trees {
		if len(ts.branches) > 0 {
			ids[id] = true
		}
	}
	persistent, maxBranches := ix.opt.Dir != "", ix.opt.MaxBranches
	ix.mu.Unlock()
	for _, id := range ix.listTreeDirs() {
		ids[id] = true
	}
	used, limit, n, ev := ix.cache.stats()
	out := &IndexStatus{FormatVersion: FormatVersion, Persistent: persistent, MemoryBytes: used, MemoryLimit: limit,
		LoadedSegments: n, Evictions: ev, MaxBranches: maxBranches, Builds: ix.builds.Load(), DiskLoads: ix.diskLoads.Load(),
		Trees: []CodeIndexTree{}}
	slugs := map[string]string{}
	for id := range ids {
		tree, err := ix.d.Store.GetTree(ctx, id)
		if errors.Is(err, domain.ErrNotFound) {
			_ = ix.Forget(ctx, id) // the tree was deleted while the server was down
			continue
		}
		if err != nil {
			return nil, err
		}
		st, ok := ix.treeStatus(ctx, tree, slugs)
		if !ok {
			continue
		}
		out.DiskBytes += st.DiskBytes
		out.Trees = append(out.Trees, st)
	}
	sort.Slice(out.Trees, func(i, j int) bool {
		if out.Trees[i].TenantSlug != out.Trees[j].TenantSlug {
			return out.Trees[i].TenantSlug < out.Trees[j].TenantSlug
		}
		return out.Trees[i].Tree < out.Trees[j].Tree
	})
	return out, nil
}

// TreeStatus returns the status of one tree's index (ok=false when the
// tree has none).
func (ix *Index) TreeStatus(ctx context.Context, tree *domain.Tree) (CodeIndexTree, bool) {
	return ix.treeStatus(ctx, tree, map[string]string{})
}

func (ix *Index) treeStatus(ctx context.Context, tree *domain.Tree, slugs map[string]string) (CodeIndexTree, bool) {
	slug, ok := slugs[tree.TenantID]
	if !ok {
		if t, err := ix.d.Store.GetTenant(ctx, tree.TenantID); err == nil {
			slug = t.Slug
		}
		slugs[tree.TenantID] = slug
	}
	st := CodeIndexTree{TreeID: tree.ID, TenantSlug: slug, Tree: tree.Name, DefaultBranch: tree.DefaultBranch,
		State: "ready", Branches: []CodeIndexBranch{}}
	ix.mu.Lock()
	ts := ix.stateLocked(tree.ID, false)
	if ts == nil || len(ts.branches) == 0 {
		ix.mu.Unlock()
		return st, false
	}
	manifests := map[string]bool{}
	for _, b := range ts.branches {
		cb := CodeIndexBranch{Branch: b.name, SnapshotID: b.snapshotID, Building: b.job != nil}
		if b.ready {
			cb.Manifest, cb.Files, cb.Bytes, cb.Symbols = b.meta.Manifest, b.meta.Files, b.meta.Bytes, b.meta.Symbols
			cb.Reused, cb.BuiltAt, cb.BuildMS = b.meta.Reused, b.meta.BuiltAt, b.meta.BuildMS
			if !b.manifest.IsZero() {
				manifests[b.meta.Manifest] = true
				cb.Loaded = ix.cache.has(segKey{tree.ID, b.manifest})
			}
		}
		if b.err != nil {
			cb.Error = b.err.Error()
		}
		if b.name == tree.DefaultBranch && b.ready {
			st.Files, st.Bytes, st.Symbols, st.Trigrams, st.BuiltAt = b.meta.Files, b.meta.Bytes, b.meta.Symbols, b.meta.Trigrams, b.meta.BuiltAt
		}
		switch {
		case cb.Building && st.State != "error":
			st.State = "building"
		case cb.Error != "" && !b.ready:
			st.State, st.Error = "error", cb.Error
		}
		st.Branches = append(st.Branches, cb)
	}
	ix.mu.Unlock()
	sort.Slice(st.Branches, func(i, j int) bool {
		bi, bj := st.Branches[i].Branch == tree.DefaultBranch, st.Branches[j].Branch == tree.DefaultBranch
		if bi != bj {
			return bi
		}
		return st.Branches[i].Branch < st.Branches[j].Branch
	})
	st.BranchCount = len(st.Branches)
	st.Segments = len(manifests)
	st.MemoryBytes, _ = ix.cache.treeUsage(tree.ID)
	st.DiskBytes = dirUsage(ix.treeDir(tree.ID))
	return st, true
}

// Rebuild discards a tree's index (memory and disk) and builds its
// branches again from the object store: the branches that were indexed,
// or the default branch when there were none.
func (ix *Index) Rebuild(ctx context.Context, tree *domain.Tree) (CodeIndexTree, error) {
	ix.mu.Lock()
	var branches []string
	if ts := ix.stateLocked(tree.ID, false); ts != nil {
		for name := range ts.branches {
			branches = append(branches, name)
		}
	}
	ix.mu.Unlock()
	if len(branches) == 0 {
		branches = []string{tree.DefaultBranch}
	}
	sort.Strings(branches)
	if err := ix.Forget(ctx, tree.ID); err != nil {
		return CodeIndexTree{}, err
	}
	for _, b := range branches {
		if _, err := ix.ensureBranch(ctx, tree, b); err != nil {
			return CodeIndexTree{}, err
		}
	}
	st, _ := ix.TreeStatus(ctx, tree)
	return st, nil
}

// RebuildAll rebuilds every indexed tree.
func (ix *Index) RebuildAll(ctx context.Context) (*RebuildResult, error) {
	status, err := ix.IndexStatus(ctx)
	if err != nil {
		return nil, err
	}
	out := &RebuildResult{Trees: []CodeIndexTree{}}
	for _, t := range status.Trees {
		tree, err := ix.d.Store.GetTree(ctx, t.TreeID)
		if errors.Is(err, domain.ErrNotFound) {
			continue
		}
		if err != nil {
			return nil, err
		}
		st, err := ix.Rebuild(ctx, tree)
		if err != nil {
			return nil, err
		}
		out.Rebuilt++
		out.Trees = append(out.Trees, st)
	}
	return out, nil
}
