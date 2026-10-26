package syncsvc

// pack-delta (docs/ENGINEERING_PLAN.md §16.1): UploadPack, QueryUpload,
// DownloadPack, the spool janitor and the pack counters.

import (
	"bytes"
	"context"
	"errors"
	"io"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"google.golang.org/grpc/codes"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
	"github.com/seanfilimon/worktree/services/server-go/internal/opsmetrics"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
)

// docFakeStore adds a real DocStore (pack_uploads, tenant usage) to the
// fake metadata store.
type docFakeStore struct {
	*fakeStore
	docs *meta.Memory
}

func (s docFakeStore) PutDoc(ctx context.Context, c, k string, v any) error {
	return s.docs.PutDoc(ctx, c, k, v)
}
func (s docFakeStore) CreateDoc(ctx context.Context, c, k string, v any) error {
	return s.docs.CreateDoc(ctx, c, k, v)
}
func (s docFakeStore) GetDoc(ctx context.Context, c, k string, v any) error {
	return s.docs.GetDoc(ctx, c, k, v)
}
func (s docFakeStore) ListDocs(ctx context.Context, c, p string) ([][]byte, error) {
	return s.docs.ListDocs(ctx, c, p)
}
func (s docFakeStore) DeleteDoc(ctx context.Context, c, k string) error {
	return s.docs.DeleteDoc(ctx, c, k)
}
func (s docFakeStore) DeleteDocs(ctx context.Context, c, p string) (int, error) {
	return s.docs.DeleteDocs(ctx, c, p)
}
func (s docFakeStore) UpdateDoc(ctx context.Context, c, k string, fn func([]byte) ([]byte, error)) error {
	return s.docs.UpdateDoc(ctx, c, k, fn)
}
func (s docFakeStore) NextSeq(ctx context.Context, name string) (int64, error) {
	return s.docs.NextSeq(ctx, name)
}

type fixedLimits struct{ max int64 }

func (l fixedLimits) Effective(*domain.Tenant) domain.TenantLimits {
	return domain.TenantLimits{MaxStorageBytes: l.max}
}

// packHarness is a harness whose service has DocStore support and packs
// configured with a temporary spool directory.
func packHarness(t *testing.T, mutate ...func(*PackOptions)) (*harness, PackOptions) {
	t.Helper()
	h := newHarness(t)
	h.svc.d.Store = docFakeStore{fakeStore: h.store, docs: meta.NewMemory()}
	o := PackOptions{SpoolDir: filepath.Join(t.TempDir(), "uploads")}
	for _, m := range mutate {
		m(&o)
	}
	h.svc.ConfigurePacks(o)
	return h, h.svc.packOpts()
}

type packItem struct {
	data []byte
	kind syncpb.ObjectKind
	base []byte
}

func objectsOf(c commit) []packItem {
	var out []packItem
	for _, o := range c.objects {
		out = append(out, packItem{data: o.GetData(), kind: o.GetKind()})
	}
	return out
}

func makePack(t *testing.T, items []packItem) []byte {
	t.Helper()
	var buf bytes.Buffer
	pw := newPackWriter(&buf, true)
	for _, it := range items {
		var bh *hash.Hash
		if it.base != nil {
			b := hash.Sum(it.base)
			bh = &b
		}
		if _, err := pw.add(hash.Sum(it.data), uint8(it.kind), it.data, bh, it.base); err != nil {
			t.Fatal(err)
		}
	}
	if _, err := pw.finish(); err != nil {
		t.Fatal(err)
	}
	return buf.Bytes()
}

// sendPack streams pack[from:] in chunks of size chunk as upload id.
func (h *harness) sendPack(ctx context.Context, id, treeID string, pack []byte, from, chunk int) (*syncpb.UploadPackResponse, error) {
	stream, err := h.client.UploadPack(ctx)
	if err != nil {
		return nil, err
	}
	for off := from; off < len(pack); off += chunk {
		end := min(off+chunk, len(pack))
		msg := &syncpb.PackChunk{Data: pack[off:end], UploadId: id, Offset: uint64(off)}
		if off == from && treeID != "" {
			msg.TreeId = &syncpb.TreeId{Value: treeID}
		}
		if err := stream.Send(msg); err != nil {
			if errors.Is(err, io.EOF) {
				break
			}
			return nil, err
		}
	}
	return stream.CloseAndRecv()
}

func bigText(prefix string, lines int) string {
	var b strings.Builder
	for i := 0; i < lines; i++ {
		b.WriteString(prefix)
		b.WriteString(" line ")
		b.WriteString(strings.Repeat("x", i%37))
		b.WriteString("\n")
	}
	return b.String()
}

func TestPackRPCsDisabledOrUnconfigured(t *testing.T) {
	h := newHarness(t)
	ctx := h.as("alice")
	_, err := h.sendPack(ctx, "u1", "", []byte("x"), 0, 1)
	wantCode(t, err, codes.Unimplemented)
	_, err = h.client.QueryUpload(ctx, &syncpb.QueryUploadRequest{UploadId: "u1"})
	wantCode(t, err, codes.Unimplemented)

	h2, _ := packHarness(t, func(o *PackOptions) { o.Disabled = true })
	tree := h2.newTree("t")
	_, err = h2.sendPack(h2.as("alice"), "u1", tree, []byte("x"), 0, 1)
	wantCode(t, err, codes.Unimplemented)
	s, err := h2.client.DownloadPack(h2.as("alice"), &syncpb.DownloadPackRequest{TreeId: &syncpb.TreeId{Value: tree}})
	if err == nil {
		_, err = s.Recv()
	}
	wantCode(t, err, codes.Unimplemented)
}

func TestUploadPackStoresWholeAndDeltaObjects(t *testing.T) {
	h, o := packHarness(t)
	ctx := h.as("alice")
	tree := h.newTree("packs")
	text := bigText("v1", 400)
	c1 := makeCommit(tree, map[string]string{"a.txt": text, "b.txt": "small"})
	h.upload(ctx, c1)
	h.mustPush(ctx, pushReq(tree, "main", "", c1))

	edited := strings.Replace(text, "v1 line xxxxx\n", "v2 edited line\n", 3)
	c2 := makeCommit(tree, map[string]string{"a.txt": edited, "b.txt": "small"}, c1)
	before := opsmetrics.PackDeltas.Get("upload")
	var items []packItem
	for _, obj := range c2.objects {
		it := packItem{data: obj.GetData(), kind: obj.GetKind()}
		switch {
		case string(obj.GetData()) == edited:
			it.base = []byte(text)
		case obj.GetKind() == syncpb.ObjectKind_OBJECT_KIND_MANIFEST:
			it.base = c1.objects[len(c1.objects)-1].GetData()
		}
		items = append(items, it)
	}
	pack := makePack(t, items)
	resp, err := h.sendPack(ctx, "up-1", tree, pack, 0, 1000)
	if err != nil {
		t.Fatal(err)
	}
	if resp.GetObjectsStored() != 2 || resp.GetObjectsAlreadyPresent() != 1 || len(resp.GetRejected()) != 0 || resp.GetBytesReceived() != uint64(len(pack)) {
		t.Fatalf("response %+v", resp)
	}
	if got := opsmetrics.PackDeltas.Get("upload") - before; got < 1 {
		t.Fatalf("delta counter grew by %d", got)
	}
	if opsmetrics.PackBytesSaved.Get("upload") == 0 {
		t.Fatal("bytes saved not counted")
	}
	h.mustPush(ctx, pushReq(tree, "main", c1.id, c2))
	// The spool and the document are gone.
	if _, err := os.Stat(spoolPath(o.SpoolDir, "up-1")); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("spool kept: %v", err)
	}
	q, err := h.client.QueryUpload(ctx, &syncpb.QueryUploadRequest{UploadId: "up-1"})
	if err != nil || q.GetFound() {
		t.Fatalf("query after success: %+v %v", q, err)
	}
}

func TestUploadPackRestrictsExternalBases(t *testing.T) {
	h, _ := packHarness(t)
	ctx := h.as("alice")
	tree := h.newTree("bases")
	secret := []byte(bigText("secret", 300))
	// Stored, but not reachable from any tree.
	if _, err := h.uploadObjects(ctx, &syncpb.ObjectData{Hash: hashToPB(hash.Sum(secret)), Kind: syncpb.ObjectKind_OBJECT_KIND_BLOB, Data: secret}); err != nil {
		t.Fatal(err)
	}
	target := append(append([]byte(nil), secret...), "probe"...)
	pack := makePack(t, []packItem{{data: target, kind: syncpb.ObjectKind_OBJECT_KIND_BLOB, base: secret}})
	for _, treeID := range []string{tree, ""} {
		resp, err := h.sendPack(ctx, "probe-"+map[bool]string{true: "t", false: "n"}[treeID != ""], treeID, pack, 0, 1<<20)
		if err != nil {
			t.Fatal(err)
		}
		if len(resp.GetRejected()) != 1 || resp.GetObjectsStored() != 0 {
			t.Fatalf("tree %q: unreachable base accepted: %+v", treeID, resp)
		}
	}
	// Bases inside the pack are fine without a tree.
	inPack := makePack(t, []packItem{{data: secret, kind: 1}, {data: target, kind: 1, base: secret}})
	resp, err := h.sendPack(ctx, "inpack", "", inPack, 0, 1<<20)
	if err != nil || len(resp.GetRejected()) != 0 || resp.GetObjectsStored() != 1 || resp.GetObjectsAlreadyPresent() != 1 {
		t.Fatalf("in-pack base: %+v %v", resp, err)
	}
	// A viewer may not upload for the tree.
	_, err = h.sendPack(h.as("bob"), "viewer", tree, inPack, 0, 1<<20)
	wantCode(t, err, codes.PermissionDenied)
}

func TestUploadPackResume(t *testing.T) {
	h, o := packHarness(t)
	ctx := h.as("alice")
	tree := h.newTree("resume")
	c1 := makeCommit(tree, map[string]string{"a.txt": bigText("a", 2000), "b.txt": bigText("b", 1500)})
	pack := makePack(t, objectsOf(c1))
	half := len(pack) / 2

	// A stream that ends early (clean EOF) keeps the prefix.
	_, err := h.sendPack(ctx, "res-1", tree, pack[:half], 0, 500)
	wantCode(t, err, codes.FailedPrecondition)
	q, err := h.client.QueryUpload(ctx, &syncpb.QueryUploadRequest{UploadId: "res-1"})
	if err != nil || !q.GetFound() || q.GetOffset() != uint64(half) {
		t.Fatalf("query: %+v %v", q, err)
	}
	// Other accounts do not see it and may not continue it.
	q, err = h.client.QueryUpload(h.as("bob"), &syncpb.QueryUploadRequest{UploadId: "res-1"})
	if err != nil || q.GetFound() {
		t.Fatalf("bob query: %+v %v", q, err)
	}
	_, err = h.sendPack(h.as("bob"), "res-1", "", pack, half, 1<<20)
	wantCode(t, err, codes.PermissionDenied)
	// Gaps and offsets past the received prefix are refused.
	_, err = h.sendPack(ctx, "res-1", tree, pack, half+10, 1<<20)
	wantCode(t, err, codes.FailedPrecondition)
	stream, err := h.client.UploadPack(ctx)
	if err != nil {
		t.Fatal(err)
	}
	_ = stream.Send(&syncpb.PackChunk{Data: pack[half : half+10], UploadId: "res-1", Offset: uint64(half), TreeId: &syncpb.TreeId{Value: tree}})
	_ = stream.Send(&syncpb.PackChunk{Data: pack[half+20:], UploadId: "res-1", Offset: uint64(half + 20)})
	_, err = stream.CloseAndRecv()
	wantCode(t, err, codes.InvalidArgument)
	// That failed stream kept half+10 bytes; resuming earlier rewinds.
	before := opsmetrics.PackUploadResumes.Get("")
	resp, err := h.sendPack(ctx, "res-1", tree, pack, half, 700)
	if err != nil {
		t.Fatal(err)
	}
	if resp.GetObjectsStored() != 3 || resp.GetBytesReceived() != uint64(len(pack)) {
		t.Fatalf("resumed: %+v", resp)
	}
	if opsmetrics.PackUploadResumes.Get("") != before+1 {
		t.Fatal("resume not counted")
	}
	h.mustPush(ctx, pushReq(tree, "main", "", c1))

	// A cancelled stream (client gone) keeps what arrived.
	c2 := makeCommit(tree, map[string]string{"c.txt": bigText("c", 3000)}, c1)
	pack2 := makePack(t, objectsOf(c2))
	cctx, cancel := context.WithCancel(ctx)
	s2, err := h.client.UploadPack(cctx)
	if err != nil {
		t.Fatal(err)
	}
	if err := s2.Send(&syncpb.PackChunk{Data: pack2[:100], UploadId: "res-2", TreeId: &syncpb.TreeId{Value: tree}}); err != nil {
		t.Fatal(err)
	}
	deadline := time.Now().Add(5 * time.Second)
	for {
		q, _ := h.client.QueryUpload(ctx, &syncpb.QueryUploadRequest{UploadId: "res-2"})
		if q.GetOffset() == 100 {
			break
		}
		if time.Now().After(deadline) {
			t.Fatalf("server never spooled the first chunk: %+v", q)
		}
		time.Sleep(10 * time.Millisecond)
	}
	cancel()
	// The upload stays locked until the handler notices; retry briefly.
	for {
		resp, err = h.sendPack(ctx, "res-2", "", pack2, 100, 1<<20)
		if err == nil || !strings.Contains(err.Error(), "in progress") || time.Now().After(deadline) {
			break
		}
		time.Sleep(10 * time.Millisecond)
	}
	if err != nil || resp.GetObjectsStored() != 2 {
		t.Fatalf("resume after cancel: %+v %v", resp, err)
	}
	if _, err := os.Stat(spoolPath(o.SpoolDir, "res-2")); !errors.Is(err, os.ErrNotExist) {
		t.Fatal("spool kept after success")
	}
}

func TestUploadPackRejectsBadInput(t *testing.T) {
	h, o := packHarness(t, func(o *PackOptions) { o.MaxPackSize = 2 << 20 })
	ctx := h.as("alice")
	tree := h.newTree("bad")
	pack := makePack(t, []packItem{{data: []byte("hello"), kind: 1}})

	_, err := h.sendPack(ctx, "bad id!", "", pack, 0, 1<<20)
	wantCode(t, err, codes.InvalidArgument)
	_, err = h.sendPack(ctx, "new", "", pack, 5, 1<<20)
	wantCode(t, err, codes.FailedPrecondition)
	_, err = h.sendPack(h.as(""), "anon", "", pack, 0, 1<<20)
	wantCode(t, err, codes.Unauthenticated)

	// Corrupt: the spool is dropped.
	corrupt := append([]byte(nil), pack...)
	corrupt[len(corrupt)-1] ^= 1
	_, err = h.sendPack(ctx, "corrupt", tree, corrupt, 0, 1<<20)
	wantCode(t, err, codes.InvalidArgument)
	if _, err := os.Stat(spoolPath(o.SpoolDir, "corrupt")); !errors.Is(err, os.ErrNotExist) {
		t.Fatal("corrupt spool kept")
	}
	// Oversized chunk and pack.
	_, err = h.sendPack(ctx, "chunk", "", make([]byte, PackMaxChunk+1), 0, PackMaxChunk+1)
	wantCode(t, err, codes.InvalidArgument)
	big := make([]byte, 3<<20)
	_, err = h.sendPack(ctx, "big", "", big, 0, 1<<20)
	wantCode(t, err, codes.ResourceExhausted)

	// Storage quota of the tree's tenant.
	hq, _ := packHarness(t, func(o *PackOptions) { o.Limits = fixedLimits{max: 1000} })
	qtree := hq.newTree("quota")
	_, err = hq.sendPack(hq.as("alice"), "q", qtree, make([]byte, 2000), 0, 500)
	wantCode(t, err, codes.ResourceExhausted)
	if !strings.Contains(err.Error(), "max_storage_bytes") {
		t.Fatalf("quota error: %v", err)
	}
}

func TestCleanPackUploads(t *testing.T) {
	h, o := packHarness(t)
	ctx := context.Background()
	now := time.Now().UTC()
	h.svc.now = func() time.Time { return now }
	if err := os.MkdirAll(o.SpoolDir, 0o750); err != nil {
		t.Fatal(err)
	}
	put := func(id string, age time.Duration) {
		doc := packUpload{ID: id, AccountID: alice.AccountID, CreatedAt: now.Add(-age), UpdatedAt: now.Add(-age)}
		if err := h.svc.d.Store.PutDoc(ctx, CollPackUploads, id, doc); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(spoolPath(o.SpoolDir, id), []byte("x"), 0o640); err != nil {
			t.Fatal(err)
		}
	}
	put("old", 25*time.Hour)
	put("fresh", time.Hour)
	orphan := spoolPath(o.SpoolDir, "orphan")
	if err := os.WriteFile(orphan, []byte("x"), 0o640); err != nil {
		t.Fatal(err)
	}
	old := now.Add(-48 * time.Hour)
	if err := os.Chtimes(orphan, old, old); err != nil {
		t.Fatal(err)
	}
	n, err := h.svc.CleanPackUploads(ctx)
	if err != nil || n != 2 {
		t.Fatalf("cleaned %d: %v", n, err)
	}
	for id, want := range map[string]bool{"old": false, "fresh": true, "orphan": false} {
		_, err := os.Stat(spoolPath(o.SpoolDir, id))
		if (err == nil) != want {
			t.Errorf("%s: exists=%v", id, err == nil)
		}
	}
	var doc packUpload
	if err := h.svc.d.Store.GetDoc(ctx, CollPackUploads, "old", &doc); !errors.Is(err, domain.ErrNotFound) {
		t.Fatalf("old document kept: %v", err)
	}
}

func TestInterruptedUploadsPerAccountAreBounded(t *testing.T) {
	h, o := packHarness(t)
	ctx := h.as("alice")
	pack := makePack(t, []packItem{{data: []byte(bigText("x", 100)), kind: 1}})
	ids := []string{"i0", "i1", "i2", "i3", "i4", "i5", "i6", "i7", "i8", "i9"}
	for _, id := range ids {
		_, err := h.sendPack(ctx, id, "", pack[:20], 0, 20)
		wantCode(t, err, codes.FailedPrecondition)
	}
	// bob's uploads do not count against alice.
	_, err := h.sendPack(h.as("bob"), "b0", "", pack[:20], 0, 20)
	wantCode(t, err, codes.FailedPrecondition)
	kept := 0
	for _, id := range ids {
		q, err := h.client.QueryUpload(ctx, &syncpb.QueryUploadRequest{UploadId: id})
		if err != nil {
			t.Fatal(err)
		}
		if q.GetFound() {
			kept++
		}
	}
	if kept != maxUploadsPerAccount {
		t.Fatalf("kept %d interrupted uploads, want %d", kept, maxUploadsPerAccount)
	}
	// The oldest were dropped (spool files too).
	if _, err := os.Stat(spoolPath(o.SpoolDir, "i0")); !errors.Is(err, os.ErrNotExist) {
		t.Fatal("oldest upload kept")
	}
	if q, _ := h.client.QueryUpload(h.as("bob"), &syncpb.QueryUploadRequest{UploadId: "b0"}); !q.GetFound() {
		t.Fatal("bob's upload dropped")
	}
}

// readDownload collects a DownloadPack stream and decodes it with haves.
func readDownload(t *testing.T, h *harness, user string, req *syncpb.DownloadPackRequest, haves map[hash.Hash][]byte) ([]*packObject, error) {
	t.Helper()
	s, err := h.client.DownloadPack(h.as(user), req)
	if err != nil {
		return nil, err
	}
	var buf bytes.Buffer
	var next uint64
	for {
		c, err := s.Recv()
		if errors.Is(err, io.EOF) {
			break
		}
		if err != nil {
			return nil, err
		}
		if c.GetOffset() != next || len(c.GetData()) > PackMaxChunk {
			t.Fatalf("chunk at %d (expected %d), %d bytes", c.GetOffset(), next, len(c.GetData()))
		}
		next += uint64(len(c.GetData()))
		buf.Write(c.GetData())
	}
	pr, err := newPackReader(&buf)
	if err != nil {
		return nil, err
	}
	store := map[hash.Hash][]byte{}
	for k, v := range haves {
		store[k] = v
	}
	var out []*packObject
	for {
		o, err := pr.next(func(h hash.Hash) ([]byte, bool) { d, ok := store[h]; return d, ok })
		if err != nil {
			return nil, err
		}
		if o == nil {
			return out, nil
		}
		if o.Err != nil {
			t.Fatalf("entry %s: %v", o.Hash.Hex(), o.Err)
		}
		store[o.Hash] = o.Data
		out = append(out, o)
	}
}

func TestDownloadPackDeltasAgainstHaves(t *testing.T) {
	h, _ := packHarness(t)
	ctx := h.as("alice")
	tree := h.newTree("down")
	text := bigText("v1", 500)
	big := makeChunkedFile(bigText("chunk-a", 200), bigText("chunk-b", 200))
	c1 := makeMixedCommit(tree, map[string]string{"a.txt": text, "same.txt": "same"}, map[string]chunkedFile{"big.bin": big})
	var objs []*syncpb.ObjectData
	objs = append(objs, big.objects...)
	objs = append(objs, c1.objects...)
	if _, err := h.uploadObjects(ctx, objs...); err != nil {
		t.Fatal(err)
	}
	h.mustPush(ctx, pushReq(tree, "main", "", c1))
	big2 := makeChunkedFile(bigText("chunk-a", 200), strings.Replace(bigText("chunk-b", 200), "chunk-b line xx\n", "chunk-b EDITED\n", 1))
	c2 := makeMixedCommit(tree, map[string]string{"a.txt": strings.Replace(text, "v1 line x\n", "v2 changed\n", 2), "same.txt": "same"}, map[string]chunkedFile{"big.bin": big2}, c1)
	objs = append(append([]*syncpb.ObjectData(nil), big2.objects...), c2.objects...)
	if _, err := h.uploadObjects(ctx, objs...); err != nil {
		t.Fatal(err)
	}
	h.mustPush(ctx, pushReq(tree, "main", c1.id, c2))

	// The client has c1 (manifest first) and wants c2's manifest, the
	// edited blob and the new chunk list (whose new chunk is expanded).
	haves := map[hash.Hash][]byte{}
	haveList := []*syncpb.ContentHash{hashToPB(c1.manifest)}
	for _, o := range append(append([]*syncpb.ObjectData(nil), c1.objects...), big.objects...) {
		haves[hash.Hash(o.GetHash().GetValue())] = o.GetData()
		if hash.Hash(o.GetHash().GetValue()) != c1.manifest {
			haveList = append(haveList, o.GetHash())
		}
	}
	var want []*syncpb.ContentHash
	for _, o := range c2.objects {
		want = append(want, o.GetHash())
	}
	want = append(want, hashToPB(big2.list))
	req := &syncpb.DownloadPackRequest{TreeId: &syncpb.TreeId{Value: tree}, Want: want, Have: haveList, AllowDeltas: true}
	before := opsmetrics.PackDeltas.Get("download")
	got, err := readDownload(t, h, "bob", req, haves)
	if err != nil {
		t.Fatal(err)
	}
	// c2: manifest + edited a.txt (same.txt is had), big2: list + new chunk.
	if len(got) != 4 {
		t.Fatalf("got %d objects", len(got))
	}
	deltas := 0
	for _, o := range got {
		if o.Base != nil {
			deltas++
		}
	}
	if deltas < 3 || opsmetrics.PackDeltas.Get("download")-before != uint64(deltas) {
		t.Fatalf("deltas = %d", deltas)
	}

	// Without allow_deltas every entry is whole.
	req.AllowDeltas = false
	got, err = readDownload(t, h, "bob", req, haves)
	if err != nil || len(got) != 4 {
		t.Fatalf("whole download: %d %v", len(got), err)
	}
	for _, o := range got {
		if o.Base != nil {
			t.Fatal("delta without allow_deltas")
		}
	}

	// Unreadable tree.
	_, err = readDownload(t, h, "carol", req, haves)
	wantCode(t, err, codes.PermissionDenied)

	// A have that is not reachable (a manifest and blob uploaded but never
	// pushed) is never used as a base.
	stray := makeCommit(tree, map[string]string{"a.txt": text + "stray"})
	h.upload(ctx, stray)
	strayBlob := stray.objects[0]
	req = &syncpb.DownloadPackRequest{TreeId: &syncpb.TreeId{Value: tree},
		Want:        []*syncpb.ContentHash{hashToPB(hash.Sum([]byte(text))), hashToPB(c1.manifest)},
		Have:        []*syncpb.ContentHash{hashToPB(stray.manifest), strayBlob.GetHash()},
		AllowDeltas: true}
	got, err = readDownload(t, h, "alice", req, map[hash.Hash][]byte{hash.Hash(strayBlob.GetHash().GetValue()): strayBlob.GetData(),
		stray.manifest: stray.objects[1].GetData()})
	if err != nil || len(got) != 2 {
		t.Fatalf("stray base: %v %+v", err, got)
	}
	for _, o := range got {
		if o.Base != nil && *o.Base != c1.manifest && hash.Sum([]byte(text)) != *o.Base {
			t.Fatalf("unreachable base %s used", o.Base.Hex())
		}
	}
}
