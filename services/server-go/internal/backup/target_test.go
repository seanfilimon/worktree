package backup

// server-ops (docs/ENGINEERING_PLAN.md §16.11): backups kept in an S3
// bucket (against the in-process fake S3), retention by age and count, and
// restoring from a streamed archive.

import (
	"bytes"
	"context"
	"errors"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/objstore"
)

func newS3Target(t *testing.T, fake *objstore.FakeS3, bucket, prefix string) *S3Target {
	t.Helper()
	srv := httptest.NewServer(fake)
	t.Cleanup(srv.Close)
	b, err := objstore.NewS3Bucket(context.Background(), objstore.S3Options{Endpoint: srv.URL, Bucket: bucket, Region: "us-east-1",
		AccessKey: "a", SecretKey: "b", MaxRetries: 1})
	must(t, err)
	if b.Bucket != bucket || b.Secure {
		t.Fatalf("bucket = %+v", b)
	}
	return NewS3Target(b, prefix)
}

func TestS3Target(t *testing.T) {
	ctx := context.Background()
	fake := objstore.NewFakeS3()
	target := newS3Target(t, fake, "wt-backups", "/prod/backups")
	if target.Prefix() != "prod/backups/" || target.Location() != "s3://wt-backups/prod/backups/" {
		t.Fatalf("prefix %q location %q", target.Prefix(), target.Location())
	}
	f := newFixture(t, nil)
	f.svc.Configure(Options{Dir: filepath.Join(f.dataDir, "backups"), Target: target, Keep: 1, Version: "test"})

	info, err := f.svc.Create(ctx, "admin", "to s3")
	must(t, err)
	if info.Path != "s3://wt-backups/prod/backups/"+info.ID+FileExt || info.SizeBytes <= 0 || info.Objects != int64(len(f.hashes)) {
		t.Fatalf("info = %+v", info)
	}
	// Nothing stays behind locally: the archive was uploaded.
	if entries, _ := os.ReadDir(filepath.Join(f.dataDir, "backups")); len(entries) != 0 {
		t.Fatalf("local staging left %v", entries)
	}
	data, ok := fake.Object("wt-backups", "prod/backups/"+info.ID+FileExt)
	if !ok || int64(len(data)) != info.SizeBytes || !bytes.HasPrefix(data, []byte{0x28, 0xb5, 0x2f, 0xfd}) {
		t.Fatalf("uploaded archive: %d bytes, found %v", len(data), ok)
	}

	st, err := f.svc.Status(ctx)
	must(t, err)
	if st.Config.Target != TargetS3 || st.Config.Location != target.Location() || len(st.Backups) != 1 || st.Backups[0].Note != "to s3" {
		t.Fatalf("status = %+v", st)
	}

	// Download and restore from the streamed object.
	rc, got, err := f.svc.Open(ctx, info.ID)
	must(t, err)
	if _, isFile := rc.(*os.File); isFile || got.SizeBytes != info.SizeBytes {
		t.Fatalf("open = %T %+v", rc, got)
	}
	dest := t.TempDir()
	rep, err := Restore(ctx, RestoreOptions{Source: rc, DataDir: dest, Objects: objstore.New(dest)})
	rc.Close()
	must(t, err)
	if rep.Manifest.ID != info.ID {
		t.Fatalf("restored %+v", rep.Manifest)
	}
	f.verifyRestored(t, dest, objstore.New(dest))

	// Unindexed archives in the bucket are listed; foreign keys are not.
	fake.SetObject("wt-backups", "prod/backups/bk-20200101T000000Z-abcdef"+FileExt, []byte("old"))
	fake.SetObject("wt-backups", "prod/backups/notes.txt", []byte("x"))
	fake.SetObject("wt-backups", "other/bk-20200101T000000Z-000000"+FileExt, []byte("x"))
	list, err := f.svc.List(ctx)
	must(t, err)
	if len(list) != 2 || list[0].ID != info.ID || list[1].ID != "bk-20200101T000000Z-abcdef" ||
		!list[1].CreatedAt.Equal(time.Date(2020, 1, 1, 0, 0, 0, 0, time.UTC)) || list[1].SizeBytes != 3 {
		t.Fatalf("list = %+v", list)
	}

	// Delete removes the object; a second delete is not found.
	must(t, f.svc.Delete(ctx, info.ID))
	if _, ok := fake.Object("wt-backups", "prod/backups/"+info.ID+FileExt); ok {
		t.Fatal("object not deleted")
	}
	if err := f.svc.Delete(ctx, info.ID); !errors.Is(err, ErrNotFound) {
		t.Fatalf("second delete = %v", err)
	}
	if _, _, err := target.Open(ctx, info.ID); !errors.Is(err, ErrNotFound) {
		t.Fatalf("open deleted = %v", err)
	}
	if _, _, err := target.Open(ctx, "../x"); !errors.Is(err, ErrNotFound) {
		t.Fatalf("open invalid = %v", err)
	}

	// A failing upload leaves neither an index entry nor a local file.
	fake.FailNext(100)
	if _, err := f.svc.Create(ctx, "admin", ""); err == nil || !strings.Contains(err.Error(), "upload") {
		t.Fatalf("create with failing S3 = %v", err)
	}
	fake.FailNext(0)
	if entries, _ := os.ReadDir(filepath.Join(f.dataDir, "backups")); len(entries) != 0 {
		t.Fatalf("local staging left %v after a failed upload", entries)
	}
	raws, err := f.db.ListDocs(ctx, Collection, "")
	must(t, err)
	if len(raws) != 0 {
		t.Fatalf("index has %d entries after a failed upload", len(raws))
	}
}

func TestPruneByAgeAndCount(t *testing.T) {
	ctx := context.Background()
	f := newFixture(t, nil)
	now := time.Date(2026, 10, 26, 12, 0, 0, 0, time.UTC)
	f.svc.now = func() time.Time { return now }
	f.svc.Configure(Options{Dir: filepath.Join(f.dataDir, "backups"), Keep: 3, MaxAge: 48 * time.Hour})

	put := func(id, by string) {
		t.Helper()
		created, _ := IDTime(id)
		must(t, os.MkdirAll(filepath.Join(f.dataDir, "backups"), 0o750))
		must(t, os.WriteFile(filepath.Join(f.dataDir, "backups", id+FileExt), []byte("x"), 0o600))
		must(t, f.db.PutDoc(ctx, Collection, id, domain.BackupInfo{ID: id, CreatedAt: created, CreatedBy: by}))
	}
	put("bk-20261026T110000Z-000001", ScheduledBy) // 1 h old
	put("bk-20261026T100000Z-000002", ScheduledBy) // 2 h
	put("bk-20261025T100000Z-000003", ScheduledBy) // 26 h
	put("bk-20261020T100000Z-000004", ScheduledBy) // 6 days: too old
	put("bk-20261019T100000Z-000005", "admin")     // manual: never pruned
	pruned, err := f.svc.PruneAudited(ctx)
	must(t, err)
	if len(pruned) != 1 || pruned[0] != "bk-20261020T100000Z-000004" {
		t.Fatalf("pruned by age = %v", pruned)
	}
	f.svc.Configure(Options{Dir: filepath.Join(f.dataDir, "backups"), Keep: 2})
	pruned, err = f.svc.Prune(ctx)
	must(t, err)
	if len(pruned) != 1 || pruned[0] != "bk-20261025T100000Z-000003" {
		t.Fatalf("pruned by count = %v", pruned)
	}
	list, err := f.svc.List(ctx)
	must(t, err)
	if len(list) != 3 || list[2].CreatedBy != "admin" {
		t.Fatalf("left = %+v", list)
	}
	if cfg := f.svc.Describe(); cfg.Target != TargetDisk || cfg.MaxAge != "0s" || cfg.Keep != 2 {
		t.Fatalf("describe = %+v", cfg)
	}
}

func TestDiskTargetAndRestoreIntoDirWithBackups(t *testing.T) {
	ctx := context.Background()
	f := newFixture(t, nil)
	info, err := f.svc.Create(ctx, "admin", "")
	must(t, err)

	// A data directory that only holds backups/ counts as empty.
	data := t.TempDir()
	dt := NewDiskTarget(filepath.Join(data, "backups"))
	must(t, os.MkdirAll(dt.Dir, 0o750))
	in, err := os.ReadFile(info.Path)
	must(t, err)
	must(t, os.WriteFile(filepath.Join(dt.Dir, info.ID+FileExt+".tmp"), in, 0o600))
	must(t, dt.Store(ctx, info.ID, filepath.Join(dt.Dir, info.ID+FileExt+".tmp")))
	objs, err := dt.List(ctx)
	must(t, err)
	if len(objs) != 1 || objs[0].ID != info.ID || objs[0].Size != int64(len(in)) {
		t.Fatalf("list = %+v", objs)
	}
	r, obj, err := dt.Open(ctx, info.ID)
	must(t, err)
	if obj.Size != int64(len(in)) {
		t.Fatalf("open size %d", obj.Size)
	}
	_, err = Restore(ctx, RestoreOptions{Source: r, DataDir: data, Objects: objstore.New(data)})
	r.Close()
	must(t, err)
	f.verifyRestored(t, data, objstore.New(data))
	// meta.db now exists: a second restore needs --force.
	r2, _, err := dt.Open(ctx, info.ID)
	must(t, err)
	_, err = Restore(ctx, RestoreOptions{Source: r2, DataDir: data, Objects: objstore.New(data)})
	r2.Close()
	if !errors.Is(err, ErrDataDirNotEmpty) {
		t.Fatalf("restore over data = %v", err)
	}
	must(t, dt.Delete(ctx, info.ID))
	if err := dt.Delete(ctx, info.ID); !errors.Is(err, ErrNotFound) {
		t.Fatalf("second delete = %v", err)
	}
	if objs, err := NewDiskTarget(filepath.Join(data, "missing")).List(ctx); err != nil || len(objs) != 0 {
		t.Fatalf("missing dir list = %v %v", objs, err)
	}
}

func TestDefaultS3Prefix(t *testing.T) {
	for in, want := range map[string]string{"": "backups/", "wt": "wt/backups/", "/wt/": "wt/backups/"} {
		if got := DefaultS3Prefix(in); got != want {
			t.Errorf("DefaultS3Prefix(%q) = %q, want %q", in, got, want)
		}
	}
}
