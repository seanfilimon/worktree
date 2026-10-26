package syncsvc

// Resumable pack uploads (docs/ENGINEERING_PLAN.md §16.1,
// crates/worktree-protocol/specs/sync/Packs.md): UploadPack, QueryUpload,
// the spool directory and its 24 h cleanup.

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"sync"
	"time"

	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/limits"
	"github.com/seanfilimon/worktree/services/server-go/internal/opsmetrics"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
)

// CollPackUploads holds one document per interrupted (or running) pack
// upload, keyed by upload id.
const CollPackUploads = "pack_uploads"

// Pack defaults.
const (
	DefaultMaxPackSize      = 4 << 30
	DefaultUploadRetention  = 24 * time.Hour
	defaultJanitorInterval  = time.Hour
	maxUploadIDLen          = 64
	packSpoolSuffix         = ".pack"
	maxExternalBasesChecked = 100_000
)

// PackLimits reports a tenant's effective quotas (implemented by
// *limits.Limiter).
type PackLimits interface {
	Effective(t *domain.Tenant) domain.TenantLimits
}

// PackOptions configures the pack RPCs (see ConfigurePacks).
type PackOptions struct {
	// SpoolDir keeps the received prefix of uploads (<data>/uploads). Empty
	// disables UploadPack and QueryUpload (Unimplemented).
	SpoolDir string
	// MaxPackSize bounds one upload in bytes (0 = DefaultMaxPackSize).
	MaxPackSize int64
	// Disabled makes every pack RPC answer Unimplemented (clients fall back
	// to UploadObjects / DownloadObjects).
	Disabled bool
	// Limits enforces the tenant storage quota on uploads (nil = none).
	Limits PackLimits
	// Retention is how long an interrupted upload is kept (0 = 24 h).
	Retention time.Duration
	// JanitorInterval is the cleanup period of RunPackJanitor (0 = 1 h).
	JanitorInterval time.Duration
}

// packState is the pack configuration plus the uploads in progress.
type packState struct {
	mu     sync.Mutex
	opts   PackOptions
	active map[string]struct{}
}

// packUpload is a pack_uploads document.
type packUpload struct {
	ID        string    `json:"id"`
	AccountID string    `json:"account_id"`
	TreeID    string    `json:"tree_id,omitempty"`
	CreatedAt time.Time `json:"created_at"`
	UpdatedAt time.Time `json:"updated_at"`
}

// ConfigurePacks sets the pack options; call before serving.
func (s *Service) ConfigurePacks(o PackOptions) {
	if o.MaxPackSize <= 0 {
		o.MaxPackSize = DefaultMaxPackSize
	}
	if o.Retention <= 0 {
		o.Retention = DefaultUploadRetention
	}
	if o.JanitorInterval <= 0 {
		o.JanitorInterval = defaultJanitorInterval
	}
	s.packs.mu.Lock()
	s.packs.opts = o
	s.packs.mu.Unlock()
}

func (s *Service) packOpts() PackOptions {
	s.packs.mu.Lock()
	defer s.packs.mu.Unlock()
	o := s.packs.opts
	if o.MaxPackSize <= 0 {
		o.MaxPackSize = DefaultMaxPackSize
	}
	if o.Retention <= 0 {
		o.Retention = DefaultUploadRetention
	}
	return o
}

// lockUpload marks id as in progress; false when another stream has it.
func (s *Service) lockUpload(id string) (unlock func(), ok bool) {
	s.packs.mu.Lock()
	defer s.packs.mu.Unlock()
	if s.packs.active == nil {
		s.packs.active = make(map[string]struct{})
	}
	if _, busy := s.packs.active[id]; busy {
		return nil, false
	}
	s.packs.active[id] = struct{}{}
	return func() {
		s.packs.mu.Lock()
		delete(s.packs.active, id)
		s.packs.mu.Unlock()
	}, true
}

func (s *Service) uploadActive(id string) bool {
	s.packs.mu.Lock()
	defer s.packs.mu.Unlock()
	_, ok := s.packs.active[id]
	return ok
}

// validUploadID: 1-64 characters of [A-Za-z0-9_-].
func validUploadID(id string) bool {
	if id == "" || len(id) > maxUploadIDLen {
		return false
	}
	for _, r := range id {
		if !(r >= 'a' && r <= 'z' || r >= 'A' && r <= 'Z' || r >= '0' && r <= '9' || r == '-' || r == '_') {
			return false
		}
	}
	return true
}

func spoolPath(dir, id string) string { return filepath.Join(dir, id+packSpoolSuffix) }

// QueryUpload reports how many bytes of an interrupted upload the server
// kept (the offset to resume from). Only the account that started the
// upload sees it; for everybody else it is not found.
func (s *Service) QueryUpload(ctx context.Context, req *syncpb.QueryUploadRequest) (*syncpb.QueryUploadResponse, error) {
	o := s.packOpts()
	if o.Disabled || o.SpoolDir == "" {
		return nil, status.Error(codes.Unimplemented, "pack uploads are disabled on this server")
	}
	p, err := requireAuth(ctx)
	if err != nil {
		return nil, err
	}
	id := req.GetUploadId()
	if !validUploadID(id) {
		return nil, invalidf("upload_id must be 1-%d characters of A-Z a-z 0-9 - _", maxUploadIDLen)
	}
	var doc packUpload
	switch err := s.d.Store.GetDoc(ctx, CollPackUploads, id, &doc); {
	case errors.Is(err, domain.ErrNotFound):
		return &syncpb.QueryUploadResponse{}, nil
	case err != nil:
		return nil, s.toStatus(ctx, err, "upload")
	}
	if doc.AccountID != p.AccountID {
		return &syncpb.QueryUploadResponse{}, nil
	}
	fi, err := os.Stat(spoolPath(o.SpoolDir, id))
	if err != nil {
		return &syncpb.QueryUploadResponse{}, nil
	}
	return &syncpb.QueryUploadResponse{Found: true, Offset: uint64(fi.Size())}, nil
}

// UploadPack receives a pack (format in specs/sync/Packs.md) as a stream
// of PackChunk messages of at most 4 MiB.
//
// The first chunk names the upload (upload_id, chosen by the client) and,
// optionally, the tree the objects are for (tree_id; the caller then needs
// tree:write on it). Every chunk carries its byte offset within the pack;
// offsets must be contiguous. The received bytes are spooled to
// <SpoolDir>/<upload_id>.pack and recorded in pack_uploads, so an
// interrupted upload can be resumed by the same account: QueryUpload
// reports the offset kept, and a new UploadPack stream starting at that
// offset (or any earlier one) continues it. Interrupted uploads are
// removed after 24 h (RunPackJanitor).
//
// When the stream ends the pack is verified (framing, delta depth,
// trailer) and its objects reconstructed, hash-verified and stored whole,
// like UploadObjects. Delta bases outside the pack must be objects of the
// named tree the caller may read; without tree_id only bases inside the
// pack are accepted. A stream that ends before the pack is complete keeps
// the spool and fails with FailedPrecondition (resume it); a corrupt pack
// is discarded (InvalidArgument). Size limits: MaxPackSize and the
// tenant's storage quota (ResourceExhausted).
func (s *Service) UploadPack(stream grpc.ClientStreamingServer[syncpb.PackChunk, syncpb.UploadPackResponse]) error {
	o := s.packOpts()
	if o.Disabled || o.SpoolDir == "" {
		return status.Error(codes.Unimplemented, "pack uploads are disabled on this server")
	}
	ctx := stream.Context()
	p, err := requireAuth(ctx)
	if err != nil {
		return err
	}
	chunk, err := stream.Recv()
	if errors.Is(err, io.EOF) {
		return invalidf("empty pack upload")
	}
	if err != nil {
		return err
	}
	id := chunk.GetUploadId()
	if !validUploadID(id) {
		return invalidf("upload_id must be 1-%d characters of A-Z a-z 0-9 - _", maxUploadIDLen)
	}
	unlock, ok := s.lockUpload(id)
	if !ok {
		return status.Errorf(codes.Aborted, "upload %s is already in progress", id)
	}
	defer unlock()

	var tree *domain.Tree
	if chunk.GetTreeId().GetValue() != "" {
		if tree, _, err = s.writableTree(ctx, chunk.GetTreeId()); err != nil {
			return err
		}
	}
	limit := o.MaxPackSize
	if tree != nil && o.Limits != nil {
		if remaining, ok, err := s.storageRemaining(ctx, o.Limits, tree); err != nil {
			return err
		} else if ok && remaining < limit {
			limit = remaining
		}
	}

	f, size, resumed, err := s.openSpool(ctx, o, p, id, tree, chunk.GetOffset())
	if err != nil {
		return err
	}
	closed := false
	closeFile := func() {
		if !closed {
			_ = f.Close()
			closed = true
		}
	}
	defer closeFile()
	if resumed {
		opsmetrics.PackUploadResumes.Inc("")
	}

	for {
		data := chunk.GetData()
		if len(data) > PackMaxChunk {
			return invalidf("pack chunk of %d bytes exceeds %d", len(data), PackMaxChunk)
		}
		if cid := chunk.GetUploadId(); cid != "" && cid != id {
			return invalidf("chunk names upload %q, stream is %q", cid, id)
		}
		if chunk.GetOffset() != uint64(size) {
			return invalidf("chunk offset %d, expected %d", chunk.GetOffset(), size)
		}
		if size+int64(len(data)) > limit {
			closeFile()
			s.dropUpload(ctx, o, id)
			if limit < o.MaxPackSize {
				return status.Errorf(codes.ResourceExhausted, "quota exceeded: %s", limits.LimitStorage)
			}
			return status.Errorf(codes.ResourceExhausted, "pack exceeds the server's %d-byte limit", o.MaxPackSize)
		}
		if _, err := f.Write(data); err != nil {
			return s.toStatus(ctx, err, "spool pack")
		}
		size += int64(len(data))
		chunk, err = stream.Recv()
		if errors.Is(err, io.EOF) {
			break
		}
		if err != nil {
			// The client went away: keep what arrived for a resume.
			_ = f.Sync()
			s.touchUpload(context.WithoutCancel(ctx), id)
			return err
		}
	}
	if err := f.Sync(); err != nil {
		return s.toStatus(ctx, err, "spool pack")
	}
	closeFile()

	resp, err := s.ingestPack(ctx, p, tree, spoolPath(o.SpoolDir, id))
	switch {
	case errors.Is(err, errPackTruncated):
		s.touchUpload(ctx, id)
		return status.Errorf(codes.FailedPrecondition, "pack upload incomplete: %d bytes received; resume at that offset (QueryUpload)", size)
	case errors.Is(err, errPackCorrupt), errors.Is(err, errPackChecksum):
		s.dropUpload(ctx, o, id)
		return invalidf("%v", err)
	case err != nil:
		return s.toStatus(ctx, err, "unpack")
	}
	s.dropUpload(ctx, o, id)
	resp.BytesReceived = uint64(size)
	return stream.SendAndClose(resp)
}

// storageRemaining returns the bytes the tree's tenant may still store
// (ok=false when unlimited).
func (s *Service) storageRemaining(ctx context.Context, l PackLimits, t *domain.Tree) (int64, bool, error) {
	tenant, err := s.d.Store.GetTenant(ctx, t.TenantID)
	if err != nil {
		return 0, false, s.toStatus(ctx, err, "tenant")
	}
	quota := l.Effective(tenant).MaxStorageBytes
	if quota <= 0 {
		return 0, false, nil
	}
	used, err := limits.StorageBytes(ctx, s.d.Store, tenant.ID)
	if err != nil {
		return 0, false, s.toStatus(ctx, err, "storage usage")
	}
	return max(0, quota-used), true, nil
}

// openSpool opens (creating or resuming) the spool file of upload id,
// positioned at offset. resumed reports a continued upload.
func (s *Service) openSpool(ctx context.Context, o PackOptions, p *domain.Principal, id string, tree *domain.Tree, offset uint64) (*os.File, int64, bool, error) {
	if err := os.MkdirAll(o.SpoolDir, 0o750); err != nil {
		return nil, 0, false, s.toStatus(ctx, err, "spool directory")
	}
	path := spoolPath(o.SpoolDir, id)
	now := s.now()
	var doc packUpload
	err := s.d.Store.GetDoc(ctx, CollPackUploads, id, &doc)
	switch {
	case errors.Is(err, domain.ErrNotFound):
		if offset != 0 {
			return nil, 0, false, status.Errorf(codes.FailedPrecondition, "unknown upload %s: start at offset 0", id)
		}
		s.limitUploads(ctx, o, p.AccountID)
		doc = packUpload{ID: id, AccountID: p.AccountID, CreatedAt: now, UpdatedAt: now}
		if tree != nil {
			doc.TreeID = tree.ID
		}
		if err := s.d.Store.CreateDoc(ctx, CollPackUploads, id, doc); err != nil {
			if errors.Is(err, domain.ErrAlreadyExists) {
				return nil, 0, false, status.Errorf(codes.Aborted, "upload %s is already in progress", id)
			}
			return nil, 0, false, s.toStatus(ctx, err, "upload")
		}
		f, err := os.OpenFile(path, os.O_CREATE|os.O_TRUNC|os.O_WRONLY, 0o640)
		if err != nil {
			return nil, 0, false, s.toStatus(ctx, err, "spool pack")
		}
		return f, 0, false, nil
	case err != nil:
		return nil, 0, false, s.toStatus(ctx, err, "upload")
	}
	if doc.AccountID != p.AccountID {
		return nil, 0, false, status.Errorf(codes.PermissionDenied, "upload %s belongs to another account", id)
	}
	if tree != nil && doc.TreeID != "" && doc.TreeID != tree.ID {
		return nil, 0, false, invalidf("upload %s was started for another tree", id)
	}
	f, err := os.OpenFile(path, os.O_CREATE|os.O_WRONLY, 0o640)
	if err != nil {
		return nil, 0, false, s.toStatus(ctx, err, "spool pack")
	}
	fi, err := f.Stat()
	if err != nil {
		_ = f.Close()
		return nil, 0, false, s.toStatus(ctx, err, "spool pack")
	}
	if offset > uint64(fi.Size()) {
		_ = f.Close()
		return nil, 0, false, status.Errorf(codes.FailedPrecondition, "offset %d is beyond the %d bytes received for upload %s", offset, fi.Size(), id)
	}
	if err := f.Truncate(int64(offset)); err != nil {
		_ = f.Close()
		return nil, 0, false, s.toStatus(ctx, err, "spool pack")
	}
	if _, err := f.Seek(int64(offset), io.SeekStart); err != nil {
		_ = f.Close()
		return nil, 0, false, s.toStatus(ctx, err, "spool pack")
	}
	s.touchUpload(ctx, id)
	return f, int64(offset), offset > 0, nil
}

// maxUploadsPerAccount bounds the interrupted uploads one account keeps
// (and so the spool space it uses: at most this many × MaxPackSize).
const maxUploadsPerAccount = 8

// limitUploads drops the least recently touched interrupted uploads of
// account until a new one fits under maxUploadsPerAccount.
func (s *Service) limitUploads(ctx context.Context, o PackOptions, account string) {
	raws, err := s.d.Store.ListDocs(ctx, CollPackUploads, "")
	if err != nil {
		return
	}
	var mine []packUpload
	for _, raw := range raws {
		var doc packUpload
		if json.Unmarshal(raw, &doc) == nil && doc.AccountID == account && !s.uploadActive(doc.ID) {
			mine = append(mine, doc)
		}
	}
	if len(mine) < maxUploadsPerAccount {
		return
	}
	sort.SliceStable(mine, func(i, j int) bool { return mine[i].UpdatedAt.Before(mine[j].UpdatedAt) })
	for _, doc := range mine[:len(mine)-maxUploadsPerAccount+1] {
		s.dropUpload(ctx, o, doc.ID)
	}
}

// touchUpload refreshes the upload's updated_at (best effort).
func (s *Service) touchUpload(ctx context.Context, id string) {
	err := s.d.Store.UpdateDoc(ctx, CollPackUploads, id, func(raw []byte) ([]byte, error) {
		if raw == nil {
			return nil, errors.New("gone")
		}
		var doc packUpload
		if err := json.Unmarshal(raw, &doc); err != nil {
			return nil, err
		}
		doc.UpdatedAt = s.now()
		return json.Marshal(doc)
	})
	if err != nil {
		s.log.DebugContext(ctx, "pack upload: touch failed", "upload", id, "err", err)
	}
}

// dropUpload removes the spool file and document of an upload.
func (s *Service) dropUpload(ctx context.Context, o PackOptions, id string) {
	if err := os.Remove(spoolPath(o.SpoolDir, id)); err != nil && !errors.Is(err, os.ErrNotExist) {
		s.log.WarnContext(ctx, "pack upload: remove spool", "upload", id, "err", err)
	}
	if err := s.d.Store.DeleteDoc(context.WithoutCancel(ctx), CollPackUploads, id); err != nil && !errors.Is(err, domain.ErrNotFound) {
		s.log.WarnContext(ctx, "pack upload: delete document", "upload", id, "err", err)
	}
}

// ingestPack verifies the spooled pack and stores its objects. A first
// pass checks the framing and trailer and collects the delta bases that
// lie outside the pack; those must be objects of tree the caller may read
// (reachable). The second pass reconstructs, verifies and stores.
func (s *Service) ingestPack(ctx context.Context, p *domain.Principal, tree *domain.Tree, path string) (*syncpb.UploadPackResponse, error) {
	external, err := scanPackBases(path)
	if err != nil {
		return nil, err
	}
	allowed := make(map[hash.Hash]struct{})
	if tree != nil && len(external) > 0 {
		if len(external) > maxExternalBasesChecked {
			return nil, packCorrupt("too many delta bases")
		}
		reach, err := s.reachable(ctx, tree, p, external)
		if err != nil {
			return nil, s.toStatus(ctx, err, "resolve delta bases")
		}
		for h := range reach.kind {
			allowed[h] = struct{}{}
		}
	}

	f, err := os.Open(path)
	if err != nil {
		return nil, err
	}
	defer f.Close()
	pr, err := newPackReader(f)
	if err != nil {
		return nil, err
	}
	// Objects are reconstructed in pack order and stored by a few workers
	// in parallel (storing is dominated by file-system latency). Objects
	// queued but not stored yet serve as delta bases from memory.
	var (
		mu          sync.Mutex
		resp        = &syncpb.UploadPackResponse{}
		inPack      = make(map[hash.Hash]struct{})
		pending     = make(map[hash.Hash][]byte)
		objectBytes int64
		deltas      int
		storeErr    error
		wg          sync.WaitGroup
	)
	jobs := make(chan *packObject, 2*packStoreWorkers)
	for i := 0; i < packStoreWorkers; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			for obj := range jobs {
				stored, ok, err := s.storeObject(ctx, &syncpb.ObjectData{Hash: hashToPB(obj.Hash), Kind: syncpb.ObjectKind(obj.Kind), Data: obj.Data})
				mu.Lock()
				delete(pending, obj.Hash)
				switch {
				case err != nil:
					if storeErr == nil {
						storeErr = err
					}
				case !ok:
					delete(inPack, obj.Hash)
					resp.Rejected = append(resp.Rejected, hashToPB(obj.Hash))
				default:
					objectBytes += int64(len(obj.Data))
					if obj.Base != nil {
						deltas++
					}
					if stored {
						resp.ObjectsStored++
					} else {
						resp.ObjectsAlreadyPresent++
					}
				}
				mu.Unlock()
			}
		}()
	}
	lookup := func(h hash.Hash) ([]byte, bool) {
		mu.Lock()
		data, queued := pending[h]
		_, mine := inPack[h]
		mu.Unlock()
		if queued {
			return data, true
		}
		if _, ok := allowed[h]; !mine && !ok {
			return nil, false
		}
		data, err := s.d.Objects.Get(ctx, h)
		if err != nil {
			return nil, false
		}
		return data, true
	}
	var readErr error
	for {
		obj, err := pr.next(lookup)
		if err != nil {
			readErr = err
			break
		}
		if obj == nil {
			break
		}
		mu.Lock()
		failed := storeErr != nil
		if obj.Err != nil {
			s.log.DebugContext(ctx, "pack upload: rejected entry", "hash", obj.Hash.Hex(), "err", obj.Err)
			resp.Rejected = append(resp.Rejected, hashToPB(obj.Hash))
		} else if !failed {
			inPack[obj.Hash] = struct{}{}
			pending[obj.Hash] = obj.Data
		}
		mu.Unlock()
		if failed {
			break
		}
		if obj.Err == nil {
			jobs <- obj
		}
	}
	close(jobs)
	wg.Wait()
	if readErr != nil {
		return nil, readErr
	}
	if storeErr != nil {
		return nil, storeErr
	}
	countPack("upload", pr.consumed, objectBytes, deltas)
	return resp, nil
}

// packStoreWorkers is how many objects of an upload are stored at once.
const packStoreWorkers = 8

// scanPackBases reads the whole pack once (framing, depth and trailer
// checks) and returns the delta bases that are not entries of the pack.
func scanPackBases(path string) ([]hash.Hash, error) {
	f, err := os.Open(path)
	if err != nil {
		return nil, err
	}
	defer f.Close()
	pr, err := newPackReader(f)
	if err != nil {
		return nil, err
	}
	seen := make(map[hash.Hash]struct{})
	ext := make(map[hash.Hash]struct{})
	var out []hash.Hash
	none := func(hash.Hash) ([]byte, bool) { return nil, false }
	for {
		obj, err := pr.next(none)
		if err != nil {
			return nil, err
		}
		if obj == nil {
			return out, nil
		}
		if obj.Base != nil {
			if _, in := seen[*obj.Base]; !in {
				if _, dup := ext[*obj.Base]; !dup {
					ext[*obj.Base] = struct{}{}
					out = append(out, *obj.Base)
				}
			}
		}
		seen[obj.Hash] = struct{}{}
	}
}

// countPack feeds the pack transfer counters.
func countPack(direction string, packBytes, objectBytes int64, deltas int) {
	opsmetrics.PackBytes.Add(direction, uint64(max(packBytes, 0)))
	opsmetrics.PackObjectBytes.Add(direction, uint64(max(objectBytes, 0)))
	if saved := objectBytes - packBytes; saved > 0 {
		opsmetrics.PackBytesSaved.Add(direction, uint64(saved))
	}
	opsmetrics.PackDeltas.Add(direction, uint64(deltas))
}

// RunPackJanitor removes interrupted uploads older than the retention
// (24 h) every JanitorInterval until ctx is done.
func (s *Service) RunPackJanitor(ctx context.Context) {
	o := s.packOpts()
	if o.SpoolDir == "" {
		return
	}
	interval := o.JanitorInterval
	if interval <= 0 {
		interval = defaultJanitorInterval
	}
	t := time.NewTicker(interval)
	defer t.Stop()
	for {
		if n, err := s.CleanPackUploads(ctx); err != nil {
			s.log.WarnContext(ctx, "pack janitor failed", "err", err)
		} else if n > 0 {
			s.log.InfoContext(ctx, "pack janitor removed expired uploads", "count", n)
		}
		select {
		case <-ctx.Done():
			return
		case <-t.C:
		}
	}
}

// CleanPackUploads removes uploads not touched within the retention, and
// spool files without a document that are older than it. It returns how
// many uploads were removed.
func (s *Service) CleanPackUploads(ctx context.Context) (int, error) {
	o := s.packOpts()
	if o.SpoolDir == "" {
		return 0, nil
	}
	cutoff := s.now().Add(-o.Retention)
	raws, err := s.d.Store.ListDocs(ctx, CollPackUploads, "")
	if err != nil {
		return 0, err
	}
	known := make(map[string]struct{}, len(raws))
	removed := 0
	for _, raw := range raws {
		var doc packUpload
		if err := json.Unmarshal(raw, &doc); err != nil || doc.ID == "" {
			continue
		}
		known[doc.ID] = struct{}{}
		if doc.UpdatedAt.After(cutoff) || s.uploadActive(doc.ID) {
			continue
		}
		s.dropUpload(ctx, o, doc.ID)
		removed++
	}
	entries, err := os.ReadDir(o.SpoolDir)
	if err != nil {
		if errors.Is(err, os.ErrNotExist) {
			return removed, nil
		}
		return removed, err
	}
	for _, e := range entries {
		name := e.Name()
		id, ok := strings.CutSuffix(name, packSpoolSuffix)
		if !ok || e.IsDir() {
			continue
		}
		if _, has := known[id]; has || s.uploadActive(id) {
			continue
		}
		if fi, err := e.Info(); err == nil && fi.ModTime().Before(cutoff) {
			if err := os.Remove(filepath.Join(o.SpoolDir, name)); err == nil {
				removed++
			}
		}
	}
	return removed, nil
}

// packObjectKind maps a stored object's reachability kind to the pack's
// kind byte.
func packObjectKind(k syncpb.ObjectKind) uint8 {
	if k < 0 || k > 3 {
		return 0
	}
	return uint8(k)
}
