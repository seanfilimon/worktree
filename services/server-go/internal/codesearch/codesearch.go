// Package codesearch indexes the default branch of trees and answers code
// searches (docs/ENGINEERING_PLAN.md §14.3).
//
// # Index
//
// For every tree the index covers the tip of its default branch: each text
// file of at most MaxFileSize bytes (binary files — a NUL byte in the
// first 8000 bytes — and chunked files are skipped) with the set of its
// trigrams and a posting list per trigram (see trigram.go). Indexes live
// in memory; a small description of each (Meta) is stored in the
// code_index_meta document collection under the tree id.
//
// A tree is indexed lazily by the first search that covers it. Every
// search re-reads the default-branch tip and rebuilds a stale index;
// branch.pushed events for an indexed tree's default branch rebuild it
// eagerly, and tree.deleted drops it. Rebuilds are incremental: files
// whose content hash is unchanged keep their document (content and
// trigrams) and are not re-read from the object store. A build is not
// cancelled by the search that started it: when the search's time budget
// runs out first, the build completes in the background for later
// searches.
//
// # Search
//
// Search does not authorize: the caller passes the trees the principal
// may read (tree:read) and a ReadFilter for per-path restrictions
// (path-deny policies, license read rules).
package codesearch

import (
	"context"
	"errors"
	"log/slog"
	"sync"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/objstore"
)

// MetaCollection is the document collection holding one Meta per indexed
// tree (key = tree id).
const MetaCollection = "code_index_meta"

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

// Index is the in-memory code index of every searched tree. It is safe
// for concurrent use.
type Index struct {
	d Deps

	mu    sync.Mutex
	trees map[string]*treeState
}

// treeState holds the current index of one tree and its in-flight build.
type treeState struct {
	idx      *treeIndex
	building chan struct{} // closed when the in-flight build ends; nil = none
	err      error         // error of the last build
	gen      uint64        // bumped by Forget; stale builds do not install
}

// New returns an empty Index.
func New(d Deps) *Index {
	if d.Logger == nil {
		d.Logger = slog.Default()
	}
	return &Index{d: d, trees: map[string]*treeState{}}
}

// Start subscribes to branch.pushed and tree.deleted until ctx ends.
func (ix *Index) Start(ctx context.Context) {
	if ix.d.Bus == nil {
		return
	}
	ch, cancel := ix.d.Bus.Subscribe(domain.EventFilter{
		Types: []string{domain.EventBranchPushed, domain.EventTreeDeleted},
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
// every subscribed event; tests call it directly). A push to the default
// branch of an indexed tree rebuilds its index; pushes to other branches
// and to trees that were never searched are ignored. tree.deleted drops
// the tree's index and metadata.
func (ix *Index) HandleEvent(ctx context.Context, e domain.Event) error {
	if e.TreeID == "" {
		return nil
	}
	switch e.Type {
	case domain.EventTreeDeleted:
		return ix.Forget(ctx, e.TreeID)
	case domain.EventBranchPushed:
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
		if b := payloadBranch(e.Payload); b != "" && b != tree.DefaultBranch {
			return nil
		}
		_, err = ix.ensure(ctx, tree)
		return err
	}
	return nil
}

// payloadBranch extracts the "branch" field of a branch.pushed payload.
func payloadBranch(p any) string {
	switch v := p.(type) {
	case map[string]any:
		s, _ := v["branch"].(string)
		return s
	case map[string]string:
		return v["branch"]
	}
	return ""
}

// Indexed reports whether the tree has an index in memory (or one being
// built).
func (ix *Index) Indexed(treeID string) bool {
	ix.mu.Lock()
	defer ix.mu.Unlock()
	_, ok := ix.trees[treeID]
	return ok
}

// Status returns the metadata of the tree's current index.
func (ix *Index) Status(treeID string) (Meta, bool) {
	ix.mu.Lock()
	defer ix.mu.Unlock()
	st := ix.trees[treeID]
	if st == nil || st.idx == nil {
		return Meta{}, false
	}
	return st.idx.meta, true
}

// Forget drops the index of a tree and its stored metadata.
func (ix *Index) Forget(ctx context.Context, treeID string) error {
	ix.mu.Lock()
	if st := ix.trees[treeID]; st != nil {
		st.gen++
		delete(ix.trees, treeID)
	}
	ix.mu.Unlock()
	err := ix.d.Store.DeleteDoc(ctx, MetaCollection, treeID)
	if errors.Is(err, domain.ErrNotFound) {
		return nil
	}
	return err
}

// ensure returns an index of tree's current default-branch tip, building
// (or rebuilding) it when needed. It waits for the build until ctx ends.
func (ix *Index) ensure(ctx context.Context, tree *domain.Tree) (*treeIndex, error) {
	tip, err := ix.tip(ctx, tree)
	if err != nil {
		return nil, err
	}
	for {
		ix.mu.Lock()
		st := ix.trees[tree.ID]
		if st == nil {
			st = &treeState{}
			ix.trees[tree.ID] = st
		}
		if st.idx != nil && st.idx.snapshotID == tip && st.idx.branch == tree.DefaultBranch {
			idx := st.idx
			ix.mu.Unlock()
			return idx, nil
		}
		done := st.building
		if done == nil {
			done = make(chan struct{})
			st.building = done
			go ix.runBuild(ctx, tree, st, st.gen, st.idx, done)
		}
		ix.mu.Unlock()

		select {
		case <-ctx.Done():
			return nil, ctx.Err()
		case <-done:
		}
		ix.mu.Lock()
		idx, berr := st.idx, st.err
		ix.mu.Unlock()
		if berr != nil {
			return nil, berr
		}
		if idx != nil && idx.snapshotID == tip && idx.branch == tree.DefaultBranch {
			return idx, nil
		}
		// The build that finished was for an older tip (or the tree was
		// forgotten meanwhile); re-check the tip and build again.
		if tip, err = ix.tip(ctx, tree); err != nil {
			return nil, err
		}
		if idx != nil && idx.snapshotID == tip && idx.branch == tree.DefaultBranch {
			return idx, nil
		}
		if !ix.Indexed(tree.ID) {
			return nil, errForgotten
		}
	}
}

// runBuild builds tree's index detached from the requesting context and
// installs it unless the tree was forgotten meanwhile.
func (ix *Index) runBuild(ctx context.Context, tree *domain.Tree, st *treeState, gen uint64, prev *treeIndex, done chan struct{}) {
	bctx, cancel := context.WithTimeout(context.WithoutCancel(ctx), buildTimeout)
	defer cancel()
	defer close(done)
	idx, err := ix.build(bctx, tree, prev)
	if err != nil {
		ix.d.Logger.WarnContext(bctx, "codesearch: index build failed", "tree", tree.ID, "err", err)
	} else if perr := ix.d.Store.PutDoc(bctx, MetaCollection, tree.ID, idx.meta); perr != nil {
		ix.d.Logger.WarnContext(bctx, "codesearch: storing index metadata failed", "tree", tree.ID, "err", perr)
	}
	ix.mu.Lock()
	current := st.gen == gen && ix.trees[tree.ID] == st
	if current {
		st.err = err
		if err == nil {
			st.idx = idx
		}
	}
	st.building = nil
	ix.mu.Unlock()
	if err == nil && !current {
		// The tree was forgotten (deleted) during the build: do not leave
		// its metadata behind.
		_ = ix.d.Store.DeleteDoc(bctx, MetaCollection, tree.ID)
	}
	if err == nil && current {
		ix.d.Logger.DebugContext(bctx, "codesearch: tree indexed", "tree", tree.ID,
			"snapshot", idx.snapshotID, "files", idx.meta.Files, "reused", idx.meta.Reused, "ms", idx.meta.BuildMS)
	}
}
