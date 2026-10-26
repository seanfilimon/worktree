// Package codesearch indexes the branches of trees and answers code and
// symbol searches (docs/ENGINEERING_PLAN.md §14.3, §15.9).
//
// # Index
//
// The index of a tree covers the tips of the branches that were searched
// (the default branch unless a search names another branch or all
// branches): each text file of at most MaxFileSize bytes (binary files — a
// NUL byte in the first 8000 bytes — and chunked files are skipped) with a
// posting list per trigram (see trigram.go) and the symbols defined in it
// (see symbols.go). The index of one manifest is a segment (segment.go);
// branches whose tips have the same manifest share it. At most
// Options.MaxBranches branches are indexed per tree.
//
// With Options.Dir set, segments and a small state file per tree are kept
// on disk (store.go) and survive restarts; decoded segments are held in
// memory by an LRU cache capped at Options.MemoryLimit bytes and loaded
// lazily. A segment or state file with another format version, or one
// that fails its checksum, is discarded and rebuilt. The default branch's
// Meta is also stored in the code_index_meta document collection.
//
// A branch is indexed lazily by the first search that covers it. Every
// search re-reads the branch tips and rebuilds a stale branch;
// branch.pushed events for an indexed branch rebuild it eagerly,
// branch.deleted drops it and tree.deleted drops the tree's index.
// Rebuilds are incremental: blobs whose content hash appears in the
// branch's previous segment (or the default branch's) are not read from
// the object store again. A build is not cancelled by the search that
// started it: when the search's time budget runs out first, the build
// completes in the background for later searches.
//
// # Search
//
// Search does not authorize: the caller passes the trees the principal
// may read (tree:read) and a ReadFilter for per-path restrictions
// (path-deny policies, license read rules).
package codesearch

import (
	"context"
	"encoding/json"
	"errors"
	"log/slog"
	"os"
	"path/filepath"
	"sort"
	"sync"
	"sync/atomic"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/objstore"
)

// MetaCollection is the document collection holding the default-branch
// Meta of every indexed tree (key = tree id).
const MetaCollection = "code_index_meta"

// Defaults of Options.
const (
	DefaultMemoryLimit = 256 << 20
	DefaultMaxBranches = 10
)

// errForgotten reports a tree dropped (deleted) while a search waited for
// its index.
var errForgotten = errors.New("codesearch: tree was deleted while being indexed")

// buildTimeout bounds one index build.
const buildTimeout = 2 * time.Minute

// Deps are the collaborators of the codesearch package. Store and Objects
// are required; the others may be nil.
type Deps struct {
	Store   domain.MetaStore
	Objects objstore.Store
	History domain.History
	Authz   domain.Authorizer
	Bus     domain.EventBus
	Audit   domain.AuditLog
	Logger  *slog.Logger
}

// Options configure persistence and limits (Configure).
type Options struct {
	// Dir holds the on-disk index (<data>/codeindex); "" keeps the index
	// in memory only.
	Dir string
	// MemoryLimit caps the decoded segments kept in memory (bytes; 0 =
	// unlimited). Default DefaultMemoryLimit.
	MemoryLimit int64
	// MaxBranches is the most branches indexed per tree (default
	// DefaultMaxBranches).
	MaxBranches int
}

// Index is the code index of every searched tree. It is safe for
// concurrent use.
type Index struct {
	d   Deps
	opt Options

	mu    sync.Mutex
	trees map[string]*treeState
	// checked holds the tree ids whose on-disk state was consulted: after the
	// first look the in-memory state is authoritative (on Windows a file
	// being read cannot be deleted, so the disk is not polled).
	checked map[string]bool
	cache   *segCache

	ioMu    sync.Mutex
	ioLocks map[string]*sync.Mutex // per tree id: serialises state writes and GC

	builds    atomic.Int64 // segments built since start
	diskLoads atomic.Int64 // segments loaded from disk since start
}

// treeState is the index of one tree: its indexed branches.
type treeState struct {
	id            string
	defaultBranch string
	branches      map[string]*branchState
	pending       map[hash.Hash]int // manifests being built (kept by GC)
	gen           uint64            // bumped by Forget; stale builds do not install
}

// branchState is one indexed branch. ready means snapshotID/manifest/meta
// describe a usable segment (or an empty branch when manifest is zero).
type branchState struct {
	name       string
	ready      bool
	snapshotID string
	manifest   hash.Hash
	meta       Meta
	lastUsed   time.Time
	job        *buildJob // in-flight build; nil = none
	err        error     // error of the last build
}

// buildJob is one in-flight branch build; waiters read its result after
// done is closed.
type buildJob struct {
	done       chan struct{}
	snapshotID string
	seg        *segment // nil for an empty branch
	err        error
}

// New returns an empty in-memory Index; Configure enables persistence.
func New(d Deps) *Index {
	if d.Logger == nil {
		d.Logger = slog.Default()
	}
	ix := &Index{d: d, trees: map[string]*treeState{}, checked: map[string]bool{}, ioLocks: map[string]*sync.Mutex{},
		opt: Options{MemoryLimit: DefaultMemoryLimit, MaxBranches: DefaultMaxBranches}}
	ix.cache = newSegCache(ix.opt.MemoryLimit)
	return ix
}

// Configure sets the options; call it before the first search. Zero
// MaxBranches keeps the default; a negative MemoryLimit means unlimited.
func (ix *Index) Configure(o Options) {
	if o.MaxBranches <= 0 {
		o.MaxBranches = DefaultMaxBranches
	}
	if o.MemoryLimit < 0 {
		o.MemoryLimit = 0
	}
	ix.mu.Lock()
	ix.opt = o
	ix.mu.Unlock()
	ix.cache.setLimit(o.MemoryLimit)
	if o.Dir != "" {
		if err := os.MkdirAll(o.Dir, 0o755); err != nil {
			ix.d.Logger.Warn("codesearch: cannot create the index directory; indexes stay in memory", "dir", o.Dir, "err", err)
			ix.mu.Lock()
			ix.opt.Dir = ""
			ix.mu.Unlock()
		}
	}
}

// Start subscribes to branch.pushed, branch.deleted and tree.deleted until
// ctx ends.
func (ix *Index) Start(ctx context.Context) {
	if ix.d.Bus == nil {
		return
	}
	ch, cancel := ix.d.Bus.Subscribe(domain.EventFilter{
		Types: []string{domain.EventBranchPushed, domain.EventBranchDeleted, domain.EventTreeDeleted},
	})
	go func() {
		defer cancel()
		for {
			select {
			case <-ctx.Done():
				return
			case e, ok := <-ch:
				if !ok {
					return
				}
				if err := ix.HandleEvent(context.WithoutCancel(ctx), e); err != nil {
					ix.d.Logger.WarnContext(ctx, "codesearch: handling event failed",
						"type", e.Type, "tree", e.TreeID, "err", err)
				}
			}
		}
	}()
}

// HandleEvent processes one bus event synchronously (Start calls it for
// every subscribed event; tests call it directly). A push to an indexed
// branch rebuilds it; pushes to other branches and to trees that were
// never searched are ignored. branch.deleted drops the branch's index;
// tree.deleted drops the tree's index and metadata.
func (ix *Index) HandleEvent(ctx context.Context, e domain.Event) error {
	if e.TreeID == "" {
		return nil
	}
	switch e.Type {
	case domain.EventTreeDeleted:
		return ix.Forget(ctx, e.TreeID)
	case domain.EventBranchPushed, domain.EventBranchDeleted:
		if !ix.Indexed(e.TreeID) {
			return nil
		}
		tree, err := ix.d.Store.GetTree(ctx, e.TreeID)
		if errors.Is(err, domain.ErrNotFound) {
			return ix.Forget(ctx, e.TreeID)
		}
		if err != nil {
			return err
		}
		b := payloadBranch(e.Payload)
		if b == "" {
			b = tree.DefaultBranch
		}
		if e.Type == domain.EventBranchDeleted {
			ix.dropBranch(tree, b)
			return nil
		}
		ix.mu.Lock()
		ts := ix.stateLocked(tree.ID, false)
		indexed := ts != nil && ts.branches[b] != nil
		ix.mu.Unlock()
		if !indexed {
			return nil
		}
		_, err = ix.ensureBranch(ctx, tree, b)
		return err
	}
	return nil
}

// branchPayload is the union of the branch event payload shapes:
// {"branch",...} for pushes and domain.Branch for creations and deletions.
type branchPayload struct {
	Branch string `json:"branch"`
	Name   string `json:"name"`
}

// payloadBranch extracts the branch name of a branch event payload.
func payloadBranch(p any) string {
	var pl branchPayload
	switch v := p.(type) {
	case nil:
		return ""
	case map[string]any:
		pl.Branch, _ = v["branch"].(string)
		pl.Name, _ = v["name"].(string)
	case map[string]string:
		pl.Branch, pl.Name = v["branch"], v["name"]
	case domain.Branch:
		pl.Name = v.Name
	case *domain.Branch:
		if v != nil {
			pl.Name = v.Name
		}
	default:
		raw, err := json.Marshal(v)
		if err != nil || json.Unmarshal(raw, &pl) != nil {
			return ""
		}
	}
	if pl.Branch != "" {
		return pl.Branch
	}
	return pl.Name
}

// Indexed reports whether the tree has an index (in memory, on disk, or
// being built).
func (ix *Index) Indexed(treeID string) bool {
	ix.mu.Lock()
	defer ix.mu.Unlock()
	ts := ix.stateLocked(treeID, false)
	return ts != nil && len(ts.branches) > 0
}

// Status returns the Meta of the tree's default-branch index.
func (ix *Index) Status(treeID string) (Meta, bool) {
	ix.mu.Lock()
	defer ix.mu.Unlock()
	ts := ix.stateLocked(treeID, false)
	if ts == nil {
		return Meta{}, false
	}
	bs := ts.branches[ts.defaultBranch]
	if bs == nil || !bs.ready {
		return Meta{}, false
	}
	return bs.meta, true
}

// stateLocked returns the tree's state, loading it from disk on first use.
// With create, a missing state is created empty. ix.mu must be held.
func (ix *Index) stateLocked(treeID string, create bool) *treeState {
	if ts := ix.trees[treeID]; ts != nil {
		return ts
	}
	ts := &treeState{id: treeID, branches: map[string]*branchState{}, pending: map[hash.Hash]int{}}
	if dir := ix.treeDir(treeID); dir != "" && !ix.checked[treeID] {
		ix.checked[treeID] = true
		st, err := readState(dir)
		switch {
		case errors.Is(err, errSegFormat) || errors.Is(err, errSegCorrupt):
			ix.d.Logger.Warn("codesearch: discarding unusable index; it is rebuilt", "tree", treeID, "err", err)
			_ = os.RemoveAll(dir)
		case err != nil:
			ix.d.Logger.Warn("codesearch: cannot read the index state; the tree is reindexed", "tree", treeID, "err", err)
		case st != nil && st.TreeID == treeID:
			ts.defaultBranch = st.DefaultBranch
			for _, b := range st.Branches {
				bs := &branchState{name: b.Branch, ready: true, snapshotID: b.SnapshotID, meta: b.Meta, lastUsed: b.LastUsed}
				if b.Meta.Manifest != "" {
					m, err := hash.FromHex(b.Meta.Manifest)
					if err != nil {
						continue // unusable entry: the branch is rebuilt
					}
					bs.manifest = m
				}
				ts.branches[b.Branch] = bs
			}
		}
	}
	if !create && len(ts.branches) == 0 {
		return nil
	}
	ix.trees[treeID] = ts
	return ts
}

// ioLock returns the lock serialising disk writes of a tree.
func (ix *Index) ioLock(treeID string) *sync.Mutex {
	ix.ioMu.Lock()
	defer ix.ioMu.Unlock()
	l := ix.ioLocks[treeID]
	if l == nil {
		l = &sync.Mutex{}
		ix.ioLocks[treeID] = l
	}
	return l
}

// Forget drops the index of a tree, its files and its stored metadata.
func (ix *Index) Forget(ctx context.Context, treeID string) error {
	ix.mu.Lock()
	if ts := ix.trees[treeID]; ts != nil {
		ts.gen++
		delete(ix.trees, treeID)
	}
	ix.checked[treeID] = true
	ix.mu.Unlock()
	ix.cache.removeTree(treeID)
	if dir := ix.treeDir(treeID); dir != "" {
		l := ix.ioLock(treeID)
		l.Lock()
		err := removeAllRetry(dir)
		l.Unlock()
		if err != nil {
			ix.d.Logger.WarnContext(ctx, "codesearch: removing index files failed", "tree", treeID, "err", err)
		}
	}
	err := ix.d.Store.DeleteDoc(ctx, MetaCollection, treeID)
	if errors.Is(err, domain.ErrNotFound) {
		return nil
	}
	return err
}

// dropBranch removes one branch from a tree's index.
func (ix *Index) dropBranch(tree *domain.Tree, branch string) {
	ix.mu.Lock()
	ts := ix.stateLocked(tree.ID, false)
	if ts == nil || ts.branches[branch] == nil {
		ix.mu.Unlock()
		return
	}
	delete(ts.branches, branch)
	ix.mu.Unlock()
	ix.persist(context.Background(), tree.ID)
}

// branchTip returns the tip of a branch ("" for an empty one) and whether
// the branch exists.
func (ix *Index) branchTip(ctx context.Context, treeID, branch string) (string, bool, error) {
	b, err := ix.d.Store.GetBranch(ctx, treeID, branch)
	if errors.Is(err, domain.ErrNotFound) {
		return "", false, nil
	}
	if err != nil {
		return "", false, err
	}
	return b.Tip, true, nil
}

// view is one searchable branch tip of a tree.
type view struct {
	branch     string
	snapshotID string
	seg        *segment // nil = the branch is empty (or missing)
}

// ensureBranch returns the index of branch's current tip, building (or
// rebuilding, or loading from disk) it when needed. It waits for a build
// until ctx ends. A missing non-default branch yields (nil, nil); a
// missing default branch is indexed as empty.
func (ix *Index) ensureBranch(ctx context.Context, tree *domain.Tree, branch string) (*view, error) {
	tip, exists, err := ix.branchTip(ctx, tree.ID, branch)
	if err != nil {
		return nil, err
	}
	if !exists && branch != tree.DefaultBranch {
		return nil, nil
	}
	for attempt := 0; ; attempt++ {
		if attempt > 8 {
			return nil, errors.New("codesearch: index keeps changing; giving up")
		}
		ix.mu.Lock()
		ts := ix.stateLocked(tree.ID, true)
		ts.defaultBranch = tree.DefaultBranch
		bs := ts.branches[branch]
		if bs != nil && bs.job == nil && bs.ready && bs.snapshotID == tip {
			bs.lastUsed = time.Now().UTC()
			if bs.manifest.IsZero() {
				ix.mu.Unlock()
				return &view{branch: branch, snapshotID: tip}, nil
			}
			key, gen := segKey{tree.ID, bs.manifest}, ts.gen
			ix.mu.Unlock()
			if seg := ix.cache.get(key); seg != nil {
				return &view{branch: branch, snapshotID: tip, seg: seg}, nil
			}
			seg, err := ix.loadSegmentFile(tree.ID, key.manifest)
			if err == nil {
				ix.cache.put(key, seg)
				return &view{branch: branch, snapshotID: tip, seg: seg}, nil
			}
			// Missing (evicted without a data directory) or unusable:
			// rebuild the branch.
			ix.mu.Lock()
			if ts.gen == gen && ts.branches[branch] == bs && bs.manifest == key.manifest {
				bs.ready = false
			}
			ix.mu.Unlock()
			continue
		}
		if bs == nil {
			bs = &branchState{name: branch, lastUsed: time.Now().UTC()}
			ix.limitBranchesLocked(ts, tree.DefaultBranch)
			ts.branches[branch] = bs
		}
		job := bs.job
		if job == nil {
			job = &buildJob{done: make(chan struct{})}
			bs.job = job
			go ix.runBuild(ctx, tree, ts, bs, ts.gen, job)
		}
		ix.mu.Unlock()

		select {
		case <-ctx.Done():
			return nil, ctx.Err()
		case <-job.done:
		}
		if job.err != nil {
			return nil, job.err
		}
		if job.snapshotID == tip {
			if !ix.Indexed(tree.ID) {
				return nil, errForgotten
			}
			return &view{branch: branch, snapshotID: tip, seg: job.seg}, nil
		}
		// The build that finished was for another tip; re-check the tip
		// and build again.
		if tip, exists, err = ix.branchTip(ctx, tree.ID, branch); err != nil {
			return nil, err
		}
		if !exists && branch != tree.DefaultBranch {
			return nil, nil
		}
		if job.snapshotID == tip {
			return &view{branch: branch, snapshotID: tip, seg: job.seg}, nil
		}
	}
}

// limitBranchesLocked makes room for one more branch of ts by dropping the
// least recently used idle non-default branches beyond MaxBranches.
func (ix *Index) limitBranchesLocked(ts *treeState, defaultBranch string) {
	for len(ts.branches) >= ix.opt.MaxBranches {
		var victim *branchState
		for _, b := range ts.branches {
			if b.name == defaultBranch || b.job != nil {
				continue
			}
			if victim == nil || b.lastUsed.Before(victim.lastUsed) {
				victim = b
			}
		}
		if victim == nil {
			return
		}
		delete(ts.branches, victim.name)
	}
}

// segmentFor returns a tree's segment of manifest m from memory or disk
// (nil when neither has it).
func (ix *Index) segmentFor(treeID string, m hash.Hash) *segment {
	if m.IsZero() {
		return nil
	}
	key := segKey{treeID, m}
	if seg := ix.cache.get(key); seg != nil {
		return seg
	}
	seg, err := ix.loadSegmentFile(treeID, m)
	if err != nil {
		return nil
	}
	ix.cache.put(key, seg)
	return seg
}

// runBuild indexes bs's current tip detached from the requesting context
// and installs the result unless the tree was forgotten meanwhile.
func (ix *Index) runBuild(ctx context.Context, tree *domain.Tree, ts *treeState, bs *branchState, gen uint64, job *buildJob) {
	bctx, cancel := context.WithTimeout(context.WithoutCancel(ctx), buildTimeout)
	defer cancel()
	defer close(job.done)

	var (
		seg      *segment
		manifest hash.Hash
		meta     Meta
	)
	tip, _, err := ix.branchTip(bctx, tree.ID, bs.name)
	if err == nil && tip != "" {
		seg, manifest, err = ix.buildTip(bctx, tree, ts, bs, tip)
	}
	if err == nil {
		var st segStats
		if seg != nil {
			st = seg.stats
		} else {
			st.BuiltAt = time.Now().UTC()
		}
		meta = metaOf(tree.ID, bs.name, tip, manifest, st)
	} else {
		ix.d.Logger.WarnContext(bctx, "codesearch: index build failed", "tree", tree.ID, "branch", bs.name, "err", err)
	}

	ix.mu.Lock()
	current := ts.gen == gen && ix.trees[tree.ID] == ts && ts.branches[bs.name] == bs
	if current {
		bs.err = err
		if err == nil {
			bs.ready, bs.snapshotID, bs.manifest, bs.meta = true, tip, manifest, meta
		}
	}
	bs.job = nil
	if !manifest.IsZero() {
		if ts.pending[manifest]--; ts.pending[manifest] <= 0 {
			delete(ts.pending, manifest)
		}
	}
	_, forgotten := ix.trees[tree.ID]
	forgotten = !forgotten
	ix.mu.Unlock()
	job.snapshotID, job.seg, job.err = tip, seg, err
	if err != nil {
		return
	}
	if !current {
		if forgotten {
			// The tree was deleted during the build: leave nothing behind.
			_ = ix.Forget(bctx, tree.ID)
		}
		return
	}
	ix.persist(bctx, tree.ID)
	if bs.name == tree.DefaultBranch {
		if perr := ix.d.Store.PutDoc(bctx, MetaCollection, tree.ID, meta); perr != nil {
			ix.d.Logger.WarnContext(bctx, "codesearch: storing index metadata failed", "tree", tree.ID, "err", perr)
		}
	}
	ix.d.Logger.DebugContext(bctx, "codesearch: branch indexed", "tree", tree.ID, "branch", bs.name,
		"snapshot", tip, "files", meta.Files, "symbols", meta.Symbols, "reused", meta.Reused, "ms", meta.BuildMS)
}

// buildTip returns the segment of a snapshot's manifest: an existing one
// (another branch, or the disk, already has it) or a newly built one,
// written to disk and cached. The manifest is marked pending (kept by GC)
// until runBuild installs it.
func (ix *Index) buildTip(ctx context.Context, tree *domain.Tree, ts *treeState, bs *branchState, tip string) (*segment, hash.Hash, error) {
	snap, err := ix.d.Store.GetSnapshot(ctx, tip)
	if err != nil {
		return nil, hash.Hash{}, err
	}
	manifest := snap.ManifestHash
	ix.mu.Lock()
	ts.pending[manifest]++
	prevManifest := bs.manifest
	var defManifest hash.Hash
	if def := ts.branches[tree.DefaultBranch]; def != nil && def != bs && def.ready {
		defManifest = def.manifest
	}
	ix.mu.Unlock()

	// Deduplicate by manifest: an identical tree is indexed once.
	if seg := ix.segmentFor(tree.ID, manifest); seg != nil {
		return seg, manifest, nil
	}
	m, err := ix.readManifest(ctx, manifest)
	if err != nil {
		return nil, manifest, err
	}
	reuse := newReuseSet(ix.segmentFor(tree.ID, prevManifest), ix.segmentFor(tree.ID, defManifest))
	data, err := ix.buildSegment(ctx, m, reuse)
	if err != nil {
		return nil, manifest, err
	}
	seg, err := decodeSegment(manifest, data)
	if err != nil {
		return nil, manifest, err // a bug: encode and decode disagree
	}
	ix.builds.Add(1)
	if werr := ix.writeSegmentFile(tree.ID, manifest, data); werr != nil {
		ix.d.Logger.WarnContext(ctx, "codesearch: writing index segment failed; kept in memory only", "tree", tree.ID, "err", werr)
	}
	ix.cache.put(segKey{tree.ID, manifest}, seg)
	return seg, manifest, nil
}

// persist writes a tree's state file and deletes segments no branch uses.
// The state is captured under the tree's io lock, so the last write always
// reflects the latest state.
func (ix *Index) persist(ctx context.Context, treeID string) {
	dir := ix.treeDir(treeID)
	l := ix.ioLock(treeID)
	l.Lock()
	defer l.Unlock()

	ix.mu.Lock()
	ts := ix.trees[treeID]
	if ts == nil {
		ix.mu.Unlock()
		return
	}
	st := diskState{Format: FormatVersion, TreeID: treeID, DefaultBranch: ts.defaultBranch, Branches: []diskBranch{}}
	keep := map[string]bool{}
	used := map[hash.Hash]bool{}
	for _, b := range ts.branches {
		if !b.ready {
			continue
		}
		st.Branches = append(st.Branches, diskBranch{Branch: b.name, SnapshotID: b.snapshotID, Meta: b.meta, LastUsed: b.lastUsed})
		if !b.manifest.IsZero() {
			keep[segFileName(b.manifest)] = true
			used[b.manifest] = true
		}
	}
	for m := range ts.pending {
		keep[segFileName(m)] = true
		used[m] = true
	}
	for _, b := range ts.branches {
		if b.job != nil && !b.manifest.IsZero() {
			used[b.manifest] = true // the previous segment is still read by the build
		}
	}
	ix.mu.Unlock()

	// Segments no branch refers to leave memory too.
	ix.cache.mu.Lock()
	var drop []segKey
	for k := range ix.cache.m {
		if k.tree == treeID && !used[k.manifest] {
			drop = append(drop, k)
		}
	}
	ix.cache.mu.Unlock()
	for _, k := range drop {
		ix.cache.remove(k)
	}
	if dir == "" {
		return
	}
	sort.Slice(st.Branches, func(i, j int) bool { return st.Branches[i].Branch < st.Branches[j].Branch })
	raw, err := json.MarshalIndent(st, "", "  ")
	if err == nil {
		err = writeFileAtomic(filepath.Join(dir, stateFile), raw)
	}
	if err != nil {
		ix.d.Logger.WarnContext(ctx, "codesearch: writing index state failed", "tree", treeID, "err", err)
		return
	}
	gcDir(dir, keep)
}
