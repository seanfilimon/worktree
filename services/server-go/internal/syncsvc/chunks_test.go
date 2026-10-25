package syncsvc

// v0.5 chunked large files and download scoping (docs/ENGINEERING_PLAN.md
// §13.2).

import (
	"sort"
	"testing"
	"time"

	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/metadata"
	"google.golang.org/protobuf/proto"
	"google.golang.org/protobuf/types/known/timestamppb"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
)

// chunkedFile is one large file split into chunks.
type chunkedFile struct {
	list   hash.Hash
	chunks []hash.Hash
	// objects: chunks (BLOB) then the list (CHUNK_LIST).
	objects []*syncpb.ObjectData
}

func makeChunkedFile(parts ...string) chunkedFile {
	var f chunkedFile
	cl := &syncpb.ChunkList{}
	seen := map[hash.Hash]bool{}
	for _, p := range parts {
		h := hash.Sum([]byte(p))
		f.chunks = append(f.chunks, h)
		cl.Chunks = append(cl.Chunks, &syncpb.Chunk{Hash: hashToPB(h), Size: uint64(len(p))})
		cl.Size += uint64(len(p))
		if !seen[h] {
			seen[h] = true
			f.objects = append(f.objects, &syncpb.ObjectData{Hash: hashToPB(h), Kind: syncpb.ObjectKind_OBJECT_KIND_BLOB, Data: []byte(p)})
		}
	}
	data, err := proto.MarshalOptions{Deterministic: true}.Marshal(cl)
	if err != nil {
		panic(err)
	}
	f.list = hash.Sum(data)
	f.objects = append(f.objects, &syncpb.ObjectData{Hash: hashToPB(f.list), Kind: syncpb.ObjectKind_OBJECT_KIND_CHUNK_LIST, Data: data})
	return f
}

// makeMixedCommit builds a snapshot with plain files and chunked files.
// commit.objects holds the plain blobs and the manifest only.
func makeMixedCommit(treeID string, files map[string]string, chunked map[string]chunkedFile, parents ...commit) commit {
	var paths []string
	for p := range files {
		paths = append(paths, p)
	}
	for p := range chunked {
		paths = append(paths, p)
	}
	sort.Strings(paths)
	m := &syncpb.ManifestRecord{}
	var objs []*syncpb.ObjectData
	for _, p := range paths {
		if f, ok := chunked[p]; ok {
			var size uint64
			for _, o := range f.objects[:len(f.objects)-1] {
				size += uint64(len(o.GetData()))
			}
			m.Entries = append(m.Entries, &syncpb.ManifestEntry{Path: p, BlobHash: hashToPB(f.list), Size: size, Chunked: true})
			continue
		}
		data := []byte(files[p])
		bh := hash.Sum(data)
		m.Entries = append(m.Entries, &syncpb.ManifestEntry{Path: p, BlobHash: hashToPB(bh), Size: uint64(len(data))})
		objs = append(objs, &syncpb.ObjectData{Hash: hashToPB(bh), Kind: syncpb.ObjectKind_OBJECT_KIND_BLOB, Data: data})
	}
	mdata, err := proto.MarshalOptions{Deterministic: true}.Marshal(m)
	if err != nil {
		panic(err)
	}
	mh := hash.Sum(mdata)
	objs = append(objs, &syncpb.ObjectData{Hash: hashToPB(mh), Kind: syncpb.ObjectKind_OBJECT_KIND_MANIFEST, Data: mdata})
	commitClock = commitClock.Add(time.Minute)
	id := newID()
	rec := &syncpb.SnapshotRecord{
		Id:           &syncpb.SnapshotId{Value: id},
		TreeId:       &syncpb.TreeId{Value: treeID},
		BranchName:   "main",
		ManifestHash: hashToPB(mh),
		Author:       "alice",
		Message:      "commit " + id[:8],
		CreatedAt:    timestamppb.New(commitClock),
	}
	for _, p := range parents {
		rec.Parents = append(rec.Parents, &syncpb.SnapshotId{Value: p.id})
	}
	return commit{id: id, rec: rec, manifest: mh, objects: objs}
}

func TestUploadObjectsValidatesChunkLists(t *testing.T) {
	h := newHarness(t)
	ctx := h.as("alice")
	f := makeChunkedFile("aaaa", "bb")
	resp, err := h.uploadObjects(ctx, f.objects...)
	if err != nil || len(resp.GetRejected()) != 0 || resp.GetStored() != 3 {
		t.Fatalf("upload = %v, %v", resp, err)
	}
	bad := &syncpb.ChunkList{Size: 99, Chunks: []*syncpb.Chunk{{Hash: hashToPB(f.chunks[0]), Size: 4}}}
	data, _ := proto.Marshal(bad)
	shortHash := &syncpb.ChunkList{Size: 1, Chunks: []*syncpb.Chunk{{Hash: &syncpb.ContentHash{Value: []byte("x")}, Size: 1}}}
	data2, _ := proto.Marshal(shortHash)
	resp, err = h.uploadObjects(ctx,
		&syncpb.ObjectData{Hash: hashToPB(hash.Sum(data)), Kind: syncpb.ObjectKind_OBJECT_KIND_CHUNK_LIST, Data: data},
		&syncpb.ObjectData{Hash: hashToPB(hash.Sum(data2)), Kind: syncpb.ObjectKind_OBJECT_KIND_CHUNK_LIST, Data: data2},
		&syncpb.ObjectData{Hash: hashToPB(hash.Sum([]byte("junk"))), Kind: syncpb.ObjectKind_OBJECT_KIND_CHUNK_LIST, Data: []byte("junk\xff")},
	)
	if err != nil || len(resp.GetRejected()) != 3 {
		t.Fatalf("invalid chunk lists = %v, %v", resp, err)
	}
}

func TestPushValidatesChunkedEntries(t *testing.T) {
	h := newHarness(t)
	ctx := h.as("alice")
	tree := h.newTree("big")

	// Each case returns the commit to push and the objects to upload first.
	cases := []struct {
		name  string
		build func() (commit, []*syncpb.ObjectData)
	}{
		{"chunk missing", func() (commit, []*syncpb.ObjectData) {
			f := makeChunkedFile("c1-first", "c1-second")
			c := makeMixedCommit(tree, map[string]string{"small": "1"}, map[string]chunkedFile{"big.bin": f})
			return c, append(append([]*syncpb.ObjectData{}, c.objects...), f.objects[1:]...)
		}},
		{"chunk list missing", func() (commit, []*syncpb.ObjectData) {
			f := makeChunkedFile("c2-first", "c2-second")
			c := makeMixedCommit(tree, map[string]string{"small": "2"}, map[string]chunkedFile{"big.bin": f})
			return c, append(append([]*syncpb.ObjectData{}, c.objects...), f.objects[:len(f.objects)-1]...)
		}},
		{"chunk list is not a ChunkList", func() (commit, []*syncpb.ObjectData) {
			junk := []byte("not a chunk list")
			f := chunkedFile{list: hash.Sum(junk), objects: []*syncpb.ObjectData{
				// Smuggled in as a BLOB, so upload validation does not see it.
				{Hash: hashToPB(hash.Sum(junk)), Kind: syncpb.ObjectKind_OBJECT_KIND_BLOB, Data: junk},
			}}
			c := makeMixedCommit(tree, map[string]string{"small": "3"}, map[string]chunkedFile{"big.bin": f})
			return c, append(append([]*syncpb.ObjectData{}, c.objects...), f.objects...)
		}},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			c, objs := tc.build()
			if _, err := h.uploadObjects(ctx, objs...); err != nil {
				t.Fatal(err)
			}
			_, err := h.client.Push(ctx, pushReq(tree, "main", "", c))
			wantCode(t, err, codes.FailedPrecondition)
		})
	}

	// Everything present: accepted.
	f := makeChunkedFile("one", "two", "one")
	c := makeMixedCommit(tree, map[string]string{"small": "ok"}, map[string]chunkedFile{"big.bin": f})
	h.upload(ctx, c)
	if _, err := h.uploadObjects(ctx, f.objects...); err != nil {
		t.Fatal(err)
	}
	h.mustPush(ctx, pushReq(tree, "main", "", c))
	if got := h.tip(tree, "main"); got != c.id {
		t.Fatalf("tip = %q", got)
	}
}

func TestDownloadObjectsServesChunksAndHonoursLicense(t *testing.T) {
	h := newHarness(t)
	ctx := h.as("alice")
	tree := h.newTree("big")
	open := makeChunkedFile("open-1", "shared-chunk")
	secret := makeChunkedFile("secret-1", "shared-chunk")
	c := makeMixedCommit(tree,
		map[string]string{"readme": "hello", "vendor/gpl.c": "restricted"},
		map[string]chunkedFile{"open.bin": open, "vendor/secret.bin": secret})
	h.upload(ctx, c)
	if _, err := h.uploadObjects(ctx, append(open.objects, secret.objects...)...); err != nil {
		t.Fatal(err)
	}
	h.mustPush(ctx, pushReq(tree, "main", "", c))

	scoped := metadata.AppendToOutgoingContext(h.as("bob"), TreeIDMetadataKey, tree)
	gpl := hash.Sum([]byte("restricted"))
	readme := hash.Sum([]byte("hello"))
	shared := hash.Sum([]byte("shared-chunk"))
	want := []*syncpb.ContentHash{
		hashToPB(c.manifest), hashToPB(readme), hashToPB(gpl),
		hashToPB(open.list), hashToPB(open.chunks[0]), hashToPB(shared),
		hashToPB(secret.list), hashToPB(secret.chunks[0]),
	}
	kinds := func(objs []*syncpb.ObjectData) map[hash.Hash]syncpb.ObjectKind {
		out := map[hash.Hash]syncpb.ObjectKind{}
		for _, o := range objs {
			out[hash.Hash(o.GetHash().GetValue())] = o.GetKind()
		}
		return out
	}

	// Everything readable: all eight, labelled by kind.
	got := kinds(h.download(t, scoped, want))
	if len(got) != 8 {
		t.Fatalf("downloaded %d objects, want 8", len(got))
	}
	for h0, k := range map[hash.Hash]syncpb.ObjectKind{
		c.manifest:  syncpb.ObjectKind_OBJECT_KIND_MANIFEST,
		readme:      syncpb.ObjectKind_OBJECT_KIND_BLOB,
		open.list:   syncpb.ObjectKind_OBJECT_KIND_CHUNK_LIST,
		shared:      syncpb.ObjectKind_OBJECT_KIND_BLOB,
		secret.list: syncpb.ObjectKind_OBJECT_KIND_CHUNK_LIST,
	} {
		if got[h0] != k {
			t.Fatalf("kind of %s = %v, want %v", h0.Hex(), got[h0], k)
		}
	}

	// License rules deny vendor/: its blob, chunk list and private chunk
	// are withheld; the chunk shared with a readable file is still served.
	h.license.setRead(func(paths []string) error {
		for _, p := range paths {
			if len(p) >= 7 && p[:7] == "vendor/" {
				return &domain.PushDenied{Kind: "license_violation", Path: p, License: "GPL-3.0"}
			}
		}
		return nil
	})
	got = kinds(h.download(t, scoped, want))
	for _, withheld := range []hash.Hash{gpl, secret.list, secret.chunks[0]} {
		if _, ok := got[withheld]; ok {
			t.Fatalf("license-restricted object %s was served", withheld.Hex())
		}
	}
	for _, served := range []hash.Hash{c.manifest, readme, open.list, open.chunks[0], shared} {
		if _, ok := got[served]; !ok {
			t.Fatalf("readable object %s was withheld", served.Hex())
		}
	}

	// The header is mandatory for non-admins.
	wantCode(t, h.downloadErr(h.as("bob"), want), codes.FailedPrecondition)
	wantCode(t, h.downloadErr(metadata.AppendToOutgoingContext(h.as("carol"), TreeIDMetadataKey, tree), want), codes.PermissionDenied)
}
