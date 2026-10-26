package config

// Server operations (docs/ENGINEERING_PLAN.md §16.11): backup targets
// (--backup-target disk|s3, --backup-s3-prefix, --backup-max-age),
// retention (--policy-version-retention, --webhook-delivery-retention,
// --webhook-delivery-keep; --audit-retention is the §13.3 flag) and the
// email outbox's dead-letter age (--email-dead-after).

import (
	"errors"
	"flag"
	"fmt"
	"strconv"
	"strings"
	"time"
)

// Backup targets (--backup-target).
const (
	BackupTargetDisk = "disk"
	BackupTargetS3   = "s3"
)

// Defaults of the §16.11 settings.
const (
	DefaultWebhookDeliveryRetention = 30 * 24 * time.Hour
	DefaultWebhookDeliveryKeep      = 200
	DefaultEmailDeadAfter           = 24 * time.Hour
)

// Retention is an age and/or count limit; zero values mean "no limit".
type Retention struct {
	MaxAge time.Duration
	Keep   int
}

// IsZero reports whether r keeps everything.
func (r Retention) IsZero() bool { return r.MaxAge <= 0 && r.Keep <= 0 }

// String formats r the way ParseRetention reads it ("" when zero).
func (r Retention) String() string {
	var parts []string
	if r.MaxAge > 0 {
		parts = append(parts, r.MaxAge.String())
	}
	if r.Keep > 0 {
		parts = append(parts, strconv.Itoa(r.Keep))
	}
	return strings.Join(parts, ",")
}

// ParseAge parses a Go duration or a whole number of days ("90d").
func ParseAge(v string) (time.Duration, error) {
	v = strings.TrimSpace(v)
	if days, ok := strings.CutSuffix(v, "d"); ok {
		n, err := strconv.Atoi(days)
		if err != nil || n < 0 {
			return 0, fmt.Errorf("invalid age %q: want a duration such as 720h or a number of days such as 30d", v)
		}
		return time.Duration(n) * 24 * time.Hour, nil
	}
	d, err := time.ParseDuration(v)
	if err != nil {
		return 0, fmt.Errorf("invalid age %q: want a duration such as 720h or a number of days such as 30d", v)
	}
	if d < 0 {
		return 0, fmt.Errorf("invalid age %q: must not be negative", v)
	}
	return d, nil
}

// ParseRetention parses "AGE", "COUNT" or "AGE,COUNT" (either order), e.g.
// "90d", "100", "2160h,100". "" and "0" keep everything.
func ParseRetention(v string) (Retention, error) {
	var r Retention
	v = strings.TrimSpace(v)
	if v == "" || v == "0" {
		return r, nil
	}
	parts := strings.Split(v, ",")
	if len(parts) > 2 {
		return r, fmt.Errorf("invalid retention %q: want AGE, COUNT or AGE,COUNT (e.g. 90d,100)", v)
	}
	seenAge, seenCount := false, false
	for _, p := range parts {
		p = strings.TrimSpace(p)
		if n, err := strconv.Atoi(p); err == nil {
			if seenCount || n < 0 {
				return Retention{}, fmt.Errorf("invalid retention %q: want AGE, COUNT or AGE,COUNT (e.g. 90d,100)", v)
			}
			seenCount, r.Keep = true, n
			continue
		}
		d, err := ParseAge(p)
		if err != nil || seenAge {
			return Retention{}, fmt.Errorf("invalid retention %q: want AGE, COUNT or AGE,COUNT (e.g. 90d,100)", v)
		}
		seenAge, r.MaxAge = true, d
	}
	return r, nil
}

// serverOpsFromEnv applies the §16.11 defaults and WT_* variables.
func serverOpsFromEnv(cfg *Config, lookup lookupFunc) []error {
	var errs []error
	cfg.BackupTarget = BackupTargetDisk
	cfg.WebhookDeliveryRetention = DefaultWebhookDeliveryRetention
	cfg.WebhookDeliveryKeep = DefaultWebhookDeliveryKeep
	cfg.EmailDeadAfter = DefaultEmailDeadAfter
	if v, ok := lookup("WT_BACKUP_TARGET"); ok && v != "" {
		cfg.BackupTarget = v
	}
	if v, ok := lookup("WT_BACKUP_S3_PREFIX"); ok && v != "" {
		cfg.BackupS3Prefix = v
	}
	age := func(key string, dst *time.Duration) {
		if v, ok := lookup(key); ok && v != "" {
			d, err := ParseAge(v)
			if err != nil {
				errs = append(errs, fmt.Errorf("%s: %v", key, err))
				return
			}
			*dst = d
		}
	}
	age("WT_BACKUP_MAX_AGE", &cfg.BackupMaxAge)
	age("WT_WEBHOOK_DELIVERY_RETENTION", &cfg.WebhookDeliveryRetention)
	age("WT_EMAIL_DEAD_AFTER", &cfg.EmailDeadAfter)
	if v, ok := lookup("WT_POLICY_VERSION_RETENTION"); ok && v != "" {
		r, err := ParseRetention(v)
		if err != nil {
			errs = append(errs, fmt.Errorf("WT_POLICY_VERSION_RETENTION: %v", err))
		}
		cfg.PolicyVersionRetention = r
	}
	if v, ok := lookup("WT_WEBHOOK_DELIVERY_KEEP"); ok && v != "" {
		n, err := strconv.Atoi(v)
		if err != nil {
			errs = append(errs, fmt.Errorf("WT_WEBHOOK_DELIVERY_KEEP: invalid integer %q", v))
		}
		cfg.WebhookDeliveryKeep = n
	}
	return errs
}

// ageFlag binds a ParseAge flag.
func ageFlag(fs *flag.FlagSet, dst *time.Duration, name, usage string) {
	fs.Func(name, usage, func(v string) error {
		d, err := ParseAge(v)
		if err != nil {
			return err
		}
		*dst = d
		return nil
	})
}

// backupTargetFlags binds --backup-target and --backup-s3-prefix (server
// and `worktree-server restore`).
func backupTargetFlags(fs *flag.FlagSet, cfg *Config) {
	fs.StringVar(&cfg.BackupTarget, "backup-target", cfg.BackupTarget, "where backups are kept: disk (<data>/backups) or s3 (the --s3-* bucket) (env WT_BACKUP_TARGET)")
	fs.StringVar(&cfg.BackupS3Prefix, "backup-s3-prefix", cfg.BackupS3Prefix, "key `prefix` of backups in the S3 bucket; default <s3-prefix>backups/ (env WT_BACKUP_S3_PREFIX)")
}

// serverOpsFlags binds every §16.11 server flag.
func serverOpsFlags(fs *flag.FlagSet, cfg *Config) {
	backupTargetFlags(fs, cfg)
	ageFlag(fs, &cfg.BackupMaxAge, "backup-max-age", "prune scheduled backups older than this `age` (e.g. 720h or 30d); 0 keeps them (env WT_BACKUP_MAX_AGE)")
	fs.Func("policy-version-retention", "prune policy versions: `AGE`, COUNT or AGE,COUNT per policy (e.g. 90d,100); the latest version is always kept; empty keeps all (env WT_POLICY_VERSION_RETENTION)", func(v string) error {
		r, err := ParseRetention(v)
		if err != nil {
			return err
		}
		cfg.PolicyVersionRetention = r
		return nil
	})
	ageFlag(fs, &cfg.WebhookDeliveryRetention, "webhook-delivery-retention", "delete webhook deliveries older than this `age` (default 720h; env WT_WEBHOOK_DELIVERY_RETENTION)")
	fs.IntVar(&cfg.WebhookDeliveryKeep, "webhook-delivery-keep", cfg.WebhookDeliveryKeep, "deliveries kept per webhook (env WT_WEBHOOK_DELIVERY_KEEP)")
	ageFlag(fs, &cfg.EmailDeadAfter, "email-dead-after", "dead-letter queued emails that still fail after this `age` (default 24h; env WT_EMAIL_DEAD_AFTER)")
}

// validateServerOps checks the §16.11 settings.
func (c *Config) validateServerOps() []error {
	var errs []error
	errs = append(errs, validateBackupTarget(c.BackupTarget, c.S3)...)
	if c.BackupTarget == BackupTargetS3 && c.ObjectStore == ObjectStoreS3 &&
		strings.HasPrefix(normPrefix(BackupS3KeyPrefix(c.S3, c.BackupS3Prefix)), normPrefix(c.S3.Prefix)+"objects/") {
		errs = append(errs, fmt.Errorf("backup-s3-prefix %q must not be inside the object store's objects/ prefix", c.BackupS3Prefix))
	}
	if c.WebhookDeliveryKeep < 1 {
		errs = append(errs, fmt.Errorf("webhook-delivery-keep must be at least 1, got %d", c.WebhookDeliveryKeep))
	}
	if c.WebhookDeliveryRetention > 0 && c.WebhookDeliveryRetention < time.Minute {
		errs = append(errs, fmt.Errorf("webhook-delivery-retention must be at least 1m, got %s", c.WebhookDeliveryRetention))
	}
	if c.EmailDeadAfter < time.Second {
		errs = append(errs, fmt.Errorf("email-dead-after must be at least 1s, got %s", c.EmailDeadAfter))
	}
	if c.BackupMaxAge > 0 && c.BackupMaxAge < time.Minute {
		errs = append(errs, fmt.Errorf("backup-max-age must be at least 1m, got %s", c.BackupMaxAge))
	}
	return errs
}

func validateBackupTarget(target string, s3 S3Config) []error {
	switch target {
	case BackupTargetDisk:
		return nil
	case BackupTargetS3:
		var errs []error
		if strings.TrimSpace(s3.Endpoint) == "" {
			errs = append(errs, errors.New("backup-target s3 requires --s3-endpoint (WT_S3_ENDPOINT)"))
		}
		if strings.TrimSpace(s3.Bucket) == "" {
			errs = append(errs, errors.New("backup-target s3 requires --s3-bucket (WT_S3_BUCKET)"))
		}
		if (s3.AccessKey == "") != (s3.SecretKey == "") {
			errs = append(errs, errors.New("s3-access-key and s3-secret-key must be given together"))
		}
		return errs
	default:
		return []error{fmt.Errorf("backup-target %q: want disk or s3", target)}
	}
}

// BackupS3KeyPrefix is the effective key prefix of S3 backups: the
// --backup-s3-prefix, or <s3-prefix>backups/.
func BackupS3KeyPrefix(s3 S3Config, prefix string) string {
	if strings.TrimSpace(prefix) != "" {
		return normPrefix(prefix)
	}
	return normPrefix(s3.Prefix) + "backups/"
}

// normPrefix trims leading slashes and adds a trailing one ("" stays "").
func normPrefix(p string) string {
	p = strings.TrimLeft(strings.TrimSpace(p), "/")
	if p != "" && !strings.HasSuffix(p, "/") {
		p += "/"
	}
	return p
}
