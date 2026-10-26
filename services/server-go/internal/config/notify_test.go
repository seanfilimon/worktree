package config

import (
	"strings"
	"testing"
)

// notifications (§15.3): --smtp-url / WT_SMTP_URL and --smtp-from / WT_SMTP_FROM.
func TestSMTPSettings(t *testing.T) {
	cfg, err := load(nil, env(nil))
	if err != nil || cfg.SMTPURL != "" || cfg.SMTPFrom != "" {
		t.Fatalf("default = %q %q, %v", cfg.SMTPURL, cfg.SMTPFrom, err)
	}
	cfg, err = load(nil, env(map[string]string{"WT_SMTP_URL": "smtp://u:p@mail.test:587", "WT_SMTP_FROM": "W0rkTree <wt@x.test>"}))
	if err != nil || cfg.SMTPURL != "smtp://u:p@mail.test:587" || cfg.SMTPFrom != "W0rkTree <wt@x.test>" {
		t.Fatalf("env = %q %q, %v", cfg.SMTPURL, cfg.SMTPFrom, err)
	}
	cfg, err = load([]string{"--smtp-url", "smtps://mail.test", "--smtp-from", "wt@x.test"}, env(map[string]string{"WT_SMTP_URL": "smtp://other"}))
	if err != nil || cfg.SMTPURL != "smtps://mail.test" || cfg.SMTPFrom != "wt@x.test" {
		t.Fatalf("flags = %q %q, %v", cfg.SMTPURL, cfg.SMTPFrom, err)
	}
	for _, args := range [][]string{
		{"--smtp-url", "http://mail.test"},
		{"--smtp-url", "smtp://"},
		{"--smtp-url", "smtp://mail.test/x"},
		{"--smtp-url", "smtp://mail.test", "--smtp-from", "not an address"},
		{"--smtp-from", "wt@x.test"},
	} {
		if _, err := load(args, env(nil)); err == nil {
			t.Errorf("load(%v) accepted", args)
		}
	}
	_, err = load([]string{"--smtp-url", "ftp://user:hunter2@mail.test"}, env(nil))
	if err == nil || strings.Contains(err.Error(), "hunter2") {
		t.Fatalf("invalid url error leaks the password or is missing: %v", err)
	}
}
