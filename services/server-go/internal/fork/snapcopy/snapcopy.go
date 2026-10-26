// Package snapcopy copies snapshot records between the trees of a fork
// network (docs/ENGINEERING_PLAN.md §16.2, specs/tree/Forks.md §3).
//
// Snapshot records are tree-scoped (domain.Snapshot.TreeID; pushes, object
// downloads and the REST snapshot routes only accept records of the tree
// they address), so a fork cannot simply point its branches at its
// upstream's snapshots. Instead every snapshot a fork needs is copied into
// it as a new record: the same manifest, author, message and times, a new
// id and the fork's tree id. Objects (manifests, blobs, chunks) are
// content-addressed and shared, so nothing in the object store is copied.
//
// Ids are deterministic: the copy of snapshot S into tree T is the UUID v5
// DerivedID(T, S), unless S itself came from T (directly or through a chain
// of copies), in which case the original id is reused. Every copy records
// its origin in the `fork_snapshots` collection (key DocKey(treeID,
// snapshotID) → Origin), which is how a fork's snapshot maps back to the
// upstream snapshot it was copied from. So history copied upstream → fork →
// upstream (a merge request from a fork, then a fork sync) keeps its
// original ids, and repeated copies are idempotent.
package snapcopy

import (
	"context"
	"crypto/sha1" //nolint:gosec // UUID v5 is defined over SHA-1 (RFC 4122 §4.3)
	"errors"
	"fmt"
	"slices"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// Coll is the DocStore collection of copy origins.
const Coll = "fork_snapshots"

// maxChain bounds origin chains (fork of a fork of a fork...).
const maxChain = 64

// Origin is the snapshot a copied record was made from.
type Origin struct {
	TreeID     string `json:"origin_tree_id"`
	SnapshotID string `json:"origin_snapshot_id"`
}

// Store is the subset of domain.MetaStore the copier uses.
type Store interface {
	GetSnapshot(ctx context.Context, id string) (*domain.Snapshot, error)
	HasSnapshot(ctx context.Context, id string) (bool, error)
	PutSnapshot(ctx context.Context, s *domain.Snapshot) error
	GetDoc(ctx context.Context, coll, key string, v any) error
	PutDoc(ctx context.Context, coll, key string, v any) error
}

// namespace is the UUID v5 namespace of derived snapshot ids.
var namespace = [16]byte{0x5f, 0x6b, 0x0e, 0x2a, 0x93, 0x1d, 0x4c, 0x77, 0x9e, 0x41, 0x0b, 0x8c, 0x52, 0xd3, 0x7a, 0x10}

// DerivedID returns the id of the copy of snapshot id in tree dstTreeID
// when it has no origin there: a UUID v5 of the tree id and the snapshot id.
func DerivedID(dstTreeID, id string) string {
	h := sha1.New() //nolint:gosec // see import
	h.Write(namespace[:])
	h.Write([]byte(dstTreeID))
	h.Write([]byte{0})
	h.Write([]byte(id))
	var b [16]byte
	copy(b[:], h.Sum(nil))
	b[6] = b[6]&0x0f | 0x50
	b[8] = b[8]&0x3f | 0x80
	return fmt.Sprintf("%x-%x-%x-%x-%x", b[0:4], b[4:6], b[6:8], b[8:10], b[10:16])
}

// MappedID returns the id snapshot id of tree srcTreeID has (or would have)
// in tree dstTreeID: the original id when id was copied from dstTreeID
// (following the origin chain), else DerivedID(dstTreeID, id). The snapshot
// need not exist in dstTreeID yet.
func MappedID(ctx context.Context, st Store, srcTreeID, dstTreeID, id string) (string, error) {
	if srcTreeID == dstTreeID {
		return id, nil
	}
	chain := []string{id}
	cur, curTree := id, srcTreeID
	for range maxChain {
		var o Origin
		err := st.GetDoc(ctx, Coll, domain.DocKey(curTree, cur), &o)
		if errors.Is(err, domain.ErrNotFound) {
			break
		}
		if err != nil {
			return "", fmt.Errorf("snapcopy: origin of %s: %w", cur, err)
		}
		if o.TreeID == dstTreeID {
			return o.SnapshotID, nil
		}
		if o.TreeID == "" || o.SnapshotID == "" {
			break
		}
		chain = append(chain, o.SnapshotID)
		cur, curTree = o.SnapshotID, o.TreeID
	}
	// No origin in dstTreeID: an earlier copy of id, or of a snapshot id
	// was copied from (a fork of a fork sending work to the root), may
	// already be there.
	for _, c := range chain {
		d := DerivedID(dstTreeID, c)
		ok, err := st.HasSnapshot(ctx, d)
		if err != nil {
			return "", fmt.Errorf("snapcopy: snapshot %s: %w", d, err)
		}
		if ok {
			return d, nil
		}
	}
	return DerivedID(dstTreeID, id), nil
}

// Existing returns the id of id's counterpart in dstTreeID and whether that
// snapshot is stored there (without copying anything).
func Existing(ctx context.Context, st Store, srcTreeID, dstTreeID, id string) (string, bool, error) {
	mapped, err := MappedID(ctx, st, srcTreeID, dstTreeID, id)
	if err != nil {
		return "", false, err
	}
	s, err := st.GetSnapshot(ctx, mapped)
	switch {
	case errors.Is(err, domain.ErrNotFound):
		return mapped, false, nil
	case err != nil:
		return "", false, fmt.Errorf("snapcopy: snapshot %s: %w", mapped, err)
	}
	return mapped, s.TreeID == dstTreeID, nil
}

// Copy makes snapshot tip of tree srcTreeID and all its ancestors available
// in tree dstTreeID and returns the id of tip there. Snapshots already
// present are not copied again; their ancestors are assumed present.
func Copy(ctx context.Context, st Store, srcTreeID, dstTreeID, tip string) (string, error) {
	if tip == "" || srcTreeID == dstTreeID {
		return tip, nil
	}
	c := &copier{ctx: ctx, st: st, src: srcTreeID, dst: dstTreeID, done: map[string]string{}}
	return c.copy(tip)
}

// copier holds the state of one Copy.
type copier struct {
	ctx      context.Context
	st       Store
	src, dst string
	done     map[string]string // source id -> destination id
}

type frame struct {
	id       string
	mapped   string
	snap     *domain.Snapshot
	expanded bool
}

// copy walks the ancestry of tip depth-first (iteratively: histories can
// be long) and writes parents before children.
func (c *copier) copy(tip string) (string, error) {
	active := map[string]bool{}
	stack := []frame{{id: tip}}
	for len(stack) > 0 {
		top := len(stack) - 1
		f := stack[top]
		if _, ok := c.done[f.id]; ok {
			stack = stack[:top]
			continue
		}
		if !f.expanded {
			mapped, present, err := Existing(c.ctx, c.st, c.src, c.dst, f.id)
			if err != nil {
				return "", err
			}
			if present {
				c.done[f.id] = mapped
				stack = stack[:top]
				continue
			}
			snap, err := c.st.GetSnapshot(c.ctx, f.id)
			if err != nil {
				return "", fmt.Errorf("snapcopy: snapshot %s: %w", f.id, err)
			}
			if snap.TreeID != c.src {
				return "", fmt.Errorf("snapcopy: snapshot %s belongs to tree %s, not %s: %w", f.id, snap.TreeID, c.src, domain.ErrInvalid)
			}
			stack[top] = frame{id: f.id, mapped: mapped, snap: snap, expanded: true}
			active[f.id] = true
			for i := len(snap.Parents) - 1; i >= 0; i-- {
				p := snap.Parents[i]
				if _, ok := c.done[p]; ok {
					continue
				}
				if active[p] {
					return "", fmt.Errorf("snapcopy: snapshot ancestry contains a cycle through %s: %w", p, domain.ErrInvalid)
				}
				stack = append(stack, frame{id: p})
			}
			continue
		}
		if err := c.write(f); err != nil {
			return "", err
		}
		delete(active, f.id)
		c.done[f.id] = f.mapped
		stack = stack[:top]
	}
	return c.done[tip], nil
}

// write stores the copy of f.snap (its parents are copied) and its origin.
func (c *copier) write(f frame) error {
	s := f.snap
	parents := make([]string, len(s.Parents))
	for i, p := range s.Parents {
		parents[i] = c.done[p]
	}
	reverts := s.Reverts
	if reverts != "" {
		m, err := MappedID(c.ctx, c.st, c.src, c.dst, reverts)
		if err != nil {
			return err
		}
		reverts = m
	}
	cp := &domain.Snapshot{
		ID:            f.mapped,
		TreeID:        c.dst,
		BranchName:    s.BranchName,
		Parents:       parents,
		ManifestHash:  s.ManifestHash,
		Author:        s.Author,
		Message:       s.Message,
		CreatedAt:     s.CreatedAt,
		AutoGenerated: s.AutoGenerated,
		PushedBy:      s.PushedBy,
		PushedAt:      s.PushedAt,
		Dependencies:  slices.Clone(s.Dependencies),
		Reverts:       reverts,
		// Signatures cover the snapshot id and parents, which change: a
		// copy carries no signature (specs/tree/Forks.md §3).
	}
	// The origin is written first: a copy without its origin would map
	// back to a derived id instead of the original.
	if err := c.st.PutDoc(c.ctx, Coll, domain.DocKey(c.dst, cp.ID), Origin{TreeID: c.src, SnapshotID: s.ID}); err != nil {
		return fmt.Errorf("snapcopy: record origin of %s: %w", cp.ID, err)
	}
	if err := c.st.PutSnapshot(c.ctx, cp); err != nil {
		return fmt.Errorf("snapcopy: store copy of %s: %w", s.ID, err)
	}
	return nil
}
