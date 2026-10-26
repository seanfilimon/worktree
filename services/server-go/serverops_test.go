package main

// server-ops (docs/ENGINEERING_PLAN.md §16.11): `worktree-server restore
// --list` and restoring a backup id from the disk or S3 backup target;
// `worktree-server fake-s3` usage.

import (
	"bytes"
	"context"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/seanfilimon/worktree/services/server-go/internal/backup"
	"github.com/seanfilimon/worktree/services/server-go/internal/objstore"
)

func TestRestoreByIDFromDiskTarget(t *testing.T) {
	file, h := makeBackup(t)
	id := strings.TrimSuffix(filepath.Base(file), backup.FileExt)
	data := filepath.Join(t.TempDir(), "data")
	in, err := os.ReadFile(file)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.MkdirAll(filepath.Join(data, "backups"), 0o750); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(data, "backups", id+backup.FileExt), in, 0o600); err != nil {
		t.Fatal(err)
	}

	var out, errb bytes.Buffer
	if code := run([]string{"restore", "--data", data, "--list"}, &out, &errb); code != 0 || !strings.Contains(out.String(), id) {
		t.Fatalf("--list: exit %d\n%s%s", code, out.String(), errb.String())
	}
	out.Reset()
	// The data directory only holds backups/: it counts as empty.
	if code := run([]string{"restore", "--data", data, id}, &out, &errb); code != 0 || !strings.Contains(out.String(), "hashes verified") {
		t.Fatalf("restore by id: exit %d\n%s%s", code, out.String(), errb.String())
	}
	if ok, _ := objstore.New(data).Has(context.Background(), h); !ok {
		t.Fatal("restored object missing")
	}
	errb.Reset()
	if code := run([]string{"restore", "--data", t.TempDir(), "bk-20200101T000000Z-abcdef"}, &out, &errb); code != 1 || !strings.Contains(errb.String(), "not found") {
		t.Fatalf("unknown id: exit %d, %s", code, errb.String())
	}
}

func TestRestoreByIDFromS3Target(t *testing.T) {
	file, h := makeBackup(t)
	id := strings.TrimSuffix(filepath.Base(file), backup.FileExt)
	in, err := os.ReadFile(file)
	if err != nil {
		t.Fatal(err)
	}
	fake := objstore.NewFakeS3()
	fake.SetObject("wt-backups", "prod/backups/"+id+backup.FileExt, in)
	srv := httptest.NewServer(fake)
	defer srv.Close()
	s3 := []string{"--backup-target", "s3", "--s3-endpoint", srv.URL, "--s3-bucket", "wt-backups", "--s3-prefix", "prod",
		"--s3-access-key", "a", "--s3-secret-key", "b", "--s3-region", "us-east-1"}
	data := filepath.Join(t.TempDir(), "data")

	var out, errb bytes.Buffer
	args := append([]string{"restore", "--data", data, "--list"}, s3...)
	if code := run(args, &out, &errb); code != 0 || !strings.Contains(out.String(), "s3://wt-backups/prod/backups/") || !strings.Contains(out.String(), id) {
		t.Fatalf("--list: exit %d\n%s%s", code, out.String(), errb.String())
	}
	out.Reset()
	// Objects go to disk (no --object-store s3) while the archive comes from the bucket.
	args = append(append([]string{"restore", "--data", data}, s3...), id)
	if code := run(args, &out, &errb); code != 0 || !strings.Contains(out.String(), "s3://wt-backups/prod/backups/"+id) {
		t.Fatalf("restore from s3: exit %d\n%s%s", code, out.String(), errb.String())
	}
	if ok, _ := objstore.New(data).Has(context.Background(), h); !ok {
		t.Fatal("restored object missing")
	}
}

func TestFakeS3Usage(t *testing.T) {
	var out, errb bytes.Buffer
	if code := run([]string{"fake-s3", "-h"}, &out, &errb); code != 0 || !strings.Contains(errb.String(), "-addr") {
		t.Fatalf("-h: exit %d, %s", code, errb.String())
	}
	if code := run([]string{"fake-s3", "extra"}, &out, &errb); code != 2 {
		t.Fatalf("extra argument: exit %d", code)
	}
	if code := run([]string{"fake-s3", "--addr", "256.0.0.1:1"}, &out, &errb); code != 1 {
		t.Fatalf("bad address: exit %d", code)
	}
}
