package main

import (
	"bytes"
	"context"
	"io"
	"log/slog"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/backup"
	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
	"github.com/seanfilimon/worktree/services/server-go/internal/objstore"
)

// makeBackup creates a server data dir with one tenant and object and
// returns a backup of it.
func makeBackup(t *testing.T) (string, hash.Hash) {
	t.Helper()
	ctx := context.Background()
	dir := t.TempDir()
	db, err := meta.NewBolt(filepath.Join(dir, "meta.db"))
	if err != nil {
		t.Fatal(err)
	}
	defer db.Close()
	if err := db.CreateTenant(ctx, &domain.Tenant{ID: "t1", Slug: "acme", Name: "Acme", Kind: domain.TenantOrg, Plan: "free", CreatedAt: time.Now()}); err != nil {
		t.Fatal(err)
	}
	objs := objstore.New(dir)
	data := bytes.Repeat([]byte("restore me "), 100)
	h := hash.Sum(data)
	if err := objs.Put(ctx, h, data); err != nil {
		t.Fatal(err)
	}
	svc := backup.New(backup.Deps{Store: db, Objects: objs, Logger: slog.New(slog.NewTextHandler(io.Discard, nil))})
	svc.Configure(backup.Options{Dir: filepath.Join(dir, "backups")})
	info, err := svc.Create(ctx, "admin", "")
	if err != nil {
		t.Fatal(err)
	}
	return info.Path, h
}

func TestRestoreSubcommand(t *testing.T) {
	file, h := makeBackup(t)
	target := filepath.Join(t.TempDir(), "data")

	var out, errb bytes.Buffer
	if code := run([]string{"restore", "--data", target, file}, &out, &errb); code != 0 {
		t.Fatalf("restore exit %d\nstdout: %s\nstderr: %s", code, out.String(), errb.String())
	}
	if !strings.Contains(out.String(), "restored 1 objects") || !strings.Contains(out.String(), "hashes verified") {
		t.Fatalf("stdout: %s", out.String())
	}
	db, err := meta.NewBolt(filepath.Join(target, "meta.db"))
	if err != nil {
		t.Fatal(err)
	}
	if _, err := db.GetTenantBySlug(context.Background(), "acme"); err != nil {
		t.Fatalf("restored tenant: %v", err)
	}
	_ = db.Close()
	if ok, _ := objstore.New(target).Has(context.Background(), h); !ok {
		t.Fatal("restored object missing")
	}

	// Non-empty data dir: refused (exit 1) unless --force.
	out.Reset()
	errb.Reset()
	if code := run([]string{"restore", "--data", target, file}, &out, &errb); code != 1 || !strings.Contains(errb.String(), "--force") {
		t.Fatalf("restore into non-empty dir: exit %d, stderr %s", code, errb.String())
	}
	if code := run([]string{"restore", file, "--data", target, "--force"}, &out, &errb); code != 0 {
		t.Fatalf("restore --force: exit %d, stderr %s", code, errb.String())
	}
}

func TestRestoreSubcommandErrors(t *testing.T) {
	var out, errb bytes.Buffer
	if code := run([]string{"restore", "-h"}, &out, &errb); code != 0 || !strings.Contains(out.String(), "Usage: worktree-server restore") {
		t.Fatalf("-h: exit %d, %s", code, out.String())
	}
	if code := run([]string{"restore", "--data", t.TempDir()}, &out, &errb); code != 2 {
		t.Fatalf("missing file: exit %d", code)
	}
	if code := run([]string{"restore", "--data", t.TempDir(), filepath.Join(t.TempDir(), "nope.wtbak")}, &out, &errb); code != 1 {
		t.Fatalf("missing archive: exit %d", code)
	}
	junk := filepath.Join(t.TempDir(), "junk.wtbak")
	if err := os.WriteFile(junk, []byte("junk"), 0o600); err != nil {
		t.Fatal(err)
	}
	errb.Reset()
	if code := run([]string{"restore", "--data", filepath.Join(t.TempDir(), "d"), junk}, &out, &errb); code != 1 || !strings.Contains(errb.String(), "corrupt") {
		t.Fatalf("junk archive: exit %d, %s", code, errb.String())
	}
}
