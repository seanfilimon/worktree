package config

import (
	"flag"
	"fmt"
	"time"
)

// Tenant lifecycle settings (tenant-lifecycle, docs/ENGINEERING_PLAN.md
// §16.4).

// Defaults of the tenant lifecycle.
const (
	DefaultTenantDeletionWindow = 30 * 24 * time.Hour
	DefaultTenantPurgeInterval  = time.Hour
)

func tenantsFromEnv(cfg *Config, lookup lookupFunc) []error {
	cfg.TenantDeletionWindow = DefaultTenantDeletionWindow
	cfg.TenantPurgeInterval = DefaultTenantPurgeInterval
	var errs []error
	for _, v := range []struct {
		key string
		dst *time.Duration
	}{{"WT_TENANT_DELETION_WINDOW", &cfg.TenantDeletionWindow}, {"WT_TENANT_PURGE_INTERVAL", &cfg.TenantPurgeInterval}} {
		if s, ok := lookup(v.key); ok && s != "" {
			d, err := time.ParseDuration(s)
			if err != nil {
				errs = append(errs, fmt.Errorf("%s: invalid duration %q", v.key, s))
				continue
			}
			*v.dst = d
		}
	}
	return errs
}

func tenantFlags(fs *flag.FlagSet, cfg *Config) {
	fs.DurationVar(&cfg.TenantDeletionWindow, "tenant-deletion-window", cfg.TenantDeletionWindow, "recovery `window` of a deleted tenant before it is purged (env WT_TENANT_DELETION_WINDOW)")
	fs.DurationVar(&cfg.TenantPurgeInterval, "tenant-purge-interval", cfg.TenantPurgeInterval, "how often deleted tenants past their recovery window are purged (env WT_TENANT_PURGE_INTERVAL)")
}

func (c *Config) validateTenants() []error {
	var errs []error // 0 selects the default
	if c.TenantDeletionWindow < 0 {
		errs = append(errs, fmt.Errorf("tenant-deletion-window must not be negative, got %s", c.TenantDeletionWindow))
	}
	if c.TenantPurgeInterval < 0 {
		errs = append(errs, fmt.Errorf("tenant-purge-interval must not be negative, got %s", c.TenantPurgeInterval))
	}
	return errs
}
