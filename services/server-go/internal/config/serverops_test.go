package config

import (
	"bytes"
	"strings"
	"testing"
	"time"
)

// Server operations (docs/ENGINEERING_PLAN.md §16.11).

func TestServerOpsDefaults(t *testing.T) {
	cfg, err := load(nil, envOf(nil))
	if err != nil {
		t.Fatal(err)
	}
	if cfg.BackupTarget != BackupTargetDisk || cfg.BackupS3Prefix != "" || cfg.BackupMaxAge != 0 ||
		!cfg.PolicyVersionRetention.IsZero() || cfg.WebhookDeliveryRetention != 720*time.Hour ||
		cfg.WebhookDeliveryKeep != 200 || cfg.EmailDeadAfter != 24*time.Hour {
		t.Fatalf("defaults: %+v", cfg)
	}
}

func TestServerOpsEnvAndFlags(t *testing.T) {
	env := envOf(map[string]string{
		"WT_S3_ENDPOINT":                "minio:9000",
		"WT_S3_BUCKET":                  "wt",
		"WT_BACKUP_TARGET":              "s3",
		"WT_BACKUP_S3_PREFIX":           "nightly/",
		"WT_BACKUP_MAX_AGE":             "30d",
		"WT_POLICY_VERSION_RETENTION":   "90d,100",
		"WT_WEBHOOK_DELIVERY_RETENTION": "48h",
		"WT_WEBHOOK_DELIVERY_KEEP":      "50",
		"WT_EMAIL_DEAD_AFTER":           "2h",
	})
	cfg, err := load(nil, env)
	if err != nil {
		t.Fatal(err)
	}
	if cfg.BackupTarget != "s3" || cfg.BackupS3Prefix != "nightly/" || cfg.BackupMaxAge != 30*24*time.Hour ||
		cfg.PolicyVersionRetention != (Retention{MaxAge: 90 * 24 * time.Hour, Keep: 100}) ||
		cfg.WebhookDeliveryRetention != 48*time.Hour || cfg.WebhookDeliveryKeep != 50 || cfg.EmailDeadAfter != 2*time.Hour {
		t.Fatalf("env: %+v", cfg)
	}
	cfg, err = load([]string{"--backup-target", "disk", "--backup-max-age", "720h", "--policy-version-retention", "10",
		"--webhook-delivery-retention", "7d", "--webhook-delivery-keep", "5", "--email-dead-after", "90m"}, env)
	if err != nil {
		t.Fatal(err)
	}
	if cfg.BackupTarget != "disk" || cfg.BackupMaxAge != 720*time.Hour || cfg.PolicyVersionRetention != (Retention{Keep: 10}) ||
		cfg.WebhookDeliveryRetention != 7*24*time.Hour || cfg.WebhookDeliveryKeep != 5 || cfg.EmailDeadAfter != 90*time.Minute {
		t.Fatalf("flags: %+v", cfg)
	}
}

func TestServerOpsValidation(t *testing.T) {
	for _, tc := range []struct {
		args []string
		env  map[string]string
		want string
	}{
		{[]string{"--backup-target", "ftp"}, nil, `backup-target "ftp"`},
		{[]string{"--backup-target", "s3"}, nil, "backup-target s3 requires --s3-endpoint"},
		{[]string{"--backup-target", "s3", "--s3-endpoint", "x:9000"}, nil, "backup-target s3 requires --s3-bucket"},
		{[]string{"--webhook-delivery-keep", "0"}, nil, "webhook-delivery-keep must be at least 1"},
		{[]string{"--webhook-delivery-retention", "10s"}, nil, "webhook-delivery-retention must be at least 1m"},
		{[]string{"--email-dead-after", "10ms"}, nil, "email-dead-after must be at least 1s"},
		{[]string{"--backup-max-age", "30s"}, nil, "backup-max-age must be at least 1m"},
		{[]string{"--policy-version-retention", "1,2"}, nil, "invalid retention"},
		{[]string{"--backup-max-age", "soon"}, nil, "invalid age"},
		{nil, map[string]string{"WT_POLICY_VERSION_RETENTION": "x"}, "WT_POLICY_VERSION_RETENTION"},
		{nil, map[string]string{"WT_WEBHOOK_DELIVERY_KEEP": "x"}, "WT_WEBHOOK_DELIVERY_KEEP"},
		{nil, map[string]string{"WT_EMAIL_DEAD_AFTER": "-1h"}, "WT_EMAIL_DEAD_AFTER"},
		{[]string{"--object-store", "s3", "--backup-target", "s3", "--s3-endpoint", "x:9000", "--s3-bucket", "b",
			"--s3-prefix", "wt", "--backup-s3-prefix", "wt/objects/bk/"}, nil, "must not be inside the object store's objects/ prefix"},
	} {
		_, err := load(tc.args, envOf(tc.env))
		if err == nil || !strings.Contains(err.Error(), tc.want) {
			t.Errorf("load(%v, %v) = %v, want %q", tc.args, tc.env, err, tc.want)
		}
	}
	// Backups and objects can share a bucket under different prefixes.
	if _, err := load([]string{"--object-store", "s3", "--backup-target", "s3", "--s3-endpoint", "x:9000", "--s3-bucket", "b",
		"--s3-prefix", "wt"}, envOf(nil)); err != nil {
		t.Fatalf("shared bucket: %v", err)
	}
}

func TestParseAgeAndRetention(t *testing.T) {
	for in, want := range map[string]time.Duration{"0": 0, "90d": 90 * 24 * time.Hour, "1h30m": 90 * time.Minute, " 2d ": 48 * time.Hour} {
		if got, err := ParseAge(in); err != nil || got != want {
			t.Errorf("ParseAge(%q) = %s, %v", in, got, err)
		}
	}
	for _, bad := range []string{"", "d", "-1d", "1.5d", "-5m", "x"} {
		if _, err := ParseAge(bad); err == nil {
			t.Errorf("ParseAge(%q) accepted", bad)
		}
	}
	for in, want := range map[string]Retention{
		"": {}, "0": {}, "100": {Keep: 100}, "90d": {MaxAge: 90 * 24 * time.Hour},
		"2160h,100": {MaxAge: 2160 * time.Hour, Keep: 100}, "100, 30d": {MaxAge: 30 * 24 * time.Hour, Keep: 100},
	} {
		got, err := ParseRetention(in)
		if err != nil || got != want {
			t.Errorf("ParseRetention(%q) = %+v, %v", in, got, err)
		}
		if again, err := ParseRetention(got.String()); err != nil || again != got {
			t.Errorf("round trip of %q via %q = %+v, %v", in, got.String(), again, err)
		}
	}
	for _, bad := range []string{"1,2", "1d,2d", "1,2,3", "x", "-1"} {
		if _, err := ParseRetention(bad); err == nil {
			t.Errorf("ParseRetention(%q) accepted", bad)
		}
	}
}

func TestBackupS3KeyPrefix(t *testing.T) {
	for _, tc := range []struct{ s3Prefix, prefix, want string }{
		{"", "", "backups/"},
		{"wt", "", "wt/backups/"},
		{"/wt/", "", "wt/backups/"},
		{"wt/", "/nightly", "nightly/"},
	} {
		if got := BackupS3KeyPrefix(S3Config{Prefix: tc.s3Prefix}, tc.prefix); got != tc.want {
			t.Errorf("BackupS3KeyPrefix(%q, %q) = %q, want %q", tc.s3Prefix, tc.prefix, got, tc.want)
		}
	}
}

func TestRestoreListAndID(t *testing.T) {
	rc, err := loadRestore([]string{"--data", "/d", "--list", "--backup-target", "s3", "--s3-endpoint", "x:9000", "--s3-bucket", "b"}, envOf(nil))
	if err != nil || !rc.List || rc.BackupTarget != "s3" || rc.File != "" {
		t.Fatalf("--list = %+v, %v", rc, err)
	}
	rc, err = loadRestore([]string{"--data", "/d", "bk-20261026T000000Z-abcdef"}, envOf(map[string]string{"WT_BACKUP_S3_PREFIX": "p/"}))
	if err != nil || rc.File != "bk-20261026T000000Z-abcdef" || rc.BackupTarget != "disk" || rc.BackupS3Prefix != "p/" || rc.List {
		t.Fatalf("id = %+v, %v", rc, err)
	}
	if _, err := loadRestore([]string{"--list", "x.wtbak"}, envOf(nil)); err == nil || !strings.Contains(err.Error(), "--list takes no argument") {
		t.Fatalf("--list FILE = %v", err)
	}
	if _, err := loadRestore([]string{"--backup-target", "s3"}, envOf(nil)); err == nil || !strings.Contains(err.Error(), "FILE or ID") {
		t.Fatalf("no file = %v", err)
	}
	var b bytes.Buffer
	RestoreUsage(&b)
	for _, want := range []string{"-list", "-backup-target", "-backup-s3-prefix", "BACKUP-ID"} {
		if !strings.Contains(b.String(), want) {
			t.Errorf("restore usage lacks %q", want)
		}
	}
	b.Reset()
	Usage(&b)
	for _, want := range []string{"-backup-target", "-backup-max-age", "-policy-version-retention", "-webhook-delivery-retention",
		"-webhook-delivery-keep", "-email-dead-after"} {
		if !strings.Contains(b.String(), want) {
			t.Errorf("usage lacks %q", want)
		}
	}
}
