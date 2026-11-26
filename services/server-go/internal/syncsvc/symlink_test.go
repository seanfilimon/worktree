package syncsvc

// Symbolic links in manifests: the entry rules checked by decodeManifest and
// the target checks of Push. The server only stores link targets; nothing
// here resolves or follows one.

import (
	"errors"
	"sort"
	"strings"
	"testing"

	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/metadata"
	"google.golang.org/grpc/status"
	"google.golang.org/protobuf/proto"

	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
)

func TestDecodeManifestSymlinkEntries(t *testing.T) {
	h := hashToPB(hash.Sum([]byte("target")))
	link := func(mutate func(*syncpb.ManifestEntry)) []byte {
		e := &syncpb.ManifestEntry{Path: "docs/latest", BlobHash: h, Size: 6, Symlink: true}
		if mutate != nil {
			mutate(e)
		}
		return mustMarshal(t, &syncpb.ManifestRecord{Entries: []*syncpb.ManifestEntry{e}})
	}
	tests := []struct {
		name   string
		mutate func(*syncpb.ManifestEntry)
		ok     bool
	}{
		{name: "valid", ok: true},
		{name: "longest target", mutate: func(e *syncpb.ManifestEntry) { e.Size = maxSymlinkTarget }, ok: true},
		{name: "executable", mutate: func(e *syncpb.ManifestEntry) { e.Executable = true }},
		{name: "chunked", mutate: func(e *syncpb.ManifestEntry) { e.Chunked = true }},
		{name: "empty target", mutate: func(e *syncpb.ManifestEntry) { e.Size = 0 }},
		{name: "target too long", mutate: func(e *syncpb.ManifestEntry) { e.Size = maxSymlinkTarget + 1 }},
		{name: "in .wt-tree", mutate: func(e *syncpb.ManifestEntry) { e.Path = ".wt-tree/hooks/pre-push" }},
		{name: "path still validated", mutate: func(e *syncpb.ManifestEntry) { e.Path = "../escape" }},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			m, err := decodeManifest(link(tt.mutate))
			switch {
			case tt.ok && err != nil:
				t.Fatalf("rejected: %v", err)
			case tt.ok && !m.GetEntries()[0].GetSymlink():
				t.Fatal("symlink flag lost")
			case !tt.ok && !errors.Is(err, errInvalidManifest):
				t.Fatalf("accepted (err = %v)", err)
			}
		})
	}
}

// A manifest written before the symlink field existed decodes unchanged and
// re-encodes to the same bytes: the field is additive and false is not
// encoded, so existing manifest hashes are stable.
func TestManifestWithoutSymlinksKeepsItsBytes(t *testing.T) {
	bh := hash.Sum([]byte("old"))
	// Hand-encoded entry: path=1, blob_hash=2{value=1}, size=3, executable=4.
	entry := append([]byte{0x0A, 0x05}, "a.txt"...)
	entry = append(entry, 0x12, 0x22, 0x0A, 0x20)
	entry = append(entry, bh[:]...)
	entry = append(entry, 0x18, 0x03, 0x20, 0x01)
	old := append([]byte{0x0A, byte(len(entry))}, entry...)

	m, err := decodeManifest(old)
	if err != nil {
		t.Fatal(err)
	}
	e := m.GetEntries()[0]
	if e.GetPath() != "a.txt" || e.GetSize() != 3 || !e.GetExecutable() || e.GetChunked() || e.GetSymlink() {
		t.Fatalf("decoded %v", e)
	}
	again, err := proto.MarshalOptions{Deterministic: true}.Marshal(m)
	if err != nil {
		t.Fatal(err)
	}
	if string(again) != string(old) {
		t.Fatalf("re-encoded bytes differ:\n got %x\nwant %x", again, old)
	}
}

// linkCommit builds a snapshot of regular files and symbolic links
// (path -> target). sizes overrides the recorded size of a link.
func linkCommit(treeID string, files, links map[string]string, sizes map[string]uint64, parents ...commit) commit {
	c := makeCommit(treeID, files, parents...)
	var m syncpb.ManifestRecord
	for _, o := range c.objects {
		if o.GetKind() == syncpb.ObjectKind_OBJECT_KIND_MANIFEST {
			if err := proto.Unmarshal(o.GetData(), &m); err != nil {
				panic(err)
			}
		}
	}
	objs := c.objects[:len(c.objects)-1]
	for p, target := range links {
		bh := hash.Sum([]byte(target))
		size := uint64(len(target))
		if s, ok := sizes[p]; ok {
			size = s
		}
		m.Entries = append(m.Entries, &syncpb.ManifestEntry{Path: p, BlobHash: hashToPB(bh), Size: size, Symlink: true})
		objs = append(objs, &syncpb.ObjectData{Hash: hashToPB(bh), Kind: syncpb.ObjectKind_OBJECT_KIND_BLOB, Data: []byte(target)})
	}
	sort.Slice(m.Entries, func(i, j int) bool { return m.Entries[i].GetPath() < m.Entries[j].GetPath() })
	mdata, err := proto.MarshalOptions{Deterministic: true}.Marshal(&m)
	if err != nil {
		panic(err)
	}
	mh := hash.Sum(mdata)
	c.objects = append(objs, &syncpb.ObjectData{Hash: hashToPB(mh), Kind: syncpb.ObjectKind_OBJECT_KIND_MANIFEST, Data: mdata})
	c.manifest = mh
	c.rec.ManifestHash = hashToPB(mh)
	return c
}

func TestPushAcceptsAndServesSymlinks(t *testing.T) {
	h := newHarness(t)
	ctx := h.as("alice")
	treeID := h.newTree("repo")
	// Absolute targets and targets leading out of the tree are stored as
	// they are: the server never resolves them.
	c1 := linkCommit(treeID,
		map[string]string{"docs/guide.md": "# guide\n"},
		map[string]string{"docs/latest": "guide.md", "abs": "/etc/hosts", "up": "../../outside"}, nil)
	h.upload(ctx, c1)
	h.mustPush(ctx, pushReq(treeID, "main", "", c1))
	if got := h.tip(treeID, "main"); got != c1.id {
		t.Fatalf("tip = %s", got)
	}

	m, err := h.svc.loadManifest(ctx, c1.manifest)
	if err != nil {
		t.Fatal(err)
	}
	links := map[string]bool{}
	for _, e := range m.GetEntries() {
		links[e.GetPath()] = e.GetSymlink()
	}
	for p, want := range map[string]bool{"abs": true, "docs/guide.md": false, "docs/latest": true, "up": true} {
		if links[p] != want {
			t.Errorf("%s: symlink = %v, want %v", p, links[p], want)
		}
	}
	// The target comes back as the bytes that were uploaded.
	target := hash.Sum([]byte("guide.md"))
	objs := h.download(t, metadata.AppendToOutgoingContext(ctx, TreeIDMetadataKey, treeID), []*syncpb.ContentHash{hashToPB(target)})
	if len(objs) != 1 || string(objs[0].GetData()) != "guide.md" {
		t.Fatalf("downloaded %v", objs)
	}

	// An incremental push on top keeps the unchanged links (trusted from
	// the tip) and validates the new one.
	c2 := linkCommit(treeID,
		map[string]string{"docs/guide.md": "# guide\n"},
		map[string]string{"docs/latest": "guide.md", "abs": "/etc/hosts", "up": "../../outside", "new": "docs"}, nil, c1)
	h.upload(ctx, c2)
	h.mustPush(ctx, pushReq(treeID, "main", c1.id, c2))
}

func TestPushRejectsInvalidSymlinkTargets(t *testing.T) {
	tests := []struct {
		name   string
		target string
		sizes  map[string]uint64
		want   string
	}{
		{name: "NUL in target", target: "a\x00b", want: "NUL"},
		{name: "size mismatch", target: "guide.md", sizes: map[string]uint64{"link": 3}, want: "records 3"},
		{name: "too long", target: strings.Repeat("a", maxSymlinkTarget+1), sizes: map[string]uint64{"link": 10}, want: "records 10"},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			h := newHarness(t)
			ctx := h.as("alice")
			treeID := h.newTree("repo")
			c := linkCommit(treeID, map[string]string{"a.txt": "a"}, map[string]string{"link": tt.target}, tt.sizes)
			h.upload(ctx, c)
			_, err := h.client.Push(ctx, pushReq(treeID, "main", "", c))
			wantCode(t, err, codes.FailedPrecondition)
			msg := status.Convert(err).Message()
			if !strings.Contains(msg, "symbolic link link") || !strings.Contains(msg, tt.want) {
				t.Fatalf("error = %q, want it to name the link and %q", msg, tt.want)
			}
			if got := h.tip(treeID, "main"); got != "" {
				t.Fatalf("tip moved to %s", got)
			}
		})
	}

	// A file's blob that is already trusted as a branch tip's content is
	// still validated when a later snapshot reuses it as a link target.
	h := newHarness(t)
	ctx := h.as("alice")
	treeID := h.newTree("repo")
	c1 := makeCommit(treeID, map[string]string{"bin.dat": "a\x00b"})
	h.upload(ctx, c1)
	h.mustPush(ctx, pushReq(treeID, "main", "", c1))
	c2 := linkCommit(treeID, map[string]string{"bin.dat": "a\x00b"}, map[string]string{"link": "a\x00b"}, nil, c1)
	h.upload(ctx, c2)
	_, err := h.client.Push(ctx, pushReq(treeID, "main", c1.id, c2))
	wantCode(t, err, codes.FailedPrecondition)

	// A type change with identical bytes is a change (path rules see it).
	bh := hashToPB(hash.Sum([]byte("x")))
	file := &syncpb.ManifestRecord{Entries: []*syncpb.ManifestEntry{{Path: "p", BlobHash: bh, Size: 1}}}
	link := &syncpb.ManifestRecord{Entries: []*syncpb.ManifestEntry{{Path: "p", BlobHash: bh, Size: 1, Symlink: true}}}
	if got := diffManifests(file, link); len(got) != 1 || got[0] != "p" {
		t.Fatalf("diff = %v", got)
	}
}
