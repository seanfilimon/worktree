package httpapi

// Symbolic links in the REST API: listed and served as links (the stored
// target text), never resolved.

import (
	"archive/tar"
	"archive/zip"
	"bytes"
	"compress/gzip"
	"io"
	"io/fs"
	"testing"
	"time"

	"google.golang.org/protobuf/proto"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
)

// addLinkSnapshot stores snapshot "snap-links" of the web tree: README.md
// plus the links docs/latest -> ../README.md and passwd -> /etc/passwd.
func addLinkSnapshot(t *testing.T, f *fixture) {
	t.Helper()
	entry := func(p, data string, link bool) *syncpb.ManifestEntry {
		h := f.objs.put([]byte(data))
		return &syncpb.ManifestEntry{Path: p, BlobHash: &syncpb.ContentHash{Value: h[:]}, Size: uint64(len(data)), Symlink: link}
	}
	m := &syncpb.ManifestRecord{Entries: []*syncpb.ManifestEntry{
		entry("README.md", "# web\n", false),
		entry("docs/latest", "../README.md", true),
		entry("passwd", "/etc/passwd", true),
	}}
	raw, err := proto.Marshal(m)
	if err != nil {
		t.Fatal(err)
	}
	f.store.snapshots["snap-links"] = domain.Snapshot{
		ID: "snap-links", TreeID: f.web.ID, BranchName: "main", ManifestHash: f.objs.put(raw),
		Author: "alice", Message: "links", CreatedAt: time.Date(2026, 1, 2, 0, 0, 0, 0, time.UTC),
	}
}

func TestSnapshotDetailMarksSymlinks(t *testing.T) {
	f := newFixture(t)
	addLinkSnapshot(t, f)
	rec := f.do(t, "GET", "/api/v1/trees/acme/web/snapshots/snap-links", "carol", nil)
	if rec.Code != 200 {
		t.Fatalf("status = %d: %s", rec.Code, rec.Body.String())
	}
	d := decode[snapshotDetail](t, rec)
	got := map[string]bool{}
	for _, fe := range d.Files {
		got[fe.Path] = fe.Symlink
	}
	want := map[string]bool{"README.md": false, "docs/latest": true, "passwd": true}
	for p, link := range want {
		if v, ok := got[p]; !ok || v != link {
			t.Errorf("%s: symlink = %v (present %v), want %v", p, v, ok, link)
		}
	}
	// The field is omitted for ordinary files, so existing clients see the
	// same JSON as before.
	raw := decode[struct {
		Files []map[string]any `json:"files"`
	}](t, rec)
	for _, fe := range raw.Files {
		_, has := fe["symlink"]
		if has != want[fe["path"].(string)] {
			t.Errorf("%v: symlink key present = %v", fe["path"], has)
		}
	}
}

func TestBlobOfSymlinkIsItsTargetText(t *testing.T) {
	f := newFixture(t)
	addLinkSnapshot(t, f)
	// An absolute target is returned as text; nothing on the server's own
	// file system is read.
	rec := f.do(t, "GET", "/api/v1/trees/acme/web/blob/snap-links/passwd", "carol", nil)
	if rec.Code != 200 || rec.Body.String() != "/etc/passwd" {
		t.Fatalf("status %d body %q", rec.Code, rec.Body.String())
	}
	if h := rec.Header(); h.Get(symlinkHeader) != "true" || h.Get("Content-Type") != "text/plain; charset=utf-8" {
		t.Fatalf("headers = %v", h)
	}
	// A link to another file of the snapshot is not followed either.
	rec = f.do(t, "GET", "/api/v1/trees/acme/web/blob/snap-links/docs/latest", "carol", nil)
	if rec.Code != 200 || rec.Body.String() != "../README.md" {
		t.Fatalf("status %d body %q", rec.Code, rec.Body.String())
	}
	// Ordinary files carry no symlink header.
	rec = f.do(t, "GET", "/api/v1/trees/acme/web/blob/snap-links/README.md", "carol", nil)
	if rec.Code != 200 || rec.Header().Get(symlinkHeader) != "" {
		t.Fatalf("status %d headers %v", rec.Code, rec.Header())
	}
}

func TestArchivesCarrySymlinksAsLinks(t *testing.T) {
	target := "../README.md"
	th := hash.Sum([]byte(target))
	files := []archiveFile{
		{entry: &syncpb.ManifestEntry{Path: "README.md", Size: 6}, data: []byte("# web\n")},
		{entry: &syncpb.ManifestEntry{Path: "docs/latest", BlobHash: &syncpb.ContentHash{Value: th[:]}, Size: uint64(len(target)), Symlink: true}},
	}
	read := func(f archiveFile) ([]byte, error) {
		if f.data != nil {
			return f.data, nil
		}
		return []byte(target), nil
	}
	mtime := time.Date(2026, 1, 2, 0, 0, 0, 0, time.UTC)

	var buf bytes.Buffer
	if err := writeTarGzArchive(&buf, files, mtime, read); err != nil {
		t.Fatal(err)
	}
	gz, err := gzip.NewReader(&buf)
	if err != nil {
		t.Fatal(err)
	}
	tr := tar.NewReader(gz)
	seen := 0
	for {
		h, err := tr.Next()
		if err == io.EOF {
			break
		}
		if err != nil {
			t.Fatal(err)
		}
		seen++
		if h.Name != "docs/latest" {
			continue
		}
		body, _ := io.ReadAll(tr)
		if h.Typeflag != tar.TypeSymlink || h.Linkname != target || h.Size != 0 || len(body) != 0 {
			t.Fatalf("tar member = %+v, body %q", h, body)
		}
	}
	if seen != 2 {
		t.Fatalf("tar members = %d", seen)
	}

	buf.Reset()
	if err := writeZipArchive(&buf, files, mtime, read); err != nil {
		t.Fatal(err)
	}
	zr, err := zip.NewReader(bytes.NewReader(buf.Bytes()), int64(buf.Len()))
	if err != nil {
		t.Fatal(err)
	}
	for _, zf := range zr.File {
		rc, err := zf.Open()
		if err != nil {
			t.Fatal(err)
		}
		body, _ := io.ReadAll(rc)
		rc.Close()
		isLink := zf.Mode()&fs.ModeSymlink != 0
		switch zf.Name {
		case "docs/latest":
			if !isLink || string(body) != target {
				t.Fatalf("zip link: mode %v body %q", zf.Mode(), body)
			}
		default:
			if isLink {
				t.Fatalf("%s is a link", zf.Name)
			}
		}
	}
}
