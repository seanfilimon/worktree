package httpapi

import (
	"context"
	"errors"
	"fmt"
	"net/http"
	"slices"
	"time"

	"google.golang.org/protobuf/proto"

	"github.com/seanfilimon/worktree/services/server-go/internal/content"
	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/objstore"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
)

// Server integrity checks (docs/ENGINEERING_PLAN.md §14.6).

// registerFsckRoutes registers the v0.6 routes for server integrity checks (§14.6).
func (a *api) registerFsckRoutes() {
	a.handle("POST /api/v1/admin/fsck", a.handleFsck)
}

// fsckReport is the response of POST /api/v1/admin/fsck.
type fsckReport struct {
	// ObjectsChecked is the number of stored objects whose content was
	// read back and re-hashed.
	ObjectsChecked int64 `json:"objects_checked"`
	// Corrupt lists objects whose stored bytes do not decode to content
	// with their hash, and manifests / chunk lists that do not parse.
	Corrupt []string `json:"corrupt"`
	// Missing lists objects referenced by a snapshot reachable from a
	// branch or tag that are not stored (each object once).
	Missing []fsckMissing `json:"missing"`
	// MissingSnapshots lists snapshot ids referenced by a branch, tag or
	// parent link that have no snapshot record.
	MissingSnapshots []string `json:"missing_snapshots"`
	// SnapshotsChecked counts the reachable snapshots walked.
	SnapshotsChecked int64 `json:"snapshots_checked"`
	DurationMS       int64 `json:"duration_ms"`
}

type fsckMissing struct {
	Snapshot string `json:"snapshot"`
	Object   string `json:"object"`
}

// handleFsck answers POST /api/v1/admin/fsck (server admins only).
func (a *api) handleFsck(w http.ResponseWriter, r *http.Request) error {
	if err := a.requireServerAdmin(r); err != nil {
		return err
	}
	if a.d.Objects == nil {
		return errNotImplemented("integrity checks")
	}
	rep, err := runFsck(r.Context(), a.d.Store, a.d.Objects)
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusOK, rep)
	return nil
}

// fsckRun holds the state of one integrity check.
type fsckRun struct {
	ctx   context.Context
	store domain.MetaStore
	objs  objstore.Store
	rep   *fsckReport

	corrupt   map[hash.Hash]bool
	present   map[hash.Hash]bool // Has cache
	missing   map[hash.Hash]bool // already reported
	noSnap    map[string]bool
	visited   map[string]bool
	manifests map[hash.Hash]bool // expanded
}

// runFsck re-hashes every stored object (when the store can list them) and
// walks every snapshot reachable from a branch tip or tag of any tree,
// checking that its manifest, blobs, chunk lists and chunks are present
// and decode.
func runFsck(ctx context.Context, store domain.MetaStore, objs objstore.Store) (*fsckReport, error) {
	start := time.Now()
	f := &fsckRun{
		ctx: ctx, store: store, objs: objs,
		rep:     &fsckReport{Corrupt: []string{}, Missing: []fsckMissing{}, MissingSnapshots: []string{}},
		corrupt: map[hash.Hash]bool{}, present: map[hash.Hash]bool{}, missing: map[hash.Hash]bool{},
		noSnap: map[string]bool{}, visited: map[string]bool{}, manifests: map[hash.Hash]bool{},
	}
	if err := f.checkObjects(); err != nil {
		return nil, err
	}
	if err := f.checkHistory(); err != nil {
		return nil, err
	}
	for h := range f.corrupt {
		f.rep.Corrupt = append(f.rep.Corrupt, h.Hex())
	}
	slices.Sort(f.rep.Corrupt)
	slices.Sort(f.rep.MissingSnapshots)
	f.rep.DurationMS = time.Since(start).Milliseconds()
	return f.rep, nil
}

// checkObjects reads back every stored object and compares its hash.
func (f *fsckRun) checkObjects() error {
	ls, ok := f.objs.(objstore.ListStore)
	if !ok {
		return nil
	}
	return ls.List(f.ctx, func(h hash.Hash) error {
		data, err := f.objs.Get(f.ctx, h)
		switch {
		case errors.Is(err, objstore.ErrNotFound):
			return nil // deleted concurrently (gc)
		case err != nil && f.ctx.Err() != nil:
			return f.ctx.Err()
		}
		f.rep.ObjectsChecked++
		f.present[h] = true
		if err != nil || hash.Sum(data) != h {
			f.corrupt[h] = true
		}
		return nil
	})
}

// checkHistory walks the snapshots reachable from every branch and tag.
func (f *fsckRun) checkHistory() error {
	trees, err := f.store.ListTrees(f.ctx, "")
	if err != nil {
		return fmt.Errorf("list trees: %w", err)
	}
	for _, t := range trees {
		var roots []string
		branches, err := f.store.ListBranches(f.ctx, t.ID)
		if err != nil {
			return fmt.Errorf("list branches of %s: %w", t.ID, err)
		}
		for _, b := range branches {
			roots = append(roots, b.Tip)
		}
		tags, err := f.store.ListTags(f.ctx, t.ID)
		if err != nil {
			return fmt.Errorf("list tags of %s: %w", t.ID, err)
		}
		for _, tg := range tags {
			roots = append(roots, tg.Target)
		}
		if err := f.walk(roots); err != nil {
			return err
		}
	}
	return nil
}

func (f *fsckRun) walk(stack []string) error {
	for len(stack) > 0 {
		if err := f.ctx.Err(); err != nil {
			return err
		}
		id := stack[len(stack)-1]
		stack = stack[:len(stack)-1]
		if id == "" || f.visited[id] {
			continue
		}
		f.visited[id] = true
		s, err := f.store.GetSnapshot(f.ctx, id)
		if errors.Is(err, domain.ErrNotFound) {
			if !f.noSnap[id] {
				f.noSnap[id] = true
				f.rep.MissingSnapshots = append(f.rep.MissingSnapshots, id)
			}
			continue
		}
		if err != nil {
			return fmt.Errorf("load snapshot %s: %w", id, err)
		}
		f.rep.SnapshotsChecked++
		stack = append(stack, s.Parents...)
		if err := f.checkManifest(s.ID, s.ManifestHash); err != nil {
			return err
		}
	}
	return nil
}

// has reports (and caches) whether h is stored, recording it as missing
// for snapshot otherwise.
func (f *fsckRun) has(snapshot string, h hash.Hash) (bool, error) {
	if f.present[h] {
		return true, nil
	}
	ok, err := f.objs.Has(f.ctx, h)
	if err != nil {
		return false, fmt.Errorf("stat object %s: %w", h, err)
	}
	if ok {
		f.present[h] = true
		return true, nil
	}
	if !f.missing[h] {
		f.missing[h] = true
		f.rep.Missing = append(f.rep.Missing, fsckMissing{Snapshot: snapshot, Object: h.Hex()})
	}
	return false, nil
}

// read returns the content of a present object; ok is false when it is
// corrupt (and recorded as such).
func (f *fsckRun) read(h hash.Hash) ([]byte, bool, error) {
	if f.corrupt[h] {
		return nil, false, nil
	}
	data, err := f.objs.Get(f.ctx, h)
	if err != nil {
		if f.ctx.Err() != nil {
			return nil, false, f.ctx.Err()
		}
		f.corrupt[h] = true
		return nil, false, nil
	}
	return data, true, nil
}

func (f *fsckRun) checkManifest(snapshot string, mh hash.Hash) error {
	if f.manifests[mh] {
		return nil
	}
	f.manifests[mh] = true
	ok, err := f.has(snapshot, mh)
	if err != nil || !ok {
		return err
	}
	raw, ok, err := f.read(mh)
	if err != nil || !ok {
		return err
	}
	var m syncpb.ManifestRecord
	if err := proto.Unmarshal(raw, &m); err != nil {
		f.corrupt[mh] = true
		return nil
	}
	for _, e := range m.GetEntries() {
		h, err := content.EntryHash(e)
		if err != nil {
			f.corrupt[mh] = true
			continue
		}
		ok, err := f.has(snapshot, h)
		if err != nil {
			return err
		}
		if !ok || !e.GetChunked() || f.manifests[h] {
			continue
		}
		f.manifests[h] = true // chunk lists are expanded once too
		data, ok, err := f.read(h)
		if err != nil {
			return err
		}
		if !ok {
			continue
		}
		cl, err := content.DecodeChunkList(data)
		if err != nil {
			f.corrupt[h] = true
			continue
		}
		for _, c := range content.ChunkHashes(cl) {
			if _, err := f.has(snapshot, c); err != nil {
				return err
			}
		}
	}
	return nil
}
