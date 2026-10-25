package config

import (
	"bytes"
	"errors"
	"flag"
	"strings"
	"testing"
	"time"
)

// Storage backends and backups (docs/ENGINEERING_PLAN.md §15.2).

func envOf(m map[string]string) lookupFunc {
	return func(k string) (string, bool) { v, ok := m[k]; return v, ok }
}

func TestBackendDefaults(t *testing.T) {
	cfg, err := load(nil, envOf(nil))
	if err != nil {
		t.Fatal(err)
	}
	if cfg.ObjectStore != ObjectStoreDisk || cfg.BackupInterval != 0 || cfg.BackupKeep != DefaultBackupKeep {
		t.Fatalf("defaults: store %q interval %s keep %d", cfg.ObjectStore, cfg.BackupInterval, cfg.BackupKeep)
	}
	if cfg.S3 != (S3Config{}) {
		t.Fatalf("S3 defaults: %+v", cfg.S3)
	}
}

func TestS3FromEnvAndFlags(t *testing.T) {
	env := envOf(map[string]string{
		"WT_OBJECT_STORE":    "s3",
		"WT_S3_ENDPOINT":     "minio:9000",
		"WT_S3_BUCKET":       "wt",
		"WT_S3_PREFIX":       "prod/",
		"WT_S3_REGION":       "eu-west-1",
		"WT_S3_ACCESS_KEY":   "AK",
		"WT_S3_SECRET_KEY":   "SK",
		"WT_S3_INSECURE":     "true",
		"WT_BACKUP_INTERVAL": "6h",
		"WT_BACKUP_KEEP":     "3",
	})
	cfg, err := load(nil, env)
	if err != nil {
		t.Fatal(err)
	}
	want := S3Config{Endpoint: "minio:9000", Bucket: "wt", Prefix: "prod/", Region: "eu-west-1", AccessKey: "AK", SecretKey: "SK", Insecure: true}
	if cfg.ObjectStore != "s3" || cfg.S3 != want || cfg.BackupInterval != 6*time.Hour || cfg.BackupKeep != 3 {
		t.Fatalf("env: %+v %+v %s %d", cfg.ObjectStore, cfg.S3, cfg.BackupInterval, cfg.BackupKeep)
	}
	// Flags override the environment.
	cfg, err = load([]string{"--s3-bucket", "other", "--s3-insecure=false", "--backup-keep", "0", "--backup-interval", "0"}, env)
	if err != nil {
		t.Fatal(err)
	}
	if cfg.S3.Bucket != "other" || cfg.S3.Insecure || cfg.BackupKeep != 0 || cfg.BackupInterval != 0 {
		t.Fatalf("flags: %+v keep %d", cfg.S3, cfg.BackupKeep)
	}
	cfg, err = load([]string{"--object-store", "disk"}, env)
	if err != nil || cfg.ObjectStore != "disk" {
		t.Fatalf("--object-store disk: %q %v", cfg.ObjectStore, err)
	}
}

func TestBackendValidation(t *testing.T) {
	for _, tc := range []struct {
		args []string
		env  map[string]string
		want string
	}{
		{[]string{"--object-store", "nfs"}, nil, `object-store "nfs"`},
		{[]string{"--object-store", "s3", "--s3-bucket", "b"}, nil, "requires --s3-endpoint"},
		{[]string{"--object-store", "s3", "--s3-endpoint", "h:1"}, nil, "requires --s3-bucket"},
		{[]string{"--object-store", "s3", "--s3-endpoint", "h:1", "--s3-bucket", "b", "--s3-access-key", "a"}, nil, "given together"},
		{[]string{"--backup-interval", "-1h"}, nil, "backup-interval must not be negative"},
		{[]string{"--backup-interval", "10s"}, nil, "at least 1m"},
		{[]string{"--backup-keep", "-2"}, nil, "backup-keep must not be negative"},
		{nil, map[string]string{"WT_BACKUP_INTERVAL": "soon"}, "WT_BACKUP_INTERVAL"},
		{nil, map[string]string{"WT_BACKUP_KEEP": "many"}, "WT_BACKUP_KEEP"},
		{nil, map[string]string{"WT_S3_INSECURE": "perhaps"}, "WT_S3_INSECURE"},
	} {
		_, err := load(tc.args, envOf(tc.env))
		if err == nil || !strings.Contains(err.Error(), tc.want) {
			t.Errorf("load(%v, %v) = %v, want error containing %q", tc.args, tc.env, err, tc.want)
		}
	}
	// Anonymous / env-credential S3 is valid.
	if _, err := load([]string{"--object-store", "s3", "--s3-endpoint", "h:1", "--s3-bucket", "b"}, envOf(nil)); err != nil {
		t.Errorf("s3 without static keys: %v", err)
	}
}

func TestLoadRestore(t *testing.T) {
	rc, err := loadRestore([]string{"--data", "/srv/wt", "backup.wtbak"}, envOf(nil))
	if err != nil {
		t.Fatal(err)
	}
	if rc.DataDir != "/srv/wt" || rc.File != "backup.wtbak" || rc.Force || rc.ObjectStore != "disk" || !rc.CompressObjects {
		t.Fatalf("%+v", rc)
	}
	// Flags after FILE, env data dir and S3 target.
	rc, err = loadRestore([]string{"b.wtbak", "--force", "--object-store=s3", "--s3-endpoint", "m:9000", "--s3-bucket", "wt"},
		envOf(map[string]string{"WT_DATA": "/env/data"}))
	if err != nil {
		t.Fatal(err)
	}
	if rc.DataDir != "/env/data" || rc.File != "b.wtbak" || !rc.Force || rc.ObjectStore != "s3" || rc.S3.Bucket != "wt" {
		t.Fatalf("%+v", rc)
	}
	for _, args := range [][]string{
		{"--data", "x"},
		{"a.wtbak", "b.wtbak"},
		{"--object-store", "s3", "a.wtbak"},
		{"--data", "", "a.wtbak"},
		{"--bogus", "a.wtbak"},
	} {
		if _, err := loadRestore(args, envOf(nil)); err == nil {
			t.Errorf("loadRestore(%v) succeeded", args)
		}
	}
	if _, err := loadRestore([]string{"-h"}, envOf(nil)); !errors.Is(err, flag.ErrHelp) {
		t.Errorf("-h = %v, want flag.ErrHelp", err)
	}
}

func TestUsageMentionsBackends(t *testing.T) {
	var b bytes.Buffer
	Usage(&b)
	for _, s := range []string{"-object-store", "-s3-endpoint", "-s3-secret-key", "-backup-interval", "-backup-keep", "worktree-server restore"} {
		if !strings.Contains(b.String(), s) {
			t.Errorf("usage lacks %q", s)
		}
	}
	b.Reset()
	RestoreUsage(&b)
	for _, s := range []string{"-force", "-data", "-object-store"} {
		if !strings.Contains(b.String(), s) {
			t.Errorf("restore usage lacks %q", s)
		}
	}
}
