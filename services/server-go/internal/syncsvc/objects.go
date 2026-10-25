package syncsvc

import (
	"context"
	"errors"
	"fmt"
	"io"
	"path"
	"strings"

	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/metadata"
	"google.golang.org/grpc/status"
	"google.golang.org/protobuf/proto"

	"github.com/seanfilimon/worktree/services/server-go/internal/content"
	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/objstore"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
)

// maxHashesPerRequest bounds the hash lists of Negotiate / DownloadObjects.
const maxHashesPerRequest = 1_000_000

// UploadObjects receives a stream of content-addressed objects.
//
// The caller must be authenticated (tree:write cannot be checked because
// objects are not tree-scoped; the push that references them performs the
// tree-level authorization). Each object is verified before it is stored:
//
//   - the hash must be 32 bytes and equal BLAKE3(data);
//   - data may not exceed MaxObjectSize;
//   - MANIFEST objects must decode as a valid ManifestRecord (see
//     decodeManifest);
//   - CHUNK_LIST objects must decode as a valid ChunkList (32-byte chunk
//     hashes, sizes summing to the total; content.DecodeChunkList).
//
// Objects failing verification are listed in `rejected`; they do not abort
// the stream. `stored` counts objects that were not present before.
func (s *Service) UploadObjects(stream grpc.ClientStreamingServer[syncpb.ObjectData, syncpb.UploadObjectsResponse]) error {
	ctx := stream.Context()
	if _, err := requireAuth(ctx); err != nil {
		return err
	}
	resp := &syncpb.UploadObjectsResponse{}
	for {
		obj, err := stream.Recv()
		if errors.Is(err, io.EOF) {
			return stream.SendAndClose(resp)
		}
		if err != nil {
			if _, ok := status.FromError(err); ok {
				return err
			}
			return s.toStatus(ctx, err, "receive object")
		}
		resp.Received++
		stored, ok, err := s.storeObject(ctx, obj)
		if err != nil {
			return err
		}
		if !ok {
			resp.Rejected = append(resp.Rejected, &syncpb.ContentHash{Value: append([]byte(nil), obj.GetHash().GetValue()...)})
			continue
		}
		if stored {
			resp.Stored++
		}
	}
}

// storeObject verifies and persists one uploaded object. ok=false means
// the object was rejected; err is reserved for server-side failures.
func (s *Service) storeObject(ctx context.Context, obj *syncpb.ObjectData) (stored, ok bool, err error) {
	h, herr := hashFromPB(obj.GetHash())
	if herr != nil {
		return false, false, nil
	}
	data := obj.GetData()
	if len(data) > MaxObjectSize {
		return false, false, nil
	}
	if hash.Sum(data) != h {
		return false, false, nil
	}
	switch obj.GetKind() {
	case syncpb.ObjectKind_OBJECT_KIND_MANIFEST:
		if _, err := decodeManifest(data); err != nil {
			s.log.DebugContext(ctx, "rejecting invalid manifest", "hash", h.Hex(), "err", err)
			return false, false, nil
		}
	case syncpb.ObjectKind_OBJECT_KIND_CHUNK_LIST:
		if _, err := content.DecodeChunkList(data); err != nil {
			s.log.DebugContext(ctx, "rejecting invalid chunk list", "hash", h.Hex(), "err", err)
			return false, false, nil
		}
	case syncpb.ObjectKind_OBJECT_KIND_BLOB, syncpb.ObjectKind_OBJECT_KIND_UNSPECIFIED:
		// Opaque bytes; UNSPECIFIED is accepted as a blob for leniency.
	default:
		return false, false, nil
	}
	exists, err := s.d.Objects.Has(ctx, h)
	if err != nil {
		return false, false, s.toStatus(ctx, err, "object store")
	}
	if exists {
		return false, true, nil
	}
	if err := s.d.Objects.Put(ctx, h, data); err != nil {
		if errors.Is(err, objstore.ErrHashMismatch) {
			return false, false, nil
		}
		// A concurrent upload of the same object can make Put fail (e.g.
		// "Access is denied" on a Windows rename onto an existing file).
		// Content addressing makes that object as good as ours.
		if exists, herr := s.d.Objects.Has(ctx, h); herr == nil && exists {
			return false, true, nil
		}
		return false, false, s.toStatus(ctx, err, "object store")
	}
	return true, true, nil
}

// errInvalidManifest wraps every decodeManifest failure.
var errInvalidManifest = errors.New("invalid manifest")

// decodeManifest parses and validates a ManifestRecord: entries sorted by
// path, unique, with clean relative '/'-separated paths and 32-byte blob
// hashes. Errors wrap errInvalidManifest.
func decodeManifest(data []byte) (*syncpb.ManifestRecord, error) {
	var m syncpb.ManifestRecord
	if err := proto.Unmarshal(data, &m); err != nil {
		return nil, fmt.Errorf("%w: not a ManifestRecord: %v", errInvalidManifest, err)
	}
	prev := ""
	for i, e := range m.GetEntries() {
		p := e.GetPath()
		if err := validateManifestPath(p); err != nil {
			return nil, fmt.Errorf("%w: entry %d: %v", errInvalidManifest, i, err)
		}
		if i > 0 && p <= prev {
			return nil, fmt.Errorf("%w: entry %d: path %q is not sorted after %q or is duplicated", errInvalidManifest, i, p, prev)
		}
		prev = p
		if len(e.GetBlobHash().GetValue()) != hash.Size {
			return nil, fmt.Errorf("%w: entry %d (%s): blob hash must be %d bytes", errInvalidManifest, i, p, hash.Size)
		}
	}
	return &m, nil
}

// validateManifestPath checks that p is a clean, relative, '/'-separated
// path.
func validateManifestPath(p string) error {
	switch {
	case p == "":
		return errors.New("empty path")
	case strings.HasPrefix(p, "/"):
		return fmt.Errorf("path %q is absolute", p)
	case strings.ContainsAny(p, "\\\x00"):
		return fmt.Errorf("path %q contains a backslash or NUL", p)
	case path.Clean(p) != p:
		return fmt.Errorf("path %q is not clean", p)
	case p == ".." || strings.HasPrefix(p, "../"):
		return fmt.Errorf("path %q escapes the tree", p)
	}
	return nil
}

// loadManifest reads and decodes the manifest stored under h.
func (s *Service) loadManifest(ctx context.Context, h hash.Hash) (*syncpb.ManifestRecord, error) {
	data, err := s.d.Objects.Get(ctx, h)
	if err != nil {
		return nil, err
	}
	return decodeManifest(data)
}

// DownloadObjects streams the requested objects; unknown hashes are
// skipped (docs/ENGINEERING_PLAN.md §13.2).
//
// Requests are tree-scoped: the client sends gRPC metadata
// `x-wt-tree-id: <tree id>`. The caller must be allowed to read that tree
// (anonymous callers can therefore download from public trees), and only
// objects reachable from the tree's branch tips or tags are served —
// manifests, blobs, chunk lists of large files and their chunks; other
// requested hashes are skipped as if unknown. Objects reachable only
// through paths the caller may not read (License.CheckRead) are refused
// the same way. Objects are labelled with their kind (BLOB / MANIFEST /
// CHUNK_LIST).
//
// Without the header only server admins may download (unscoped, kinds
// reported as UNSPECIFIED because the store does not record them); other
// callers get FailedPrecondition.
func (s *Service) DownloadObjects(req *syncpb.WantMessage, stream grpc.ServerStreamingServer[syncpb.ObjectData]) error {
	ctx := stream.Context()
	wanted, err := uniqueHashes(req.GetHashes())
	if err != nil {
		return err
	}

	var reach *reachability
	if treeID := treeIDFromMetadata(ctx); treeID != "" {
		t, p, err := s.readableTree(ctx, &syncpb.TreeId{Value: treeID})
		if err != nil {
			return err
		}
		reach, err = s.reachable(ctx, t, p, wanted)
		if err != nil {
			return s.toStatus(ctx, err, "resolve reachable objects")
		}
		if reach.denied > 0 {
			s.log.InfoContext(ctx, "download: withheld license-restricted objects",
				"tree", t.ID, "count", reach.denied)
		}
	} else {
		p, err := requireAuth(ctx)
		if err != nil {
			return status.Error(codes.Unauthenticated, "authentication required (or scope the request to a readable tree with "+TreeIDMetadataKey+")")
		}
		if !p.IsAdmin {
			return status.Error(codes.FailedPrecondition, "DownloadObjects requires "+TreeIDMetadataKey+" metadata naming the tree to download from")
		}
	}

	for _, h := range wanted {
		kind := syncpb.ObjectKind_OBJECT_KIND_UNSPECIFIED
		if reach != nil {
			k, ok := reach.kind[h]
			if !ok {
				continue
			}
			kind = k
		}
		data, err := s.d.Objects.Get(ctx, h)
		if errors.Is(err, objstore.ErrNotFound) {
			continue
		}
		if err != nil {
			return s.toStatus(ctx, err, "read object")
		}
		if err := stream.Send(&syncpb.ObjectData{Hash: hashToPB(h), Kind: kind, Data: data}); err != nil {
			return err
		}
	}
	return nil
}

// treeIDFromMetadata returns the x-wt-tree-id metadata value, or "".
func treeIDFromMetadata(ctx context.Context) string {
	md, ok := metadata.FromIncomingContext(ctx)
	if !ok {
		return ""
	}
	if v := md.Get(TreeIDMetadataKey); len(v) > 0 {
		return strings.TrimSpace(v[0])
	}
	return ""
}

// reachability is the subset of wanted objects reachable from a tree (and
// readable by the caller), with their kinds.
type reachability struct {
	kind map[hash.Hash]syncpb.ObjectKind
	// denied counts wanted objects reachable only through paths the caller
	// may not read.
	denied int
}

// reachable walks the snapshots reachable from every branch tip and tag of
// t (newest first) and records which of the wanted objects they reference:
// manifests, entry blobs, chunk lists and their chunks. Entry objects count
// only when p may read the entry's path (License.CheckRead); an object
// referenced by both a readable and an unreadable path is served. The walk
// stops early once every wanted object has been found, so the common case
// (fetching the objects of recent snapshots) reads only a few manifests.
func (s *Service) reachable(ctx context.Context, t *domain.Tree, p *domain.Principal, wanted []hash.Hash) (*reachability, error) {
	r := &reachability{kind: make(map[hash.Hash]syncpb.ObjectKind)}
	pending := make(map[hash.Hash]struct{}, len(wanted))
	for _, h := range wanted {
		pending[h] = struct{}{}
	}
	if len(pending) == 0 {
		return r, nil
	}
	denied := make(map[hash.Hash]struct{})

	var tips []string
	branches, err := s.d.Store.ListBranches(ctx, t.ID)
	if err != nil {
		return nil, err
	}
	for _, b := range branches {
		if b.Tip != "" {
			tips = append(tips, b.Tip)
		}
	}
	tags, err := s.d.Store.ListTags(ctx, t.ID)
	if err != nil {
		return nil, err
	}
	for _, tg := range tags {
		tips = append(tips, tg.Target)
	}

	readable := make(map[string]bool)
	canRead := func(path string) bool {
		if s.d.License == nil {
			return true
		}
		if ok, done := readable[path]; done {
			return ok
		}
		err := s.d.License.CheckRead(ctx, p, t, []string{path})
		if err != nil {
			var pd *domain.PushDenied
			if !errors.As(err, &pd) && !errors.Is(err, domain.ErrForbidden) {
				// Rules that cannot be evaluated deny (license contract).
				s.log.WarnContext(ctx, "download: license check failed", "tree", t.ID, "path", path, "err", err)
			}
		}
		readable[path] = err == nil
		return err == nil
	}
	note := func(h hash.Hash, kind syncpb.ObjectKind, ok bool) {
		if _, want := pending[h]; !want {
			return
		}
		if !ok {
			denied[h] = struct{}{}
			return
		}
		r.kind[h] = kind
		delete(pending, h)
		delete(denied, h)
	}
	chunkLists := make(map[hash.Hash][]hash.Hash)
	chunksOf := func(h hash.Hash) ([]hash.Hash, error) {
		if c, ok := chunkLists[h]; ok {
			return c, nil
		}
		data, err := s.d.Objects.Get(ctx, h)
		if err != nil {
			if errors.Is(err, objstore.ErrNotFound) {
				chunkLists[h] = nil
				return nil, nil
			}
			return nil, err
		}
		cl, err := content.DecodeChunkList(data)
		if err != nil {
			chunkLists[h] = nil
			return nil, nil
		}
		c := content.ChunkHashes(cl)
		chunkLists[h] = c
		return c, nil
	}

	seenSnap := make(map[string]struct{})
	seenManifest := make(map[hash.Hash]struct{})
	for _, tip := range tips {
		if _, done := seenSnap[tip]; done {
			continue
		}
		log, err := s.d.History.Log(ctx, tip, 0)
		if err != nil {
			if errors.Is(err, domain.ErrNotFound) {
				continue
			}
			return nil, err
		}
		for i := range log {
			snap := &log[i]
			if _, done := seenSnap[snap.ID]; done {
				continue
			}
			seenSnap[snap.ID] = struct{}{}
			if snap.TreeID != t.ID {
				continue
			}
			mh := snap.ManifestHash
			if _, done := seenManifest[mh]; done {
				continue
			}
			seenManifest[mh] = struct{}{}
			note(mh, syncpb.ObjectKind_OBJECT_KIND_MANIFEST, true)
			m, err := s.loadManifest(ctx, mh)
			if err != nil {
				if errors.Is(err, objstore.ErrNotFound) {
					continue
				}
				return nil, err
			}
			for _, e := range m.GetEntries() {
				bh := hash.Hash(e.GetBlobHash().GetValue())
				kind := syncpb.ObjectKind_OBJECT_KIND_BLOB
				if e.GetChunked() {
					kind = syncpb.ObjectKind_OBJECT_KIND_CHUNK_LIST
				}
				var chunks []hash.Hash
				if e.GetChunked() {
					if chunks, err = chunksOf(bh); err != nil {
						return nil, err
					}
				}
				_, wantEntry := pending[bh]
				wantChunk := false
				for _, c := range chunks {
					if _, ok := pending[c]; ok {
						wantChunk = true
						break
					}
				}
				if !wantEntry && !wantChunk {
					continue
				}
				ok := canRead(e.GetPath())
				note(bh, kind, ok)
				for _, c := range chunks {
					note(c, syncpb.ObjectKind_OBJECT_KIND_BLOB, ok)
				}
			}
			if len(pending) == 0 {
				return r, nil
			}
		}
	}
	r.denied = len(denied)
	return r, nil
}

// Negotiate returns, in objects_to_send, the subset of the client's `have`
// hashes that the server does not store, i.e. what the client must upload
// before pushing. total_size is always 0 (the server cannot know the sizes
// of objects it lacks). The `want` list is ignored. The caller must be
// authenticated, since the answer reveals which objects exist.
func (s *Service) Negotiate(ctx context.Context, req *syncpb.NegotiateRequest) (*syncpb.ObjectTransferPlan, error) {
	if _, err := requireAuth(ctx); err != nil {
		return nil, err
	}
	haves, err := uniqueHashes(req.GetHave().GetHashes())
	if err != nil {
		return nil, err
	}
	plan := &syncpb.ObjectTransferPlan{ObjectsToSend: make([]*syncpb.ContentHash, 0)}
	for _, h := range haves {
		ok, err := s.d.Objects.Has(ctx, h)
		if err != nil {
			return nil, s.toStatus(ctx, err, "object store")
		}
		if !ok {
			plan.ObjectsToSend = append(plan.ObjectsToSend, hashToPB(h))
		}
	}
	return plan, nil
}

// uniqueHashes decodes and dedups a hash list, preserving first-seen order.
func uniqueHashes(in []*syncpb.ContentHash) ([]hash.Hash, error) {
	if len(in) > maxHashesPerRequest {
		return nil, invalidf("too many hashes in one request (max %d)", maxHashesPerRequest)
	}
	out := make([]hash.Hash, 0, len(in))
	seen := make(map[hash.Hash]struct{}, len(in))
	for i, ch := range in {
		h, err := hashFromPB(ch)
		if err != nil {
			return nil, invalidf("hash %d: %v", i, err)
		}
		if _, dup := seen[h]; dup {
			continue
		}
		seen[h] = struct{}{}
		out = append(out, h)
	}
	return out, nil
}
