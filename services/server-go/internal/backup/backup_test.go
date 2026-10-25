package backup

import (
	"archive/tar"
	"bytes"
	"context"
	"errors"
	"fmt"
	"io"
	"log/slog"
	"net/http/httptest"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/klauspost/compress/zstd"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
	"github.com/seanfilimon/worktree/services/server-go/internal/objstore"
)

func must(t *testing.T, err error) {
	t.Helper()
	if err != nil {
		t.Fatal(err)
	}
}

type fixture struct {
	dataDir string
	db      *meta.Bolt
	objs    objstore.Store
	svc     *Service
	hashes  []hash.Hash
	content map[hash.Hash][]byte
}

var quiet = slog.New(slog.NewTextHandler(io.Discard, nil))

// newFixture opens a bbolt store in a temp data dir with one tenant and a
// few objects in objs (a DiskStore in the data dir when nil).
func newFixture(t *testing.T, objs objstore.Store) *fixture {
	t.Helper()
	ctx := context.Background()
	dir := t.TempDir()
	db, err := meta.NewBolt(filepath.Join(dir, "meta.db"))
	must(t, err)
	t.Cleanup(func() { _ = db.Close() })
	if objs == nil {
		objs = objstore.New(dir)
	}
	must(t, db.CreateTenant(ctx, &domain.Tenant{ID: "t-acme", Slug: "acme", Name: "Acme", Kind: domain.TenantOrg, Plan: "free", CreatedAt: time.Now().UTC()}))
	must(t, db.PutDoc(ctx, "test_docs", "k", map[string]string{"hello": "world"}))
	f := &fixture{dataDir: dir, db: db, objs: objs, content: map[hash.Hash][]byte{}}
	for _, data := range [][]byte{
		[]byte("small"),
		bytes.Repeat([]byte("compressible "), 500),
		[]byte("WTZ1 starts with the magic"),
		{},
	} {
		h := hash.Sum(data)
		must(t, objs.Put(ctx, h, data))
		f.hashes = append(f.hashes, h)
		f.content[h] = data
	}
	f.svc = New(Deps{Store: db, Objects: objs, Logger: quiet})
	f.svc.Configure(Options{Dir: filepath.Join(dir, "backups"), Keep: 2, Version: "test"})
	return f
}

// verifyRestored checks that dataDir/meta.db holds the fixture's data and
// that objs has every object.
func (f *fixture) verifyRestored(t *testing.T, dataDir string, objs objstore.Store) {
	t.Helper()
	ctx := context.Background()
	db, err := meta.NewBolt(filepath.Join(dataDir, "meta.db"))
	must(t, err)
	defer db.Close()
	tn, err := db.GetTenantBySlug(ctx, "acme")
	if err != nil || tn.ID != "t-acme" {
		t.Fatalf("restored tenant = %+v, %v", tn, err)
	}
	var doc map[string]string
	if err := db.GetDoc(ctx, "test_docs", "k", &doc); err != nil || doc["hello"] != "world" {
		t.Fatalf("restored doc = %v, %v", doc, err)
	}
	for h, want := range f.content {
		got, err := objs.Get(ctx, h)
		if err != nil || !bytes.Equal(got, want) {
			t.Fatalf("restored object %s: %v", h, err)
		}
	}
}

func TestCreateListRestoreDisk(t *testing.T) {
	ctx := context.Background()
	f := newFixture(t, nil)

	info, err := f.svc.Create(ctx, "admin", "before upgrade")
	must(t, err)
	if !ValidID(info.ID) || info.Objects != int64(len(f.hashes)) || info.CreatedBy != "admin" || info.Note != "before upgrade" || info.SizeBytes <= 0 {
		t.Fatalf("info = %+v", info)
	}
	if filepath.Base(info.Path) != info.ID+FileExt || !filepath.IsAbs(info.Path) {
		t.Fatalf("path = %s", info.Path)
	}
	if _, err := os.Stat(info.Path + ".partial"); !errors.Is(err, os.ErrNotExist) {
		t.Fatal("partial file left behind")
	}

	m, err := ReadManifest(info.Path)
	must(t, err)
	if m.Format != FormatName || m.Version != FormatVersion || m.ID != info.ID || m.Objects != info.Objects ||
		m.ObjectStore != "disk" || m.ServerVersion != "test" || m.MetaDB.Size <= 0 || len(m.MetaDB.SHA256) != 64 {
		t.Fatalf("manifest = %+v", m)
	}

	list, err := f.svc.List(ctx)
	must(t, err)
	if len(list) != 1 || list[0].ID != info.ID || list[0].Note != "before upgrade" {
		t.Fatalf("list = %+v", list)
	}
	got, err := f.svc.Get(ctx, info.ID)
	if err != nil || got.ID != info.ID {
		t.Fatalf("Get = %+v, %v", got, err)
	}

	// Restore into a fresh data directory.
	target := filepath.Join(t.TempDir(), "restored")
	rep, err := Restore(ctx, RestoreOptions{File: info.Path, DataDir: target, Objects: objstore.New(target)})
	must(t, err)
	if rep.Objects != info.Objects || rep.Manifest.ID != info.ID {
		t.Fatalf("report = %+v", rep)
	}
	f.verifyRestored(t, target, objstore.New(target))
	if _, err := os.Stat(filepath.Join(target, "meta.db.restoring")); !errors.Is(err, os.ErrNotExist) {
		t.Fatal("temporary database left behind")
	}

	// A second restore needs --force; with it, meta.db is replaced.
	_, err = Restore(ctx, RestoreOptions{File: info.Path, DataDir: target, Objects: objstore.New(target)})
	if !errors.Is(err, ErrDataDirNotEmpty) {
		t.Fatalf("restore into non-empty dir = %v", err)
	}
	_, err = Restore(ctx, RestoreOptions{File: info.Path, DataDir: target, Objects: objstore.New(target), Force: true})
	must(t, err)
	f.verifyRestored(t, target, objstore.New(target))

	// Delete.
	must(t, f.svc.Delete(ctx, info.ID))
	if _, err := os.Stat(info.Path); !errors.Is(err, os.ErrNotExist) {
		t.Fatal("archive not deleted")
	}
	if err := f.svc.Delete(ctx, info.ID); !errors.Is(err, ErrNotFound) {
		t.Fatalf("second delete = %v", err)
	}
	if _, err := f.svc.Get(ctx, info.ID); !errors.Is(err, domain.ErrNotFound) {
		t.Fatalf("Get deleted = %v", err)
	}
}

func TestBackupAndRestoreS3(t *testing.T) {
	ctx := context.Background()
	fake := objstore.NewFakeS3()
	srv := httptest.NewServer(fake)
	defer srv.Close()
	open := func(bucket string) *objstore.S3Store {
		s, err := objstore.NewS3(ctx, objstore.S3Options{Endpoint: srv.URL, Bucket: bucket, Region: "us-east-1",
			AccessKey: "a", SecretKey: "b", Compress: true, MaxRetries: 1})
		must(t, err)
		return s
	}
	f := newFixture(t, open("source"))
	info, err := f.svc.Create(ctx, "admin", "")
	must(t, err)
	m, err := ReadManifest(info.Path)
	must(t, err)
	if m.ObjectStore != "s3" || m.Objects != int64(len(f.hashes)) {
		t.Fatalf("manifest = %+v", m)
	}

	// Restore into another bucket (e.g. a new region) ...
	target := t.TempDir()
	dest := open("dest")
	_, err = Restore(ctx, RestoreOptions{File: info.Path, DataDir: target, Objects: dest})
	must(t, err)
	f.verifyRestored(t, target, dest)
	// ... or from S3 to disk.
	disk := filepath.Join(t.TempDir(), "d")
	_, err = Restore(ctx, RestoreOptions{File: info.Path, DataDir: disk, Objects: objstore.New(disk)})
	must(t, err)
	f.verifyRestored(t, disk, objstore.New(disk))

	st, err := f.svc.Status(ctx)
	must(t, err)
	if st.Store.Kind != "s3" || st.Store.Bucket != "source" || !st.Supported || len(st.Backups) != 1 {
		t.Fatalf("status = %+v", st)
	}
}

// fakeSource serves objects from a map (bad entries do not match their key).
type fakeSource struct {
	objs map[hash.Hash][]byte
}

func (s fakeSource) Put(context.Context, hash.Hash, []byte) error { return nil }
func (s fakeSource) Get(_ context.Context, h hash.Hash) ([]byte, error) {
	d, ok := s.objs[h]
	if !ok {
		return nil, objstore.ErrNotFound
	}
	return d, nil
}
func (s fakeSource) Has(_ context.Context, h hash.Hash) (bool, error) {
	_, ok := s.objs[h]
	return ok, nil
}
func (s fakeSource) Delete(context.Context, hash.Hash) error { return nil }
func (s fakeSource) List(_ context.Context, fn func(hash.Hash) error) error {
	keys := make([]hash.Hash, 0, len(s.objs))
	for h := range s.objs {
		keys = append(keys, h)
	}
	slices.SortFunc(keys, func(a, b hash.Hash) int { return bytes.Compare(a[:], b[:]) })
	for _, h := range keys {
		if err := fn(h); err != nil {
			return err
		}
	}
	return nil
}

func archiveOf(t *testing.T, f *fixture, src objectSource) string {
	t.Helper()
	path := filepath.Join(t.TempDir(), "x.wtbak")
	out, err := os.Create(path)
	must(t, err)
	_, err = writeArchive(context.Background(), out, f.db, src, Manifest{Format: FormatName, Version: FormatVersion, ID: "bk-x", CreatedAt: time.Now()})
	must(t, err)
	must(t, out.Close())
	return path
}

func TestRestoreRejectsCorruptArchives(t *testing.T) {
	ctx := context.Background()
	f := newFixture(t, nil)
	restore := func(path string) error {
		target := filepath.Join(t.TempDir(), "r")
		_, err := Restore(ctx, RestoreOptions{File: path, DataDir: target, Objects: objstore.New(target)})
		if _, serr := os.Stat(filepath.Join(target, "meta.db")); err != nil && !errors.Is(serr, os.ErrNotExist) {
			t.Errorf("failed restore left meta.db behind (%v)", err)
		}
		return err
	}

	t.Run("object does not match its hash", func(t *testing.T) {
		good := []byte("good")
		path := archiveOf(t, f, fakeSource{objs: map[hash.Hash][]byte{
			hash.Sum(good):            good,
			hash.Sum([]byte("other")): []byte("tampered"),
		}})
		err := restore(path)
		if !errors.Is(err, ErrCorruptArchive) || !strings.Contains(err.Error(), "does not match its hash") {
			t.Fatalf("restore = %v", err)
		}
	})

	t.Run("truncated", func(t *testing.T) {
		info, err := f.svc.Create(ctx, "admin", "")
		must(t, err)
		data, err := os.ReadFile(info.Path)
		must(t, err)
		cut := filepath.Join(t.TempDir(), "cut.wtbak")
		must(t, os.WriteFile(cut, data[:len(data)*2/3], 0o600))
		if err := restore(cut); !errors.Is(err, ErrCorruptArchive) {
			t.Fatalf("restore truncated = %v", err)
		}
	})

	t.Run("not an archive", func(t *testing.T) {
		junk := filepath.Join(t.TempDir(), "junk.wtbak")
		must(t, os.WriteFile(junk, []byte("definitely not zstd"), 0o600))
		if err := restore(junk); !errors.Is(err, ErrCorruptArchive) {
			t.Fatalf("restore junk = %v", err)
		}
	})

	// rewrite re-packs a good archive through edit.
	rewrite := func(t *testing.T, edit func(name string, body []byte) (string, []byte, bool)) string {
		info, err := f.svc.Create(ctx, "admin", "")
		must(t, err)
		in, err := os.Open(info.Path)
		must(t, err)
		defer in.Close()
		zr, err := zstd.NewReader(in)
		must(t, err)
		defer zr.Close()
		path := filepath.Join(t.TempDir(), "edited.wtbak")
		out, err := os.Create(path)
		must(t, err)
		zw, err := zstd.NewWriter(out)
		must(t, err)
		tr, tw := tar.NewReader(zr), tar.NewWriter(zw)
		for {
			hdr, err := tr.Next()
			if errors.Is(err, io.EOF) {
				break
			}
			must(t, err)
			body, err := io.ReadAll(tr)
			must(t, err)
			name, nb, keep := edit(hdr.Name, body)
			if !keep {
				continue
			}
			must(t, tw.WriteHeader(&tar.Header{Name: name, Size: int64(len(nb)), Mode: 0o644, Typeflag: tar.TypeReg}))
			_, err = tw.Write(nb)
			must(t, err)
		}
		must(t, tw.Close())
		must(t, zw.Close())
		must(t, out.Close())
		return path
	}

	t.Run("meta.db tampered", func(t *testing.T) {
		path := rewrite(t, func(name string, body []byte) (string, []byte, bool) {
			if name == MemberMetaDB {
				body = slices.Clone(body)
				body[len(body)-1] ^= 0xff
			}
			return name, body, true
		})
		if err := restore(path); !errors.Is(err, ErrCorruptArchive) || !strings.Contains(err.Error(), "meta.db does not match") {
			t.Fatalf("restore = %v", err)
		}
	})

	t.Run("object missing", func(t *testing.T) {
		dropped := false
		path := rewrite(t, func(name string, body []byte) (string, []byte, bool) {
			if strings.HasPrefix(name, "objects/") && !dropped {
				dropped = true
				return name, body, false
			}
			return name, body, true
		})
		if err := restore(path); !errors.Is(err, ErrCorruptArchive) || !strings.Contains(err.Error(), "objects in the archive") {
			t.Fatalf("restore = %v", err)
		}
	})

	t.Run("manifest missing", func(t *testing.T) {
		path := rewrite(t, func(name string, body []byte) (string, []byte, bool) {
			return name, body, name != MemberManifest
		})
		if err := restore(path); !errors.Is(err, ErrCorruptArchive) {
			t.Fatalf("restore = %v", err)
		}
	})

	t.Run("future format", func(t *testing.T) {
		path := rewrite(t, func(name string, body []byte) (string, []byte, bool) {
			if name == MemberManifest {
				body = bytes.Replace(body, []byte(`"version": 1`), []byte(`"version": 99`), 1)
			}
			return name, body, true
		})
		if err := restore(path); err == nil || !strings.Contains(err.Error(), "newer") {
			t.Fatalf("restore = %v", err)
		}
	})
}

func TestScheduleAndPrune(t *testing.T) {
	ctx := context.Background()
	f := newFixture(t, nil)
	manual, err := f.svc.Create(ctx, "alice", "keep me")
	must(t, err)
	var scheduled []string
	for range 4 {
		info, err := f.svc.RunScheduled(ctx)
		must(t, err)
		scheduled = append(scheduled, info.ID)
		time.Sleep(2 * time.Millisecond)
	}
	list, err := f.svc.List(ctx)
	must(t, err)
	var ids []string
	for _, b := range list {
		ids = append(ids, b.ID)
	}
	want := []string{scheduled[3], scheduled[2], manual.ID}
	if !slices.Equal(ids, want) {
		t.Fatalf("after pruning (keep 2) = %v, want %v", ids, want)
	}

	// The ticker runs scheduled backups.
	f.svc.Configure(Options{Dir: f.svc.opts().Dir, Interval: 20 * time.Millisecond, Keep: 0})
	stop := f.svc.Start(ctx)
	deadline := time.Now().Add(10 * time.Second)
	for {
		st, err := f.svc.Status(ctx)
		must(t, err)
		if len(st.Backups) >= 5 {
			if st.NextRunAt == nil || st.Config.IntervalSeconds != 0 || st.Config.Interval != "20ms" {
				t.Fatalf("status = %+v", st.Config)
			}
			break
		}
		if time.Now().After(deadline) {
			t.Fatal("no scheduled backup within 10s")
		}
		time.Sleep(10 * time.Millisecond)
	}
	stop()
	if st, _ := f.svc.Status(ctx); st.NextRunAt != nil {
		t.Fatal("next_run_at set after stop")
	}
}

func TestErrorsAndUnsupported(t *testing.T) {
	ctx := context.Background()
	f := newFixture(t, nil)

	f.svc.run.Lock()
	if _, err := f.svc.Create(ctx, "admin", ""); !errors.Is(err, ErrRunning) {
		t.Fatalf("concurrent Create = %v", err)
	}
	if !f.svc.Running() {
		t.Fatal("Running = false while locked")
	}
	f.svc.run.Unlock()

	for _, id := range []string{"", "../meta", "bk-x", "bk-20260101T000000Z-zzzzzz"} {
		if _, err := f.svc.Get(ctx, id); !errors.Is(err, ErrNotFound) {
			t.Errorf("Get(%q) = %v", id, err)
		}
		if err := f.svc.Delete(ctx, id); !errors.Is(err, ErrNotFound) {
			t.Errorf("Delete(%q) = %v", id, err)
		}
	}

	mem := New(Deps{Store: meta.NewMemory(), Objects: objstore.New(t.TempDir()), Logger: quiet})
	mem.Configure(Options{Dir: t.TempDir(), Interval: time.Hour})
	if _, err := mem.Create(ctx, "admin", ""); !errors.Is(err, ErrUnsupported) {
		t.Fatalf("Create with a memory store = %v", err)
	}
	if mem.Supported() {
		t.Fatal("Supported = true for a memory store")
	}
	mem.Start(ctx)() // no-op start/stop

	// Unindexed archives in the directory are listed (restored servers).
	info, err := f.svc.Create(ctx, "admin", "")
	must(t, err)
	must(t, f.db.DeleteDoc(ctx, Collection, info.ID))
	must(t, os.WriteFile(filepath.Join(f.svc.opts().Dir, "notes.txt"), []byte("ignored"), 0o600))
	list, err := f.svc.List(ctx)
	must(t, err)
	if len(list) != 1 || list[0].ID != info.ID || list[0].SizeBytes != info.SizeBytes {
		t.Fatalf("list = %+v", list)
	}
	// Indexed backups whose file vanished are hidden.
	must(t, os.Remove(info.Path))
	must(t, f.db.PutDoc(ctx, Collection, info.ID, info))
	list, err = f.svc.List(ctx)
	must(t, err)
	if len(list) != 0 {
		t.Fatalf("list after removing the file = %+v", list)
	}
	fh, _, err := f.svc.Open(ctx, info.ID)
	if !errors.Is(err, ErrNotFound) {
		t.Fatalf("Open missing = %v, %v", fh, err)
	}
}

func TestObjectNames(t *testing.T) {
	h := hash.Sum([]byte("x"))
	got, ok := parseObjectName(objectName(h))
	if !ok || got != h {
		t.Fatalf("roundtrip failed: %v %v", got, ok)
	}
	for _, bad := range []string{"objects/zz/x", "objects/" + strings.ToUpper(h.Hex()[:2]) + "/" + h.Hex()[2:], "meta.db", "objects/" + h.Hex(), fmt.Sprintf("objects/%s/%s/x", h.Hex()[:2], h.Hex()[2:])} {
		if _, ok := parseObjectName(bad); ok {
			t.Errorf("parseObjectName(%q) accepted", bad)
		}
	}
}
