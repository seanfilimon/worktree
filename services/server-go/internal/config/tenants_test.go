package config

import (
	"testing"
	"time"
)

// tenant lifecycle (§16.4): --tenant-deletion-window / --tenant-purge-interval.
func TestTenantLifecycleSettings(t *testing.T) {
	cfg, err := load(nil, env(nil))
	if err != nil || cfg.TenantDeletionWindow != DefaultTenantDeletionWindow || cfg.TenantPurgeInterval != DefaultTenantPurgeInterval {
		t.Fatalf("defaults = %s %s, %v", cfg.TenantDeletionWindow, cfg.TenantPurgeInterval, err)
	}
	cfg, err = load(nil, env(map[string]string{"WT_TENANT_DELETION_WINDOW": "2s", "WT_TENANT_PURGE_INTERVAL": "1s"}))
	if err != nil || cfg.TenantDeletionWindow != 2*time.Second || cfg.TenantPurgeInterval != time.Second {
		t.Fatalf("env = %s %s, %v", cfg.TenantDeletionWindow, cfg.TenantPurgeInterval, err)
	}
	cfg, err = load([]string{"--tenant-deletion-window", "72h", "--tenant-purge-interval", "5m"}, env(map[string]string{"WT_TENANT_DELETION_WINDOW": "2s"}))
	if err != nil || cfg.TenantDeletionWindow != 72*time.Hour || cfg.TenantPurgeInterval != 5*time.Minute {
		t.Fatalf("flags = %s %s, %v", cfg.TenantDeletionWindow, cfg.TenantPurgeInterval, err)
	}
	for _, c := range []struct {
		args []string
		env  map[string]string
	}{
		{[]string{"--tenant-deletion-window", "-1h"}, nil},
		{[]string{"--tenant-purge-interval", "-1s"}, nil},
		{nil, map[string]string{"WT_TENANT_DELETION_WINDOW": "soon"}},
	} {
		if _, err := load(c.args, env(c.env)); err == nil {
			t.Errorf("load(%v, %v) accepted", c.args, c.env)
		}
	}
}
