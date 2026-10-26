package syncsvc

// DownloadPack (docs/ENGINEERING_PLAN.md §16.1,
// crates/worktree-protocol/specs/sync/Packs.md).

import (
	"context"
	"errors"

	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"

	"github.com/seanfilimon/worktree/services/server-go/internal/content"
	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/objstore"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
)

// maxBaseManifests is how many leading `have` hashes are examined as base
// manifests (the client lists the manifests it bases its request on
// first).
const maxBaseManifests = 16

// plannedObject is one entry of a download pack.
type plannedObject struct {
	hash hash.Hash
	kind syncpb.ObjectKind
	base *hash.Hash
}

// DownloadPack streams, as a pack of PackChunk messages (at most 4 MiB
// each, offsets increasing), the objects in `want` that are reachable from
// the tree and readable by the caller (exactly like DownloadObjects with
// x-wt-tree-id), plus the chunks of wanted chunk lists, minus the objects
// in `have`. Unknown or unreadable hashes are skipped.
//
// With allow_deltas, objects are sent as deltas (when that saves at least
// 25 %) against a base the client has or an earlier entry: a blob or
// chunk list against the object at the same path in the previous manifest
// (the client's base manifests, i.e. the first manifests in `have`, then
// earlier wanted manifests), a chunk against the chunk at the same index
// of the base chunk list, and a manifest against the previous one. Bases
// from `have` must themselves be reachable and readable.
func (s *Service) DownloadPack(req *syncpb.DownloadPackRequest, stream grpc.ServerStreamingServer[syncpb.PackChunk]) error {
	if s.packOpts().Disabled {
		return status.Error(codes.Unimplemented, "pack downloads are disabled on this server")
	}
	ctx := stream.Context()
	t, p, err := s.readableTree(ctx, req.GetTreeId())
	if err != nil {
		return err
	}
	wanted, err := uniqueHashes(req.GetWant())
	if err != nil {
		return err
	}
	haves, err := uniqueHashes(req.GetHave())
	if err != nil {
		return err
	}
	have := make(map[hash.Hash]struct{}, len(haves))
	for _, h := range haves {
		have[h] = struct{}{}
	}
	reach, err := s.reachable(ctx, t, p, wanted)
	if err != nil {
		return s.toStatus(ctx, err, "resolve reachable objects")
	}

	// The sendable set, in request order, with chunk lists expanded.
	kinds := make(map[hash.Hash]syncpb.ObjectKind)
	var order []hash.Hash
	add := func(h hash.Hash, k syncpb.ObjectKind) {
		if _, dup := kinds[h]; dup {
			return
		}
		if _, ok := have[h]; ok {
			return
		}
		kinds[h] = k
		order = append(order, h)
	}
	chunkCache := make(map[hash.Hash][]hash.Hash)
	chunksOf := func(h hash.Hash) []hash.Hash {
		if c, ok := chunkCache[h]; ok {
			return c
		}
		var out []hash.Hash
		if data, err := s.d.Objects.Get(ctx, h); err == nil {
			if cl, err := content.DecodeChunkList(data); err == nil {
				out = content.ChunkHashes(cl)
			}
		}
		chunkCache[h] = out
		return out
	}
	for _, h := range wanted {
		k, ok := reach.kind[h]
		if !ok {
			continue
		}
		if k == syncpb.ObjectKind_OBJECT_KIND_CHUNK_LIST {
			for _, c := range chunksOf(h) {
				add(c, syncpb.ObjectKind_OBJECT_KIND_BLOB)
			}
		}
		add(h, k)
	}

	plan := s.planDownload(ctx, order, kinds, haves, chunksOf, req.GetAllowDeltas())
	if req.GetAllowDeltas() {
		s.checkHaveBases(ctx, t, p, plan, have)
	}

	sender := &packChunkSender{send: stream.Send}
	pw := newPackWriter(sender, true)
	for _, o := range plan {
		data, err := s.d.Objects.Get(ctx, o.hash)
		if errors.Is(err, objstore.ErrNotFound) {
			continue
		}
		if err != nil {
			return s.toStatus(ctx, err, "read object")
		}
		var baseData []byte
		base := o.base
		if base != nil {
			// An earlier entry may have been skipped (vanished object):
			// then only a base the client has is still valid.
			_, clientHas := have[*base]
			if !pw.has(*base) && !clientHas {
				base = nil
			} else if baseData, err = s.d.Objects.Get(ctx, *base); err != nil {
				base, baseData = nil, nil
			}
		}
		if _, err := pw.add(o.hash, packObjectKind(o.kind), data, base, baseData); err != nil {
			return err
		}
	}
	st, err := pw.finish()
	if err != nil {
		return err
	}
	if err := sender.flush(); err != nil {
		return err
	}
	countPack("download", st.PackBytes, st.ObjectBytes, st.Deltas)
	return nil
}

// planDownload orders the objects to send and picks a delta base
// candidate for each (validated later).
func (s *Service) planDownload(ctx context.Context, order []hash.Hash, kinds map[hash.Hash]syncpb.ObjectKind, haves []hash.Hash,
	chunksOf func(hash.Hash) []hash.Hash, deltas bool) []plannedObject {
	planned := make(map[hash.Hash]struct{}, len(order))
	var plan []plannedObject
	push := func(h hash.Hash, base *hash.Hash) {
		if _, dup := planned[h]; dup {
			return
		}
		if _, ok := kinds[h]; !ok {
			return
		}
		if !deltas || (base != nil && *base == h) {
			base = nil
		}
		planned[h] = struct{}{}
		plan = append(plan, plannedObject{hash: h, kind: kinds[h], base: base})
	}

	// path -> object of the most recent manifest, seeded from the client's
	// base manifests (first manifest wins).
	pathBase := make(map[string]hash.Hash)
	var prevManifest *hash.Hash
	if deltas {
		for i, h := range haves {
			if i >= maxBaseManifests {
				break
			}
			m, err := s.loadManifest(ctx, h)
			if err != nil {
				continue
			}
			if prevManifest == nil {
				mh := h
				prevManifest = &mh
			}
			for _, e := range m.GetEntries() {
				if _, ok := pathBase[e.GetPath()]; !ok {
					pathBase[e.GetPath()] = hash.Hash(e.GetBlobHash().GetValue())
				}
			}
		}
	}

	for _, mh := range order {
		if kinds[mh] != syncpb.ObjectKind_OBJECT_KIND_MANIFEST {
			continue
		}
		m, err := s.loadManifest(ctx, mh)
		if err != nil {
			continue
		}
		for _, e := range m.GetEntries() {
			eh := hash.Hash(e.GetBlobHash().GetValue())
			var base *hash.Hash
			if b, ok := pathBase[e.GetPath()]; ok {
				bb := b
				base = &bb
			}
			if e.GetChunked() {
				var baseChunks []hash.Hash
				if base != nil {
					baseChunks = chunksOf(*base)
				}
				for i, c := range chunksOf(eh) {
					var cb *hash.Hash
					if i < len(baseChunks) {
						bc := baseChunks[i]
						cb = &bc
					}
					push(c, cb)
				}
			}
			push(eh, base)
		}
		push(mh, prevManifest)
		for _, e := range m.GetEntries() {
			pathBase[e.GetPath()] = hash.Hash(e.GetBlobHash().GetValue())
		}
		mhc := mh
		prevManifest = &mhc
	}
	for _, h := range order {
		push(h, nil)
	}
	return plan
}

// checkHaveBases drops delta bases that are neither earlier entries of the
// plan nor objects the client has that it may read in t.
func (s *Service) checkHaveBases(ctx context.Context, t *domain.Tree, p *domain.Principal, plan []plannedObject, have map[hash.Hash]struct{}) {
	earlier := make(map[hash.Hash]struct{}, len(plan))
	var external []hash.Hash
	extSeen := make(map[hash.Hash]struct{})
	for _, o := range plan {
		if o.base != nil {
			if _, ok := earlier[*o.base]; !ok {
				if _, h := have[*o.base]; h {
					if _, dup := extSeen[*o.base]; !dup {
						extSeen[*o.base] = struct{}{}
						external = append(external, *o.base)
					}
				}
			}
		}
		earlier[o.hash] = struct{}{}
	}
	allowed := map[hash.Hash]struct{}{}
	if len(external) > 0 {
		if reach, err := s.reachable(ctx, t, p, external); err == nil {
			for h := range reach.kind {
				allowed[h] = struct{}{}
			}
		}
	}
	done := make(map[hash.Hash]struct{}, len(plan))
	for i := range plan {
		if b := plan[i].base; b != nil {
			_, prior := done[*b]
			_, ok := allowed[*b]
			if !prior && !ok {
				plan[i].base = nil
			}
		}
		done[plan[i].hash] = struct{}{}
	}
}

// packChunkSender is an io.Writer that sends PackChunk messages of at most
// PackMaxChunk bytes with their offsets.
type packChunkSender struct {
	send   func(*syncpb.PackChunk) error
	buf    []byte
	offset uint64
}

func (c *packChunkSender) Write(p []byte) (int, error) {
	n := len(p)
	for len(p) > 0 {
		room := PackMaxChunk - len(c.buf)
		take := min(room, len(p))
		c.buf = append(c.buf, p[:take]...)
		p = p[take:]
		if len(c.buf) == PackMaxChunk {
			if err := c.flush(); err != nil {
				return 0, err
			}
		}
	}
	return n, nil
}

func (c *packChunkSender) flush() error {
	if len(c.buf) == 0 {
		return nil
	}
	msg := &syncpb.PackChunk{Data: c.buf, Offset: c.offset}
	c.offset += uint64(len(c.buf))
	c.buf = make([]byte, 0, min(PackMaxChunk, 256<<10))
	return c.send(msg)
}
