// Package config loads the worktree-server configuration from command-line
// flags and WT_* environment variables.
//
// Precedence is: explicit flag > environment variable > built-in default.
package config

import (
	"errors"
	"flag"
	"fmt"
	"io"
	"log/slog"
	"net"
	"net/mail"
	"net/netip"
	"net/url"
	"os"
	"regexp"
	"strconv"
	"strings"
	"time"
)

// Defaults.
const (
	DefaultAddr            = ":7420"
	DefaultDataDir         = "./wt-data"
	DefaultAdminUser       = "admin"
	DefaultShutdownTimeout = 15 * time.Second

	// Operations defaults (§13.3). staged-v2 (§16.8): staged TTL 30 days.
	DefaultGCInterval      = 24 * time.Hour
	DefaultGCGrace         = 720 * time.Hour
	DefaultStagedRetention = 720 * time.Hour
	DefaultRateLimit       = 1200 // requests per minute per principal / IP
)

// Config is the complete server configuration.
type Config struct {
	// Addr is the TCP listen address for gRPC (h2c) and HTTP. Flag --addr,
	// env WT_ADDR.
	Addr string
	// DataDir holds meta.db and objects/. Flag --data, env WT_DATA.
	DataDir string
	// AdminUser is the server admin account name. Flag --admin-user, env
	// WT_ADMIN_USER.
	AdminUser string
	// AdminPassword, when set, ensures AdminUser exists with this password
	// and server-admin rights. Flag --admin-password, env WT_ADMIN_PASSWORD.
	AdminPassword string
	// AdminDir is the static admin SPA build served at /admin/. Flag
	// --admin-dir, env WT_ADMIN_DIR.
	AdminDir string
	// AllowRegistration enables self-service account registration. Flag
	// --allow-registration, env WT_ALLOW_REGISTRATION.
	AllowRegistration bool
	// LogLevel is the minimum log level. Flag --log-level, env WT_LOG_LEVEL.
	LogLevel slog.Level
	// LogFormat is "text" or "json". Flag --log-format, env WT_LOG_FORMAT.
	LogFormat string
	// CORSOrigins are extra browser origins allowed to call the REST API.
	// Flag --cors-origins, env WT_CORS_ORIGINS (comma-separated).
	CORSOrigins []string
	// TrustedProxies are reverse-proxy addresses (IPs or CIDRs) whose
	// X-Forwarded-For header names the real client (IP policy conditions,
	// rate limits, audit). Flag --trusted-proxies, env WT_TRUSTED_PROXIES
	// (comma-separated). Empty = the socket peer is the client.
	TrustedProxies []string
	// ShutdownTimeout bounds graceful shutdown. Flag --shutdown-timeout, env
	// WT_SHUTDOWN_TIMEOUT.
	ShutdownTimeout time.Duration
	// PublicURL is the externally visible base URL of the server (e.g.
	// https://wt.example.com), used for the default SSO redirect URL. Flag
	// --public-url, env WT_PUBLIC_URL.
	PublicURL string
	// ShowVersion requests printing the version and exiting (--version).
	ShowVersion bool

	// ---- operations (§13.3) ----

	// TLSCert and TLSKey are PEM files; when both are set the server
	// serves HTTPS and gRPC over TLS (ALPN h2, http/1.1). Flags
	// --tls-cert/--tls-key, env WT_TLS_CERT/WT_TLS_KEY.
	TLSCert string
	TLSKey  string
	// TLSClientCA is a PEM bundle of CAs; when set, clients must present a
	// certificate signed by one of them (mTLS). Flag --tls-client-ca, env
	// WT_TLS_CLIENT_CA.
	TLSClientCA string
	// GCInterval is the period of automatic garbage collection (0
	// disables). Flag --gc-interval, env WT_GC_INTERVAL.
	GCInterval time.Duration
	// GCGrace is how long an unreachable object stays marked before it is
	// deleted. Flag --gc-grace, env WT_GC_GRACE.
	GCGrace time.Duration
	// StagedRetention expires staged snapshots older than this (0 keeps
	// them). Flags --staged-ttl / --staged-retention, env WT_STAGED_TTL /
	// WT_STAGED_RETENTION (staged-v2, §16.8: default 30 days).
	StagedRetention time.Duration
	// AuditRetention deletes audit events older than this (0 keeps them).
	// Flag --audit-retention, env WT_AUDIT_RETENTION.
	AuditRetention time.Duration
	// RateLimit is the request budget per minute per principal (or
	// anonymous IP); 0 disables. Flag --rate-limit, env WT_RATE_LIMIT.
	RateLimit int
	// DefaultMaxTrees and DefaultMaxStorage are the server-wide tenant
	// quotas (0 = unlimited) that tenant limits override. Flags
	// --default-max-trees/--default-max-storage, env
	// WT_DEFAULT_MAX_TREES/WT_DEFAULT_MAX_STORAGE.
	DefaultMaxTrees   int
	DefaultMaxStorage int64

	// ---- webhooks (§14.2) ----

	// WebhookAllowPrivate permits webhook targets resolving to loopback,
	// link-local or private addresses (turns the SSRF guard off). Flag
	// --webhook-allow-private, env WT_WEBHOOK_ALLOW_PRIVATE.
	WebhookAllowPrivate bool

	// ---- mirrors (§15.6) ----

	// GitBinary is the git executable Git mirrors run (a name looked up on
	// PATH or a path); mirrors are disabled when it is missing. Flag
	// --git-binary, env WT_GIT_BINARY (default "git").
	GitBinary string
	// MirrorAllowLocal permits file:// mirror remotes and mirror hosts on
	// loopback, link-local or private addresses. Flag --mirror-allow-local,
	// env WT_MIRROR_ALLOW_LOCAL.
	MirrorAllowLocal bool

	// ---- end mirrors ----

	// ---- storage (§14.6) ----

	// CompressObjects stores new objects zstd-compressed (WTZ1) when that
	// saves at least 10 %; existing objects are read either way. Flag
	// --compress-objects, env WT_COMPRESS_OBJECTS (default true).
	CompressObjects bool

	// ---- storage backends and backups (§15.2, backends.go) ----

	// ObjectStore selects the object store: "disk" (default, objects/
	// under DataDir) or "s3". Flag --object-store, env WT_OBJECT_STORE.
	ObjectStore string
	// S3 holds the --s3-* settings used when ObjectStore is "s3".
	S3 S3Config
	// BackupInterval is the period of automatic backups (0 = off). Flag
	// --backup-interval, env WT_BACKUP_INTERVAL.
	BackupInterval time.Duration
	// BackupKeep is how many scheduled backups are kept (0 keeps all).
	// Flag --backup-keep, env WT_BACKUP_KEEP (default 7).
	BackupKeep int

	// ---- end storage backends ----
	// ---- notifications (§15.3) ----

	// SMTPURL enables notification emails: smtp://[user:pass@]host[:port]
	// (STARTTLS when offered) or smtps://... (implicit TLS). Flag
	// --smtp-url, env WT_SMTP_URL (prefer the env var: it holds a password).
	SMTPURL string
	// SMTPFrom is the From address of notification emails. Flag
	// --smtp-from, env WT_SMTP_FROM.
	SMTPFrom string
	// ---- end notifications ----
	// ---- code search v2 (§15.9) ----

	// CodeIndexMemory caps the decoded code search index segments kept in
	// memory (bytes; 0 = unlimited); the index itself lives under
	// <data>/codeindex. Flag --code-index-memory (e.g. 256MiB, 1G), env
	// WT_CODE_INDEX_MEMORY (default 256 MiB).
	CodeIndexMemory int64
	// CodeIndexBranches is the most branches indexed per tree for
	// branches=all searches. Flag --code-index-branches, env
	// WT_CODE_INDEX_BRANCHES (default 10).
	CodeIndexBranches int
	// ---- end code search v2 ----
	// ---- tenant lifecycle (§16.4) ----

	// TenantDeletionWindow is the recovery window of a deleted tenant
	// (default 30 days). Flag --tenant-deletion-window, env
	// WT_TENANT_DELETION_WINDOW.
	TenantDeletionWindow time.Duration
	// TenantPurgeInterval is the period of the tenant purge job (default
	// 1h). Flag --tenant-purge-interval, env WT_TENANT_PURGE_INTERVAL.
	TenantPurgeInterval time.Duration
	// ---- end tenant lifecycle ----
	// pack-delta (§16.1, packs.go): --disable-packs, --max-pack-size.
	Packs PackConfig
	// hardening (§15.10)

	// Pprof serves the Go runtime profiles at /debug/pprof (server admins
	// only; 404 when off). Flag --pprof, env WT_PPROF (default false).
	Pprof bool

	// hardening (§15.10): for tests
	//
	// WebhookRetrySchedule replaces the webhook retry delays (1m, 5m, 30m,
	// 2h) so e2e tests can observe retries; nil = the defaults. Hidden
	// testing flag --webhook-retry-schedule, env WT_WEBHOOK_RETRY_SCHEDULE
	// (comma-separated durations, e.g. "1s,2s").
	WebhookRetrySchedule []time.Duration
	// end hardening (§15.10): for tests

	// The fields below are set programmatically, not from flags.

	// Version is the server version reported by /healthz, Ping and logs.
	Version string
	// Stdout receives the plain startup lines (nil = os.Stdout).
	Stdout io.Writer
	// LogOutput receives structured logs (nil = os.Stderr).
	LogOutput io.Writer
	// OnListen, if set, is called with the bound address once the server
	// is accepting connections.
	OnListen func(addr net.Addr)
}

// Load parses args (without the program name) and the process environment.
// It returns flag.ErrHelp when -h/--help was requested.
func Load(args []string) (Config, error) {
	return load(args, os.LookupEnv)
}

type lookupFunc func(string) (string, bool)

func load(args []string, lookup lookupFunc) (Config, error) {
	cfg, err := fromEnv(lookup)
	if err != nil {
		return Config{}, err
	}
	fs := newFlagSet(&cfg, io.Discard)
	if err := fs.Parse(args); err != nil {
		return Config{}, err
	}
	if fs.NArg() > 0 {
		return Config{}, fmt.Errorf("unexpected argument %q", fs.Arg(0))
	}
	if cfg.ShowVersion {
		return cfg, nil
	}
	if err := cfg.Validate(); err != nil {
		return Config{}, err
	}
	return cfg, nil
}

// fromEnv returns the defaults overlaid with WT_* environment variables.
func fromEnv(lookup lookupFunc) (Config, error) {
	cfg := Config{
		Addr:              DefaultAddr,
		DataDir:           DefaultDataDir,
		AdminUser:         DefaultAdminUser,
		AllowRegistration: true,
		LogLevel:          slog.LevelInfo,
		LogFormat:         "text",
		ShutdownTimeout:   DefaultShutdownTimeout,
		GCInterval:        DefaultGCInterval,
		GCGrace:           DefaultGCGrace,
		StagedRetention:   DefaultStagedRetention,
		RateLimit:         DefaultRateLimit,
	}
	str := func(key string, dst *string) {
		if v, ok := lookup(key); ok && v != "" {
			*dst = v
		}
	}
	str("WT_ADDR", &cfg.Addr)
	str("WT_DATA", &cfg.DataDir)
	str("WT_ADMIN_USER", &cfg.AdminUser)
	str("WT_ADMIN_PASSWORD", &cfg.AdminPassword)
	str("WT_ADMIN_DIR", &cfg.AdminDir)
	str("WT_LOG_FORMAT", &cfg.LogFormat)
	str("WT_PUBLIC_URL", &cfg.PublicURL)

	var errs []error
	if v, ok := lookup("WT_ALLOW_REGISTRATION"); ok && v != "" {
		b, err := strconv.ParseBool(v)
		if err != nil {
			errs = append(errs, fmt.Errorf("WT_ALLOW_REGISTRATION: invalid boolean %q", v))
		}
		cfg.AllowRegistration = b
	}
	if v, ok := lookup("WT_LOG_LEVEL"); ok && v != "" {
		if err := cfg.LogLevel.UnmarshalText([]byte(v)); err != nil {
			errs = append(errs, fmt.Errorf("WT_LOG_LEVEL: invalid level %q (want debug, info, warn or error)", v))
		}
	}
	if v, ok := lookup("WT_SHUTDOWN_TIMEOUT"); ok && v != "" {
		d, err := time.ParseDuration(v)
		if err != nil {
			errs = append(errs, fmt.Errorf("WT_SHUTDOWN_TIMEOUT: invalid duration %q", v))
		}
		cfg.ShutdownTimeout = d
	}
	if v, ok := lookup("WT_TRUSTED_PROXIES"); ok {
		cfg.TrustedProxies = splitList(v)
	}
	if v, ok := lookup("WT_CORS_ORIGINS"); ok {
		cfg.CORSOrigins = splitList(v)
	}

	// Operations (§13.3).
	str("WT_TLS_CERT", &cfg.TLSCert)
	str("WT_TLS_KEY", &cfg.TLSKey)
	str("WT_TLS_CLIENT_CA", &cfg.TLSClientCA)
	dur := func(key string, dst *time.Duration) {
		if v, ok := lookup(key); ok && v != "" {
			d, err := time.ParseDuration(v)
			if err != nil {
				errs = append(errs, fmt.Errorf("%s: invalid duration %q", key, v))
				return
			}
			*dst = d
		}
	}
	dur("WT_GC_INTERVAL", &cfg.GCInterval)
	dur("WT_GC_GRACE", &cfg.GCGrace)
	dur("WT_STAGED_RETENTION", &cfg.StagedRetention)
	dur("WT_STAGED_TTL", &cfg.StagedRetention) // staged-v2 (§16.8): preferred name
	dur("WT_AUDIT_RETENTION", &cfg.AuditRetention)
	integer := func(key string, dst *int64) {
		if v, ok := lookup(key); ok && v != "" {
			n, err := strconv.ParseInt(v, 10, 64)
			if err != nil {
				errs = append(errs, fmt.Errorf("%s: invalid integer %q", key, v))
				return
			}
			*dst = n
		}
	}
	var rate, maxTrees = int64(cfg.RateLimit), int64(cfg.DefaultMaxTrees)
	integer("WT_RATE_LIMIT", &rate)
	integer("WT_DEFAULT_MAX_TREES", &maxTrees)
	integer("WT_DEFAULT_MAX_STORAGE", &cfg.DefaultMaxStorage)
	cfg.RateLimit, cfg.DefaultMaxTrees = int(rate), int(maxTrees)

	// webhooks (§14.2).
	if v, ok := lookup("WT_WEBHOOK_ALLOW_PRIVATE"); ok && v != "" {
		b, err := strconv.ParseBool(v)
		if err != nil {
			errs = append(errs, fmt.Errorf("WT_WEBHOOK_ALLOW_PRIVATE: invalid boolean %q", v))
		}
		cfg.WebhookAllowPrivate = b
	}

	// ---- mirrors (§15.6) ----
	cfg.GitBinary = "git"
	str("WT_GIT_BINARY", &cfg.GitBinary)
	if v, ok := lookup("WT_MIRROR_ALLOW_LOCAL"); ok && v != "" {
		b, err := strconv.ParseBool(v)
		if err != nil {
			errs = append(errs, fmt.Errorf("WT_MIRROR_ALLOW_LOCAL: invalid boolean %q", v))
		}
		cfg.MirrorAllowLocal = b
	}
	// ---- end mirrors ----

	// ---- storage (§14.6) ----
	cfg.CompressObjects = true
	if v, ok := lookup("WT_COMPRESS_OBJECTS"); ok && v != "" {
		b, err := strconv.ParseBool(v)
		if err != nil {
			errs = append(errs, fmt.Errorf("WT_COMPRESS_OBJECTS: invalid boolean %q", v))
		} else {
			cfg.CompressObjects = b
		}
	}
	// ---- end storage ----
	errs = append(errs, backendsFromEnv(&cfg, lookup)...) // storage backends (§15.2)

	// ---- notifications (§15.3) ----
	str("WT_SMTP_URL", &cfg.SMTPURL)
	str("WT_SMTP_FROM", &cfg.SMTPFrom)
	// ---- end notifications ----

	// ---- code search v2 (§15.9) ----
	errs = append(errs, codeIndexFromEnv(&cfg, lookup)...)
	errs = append(errs, packsFromEnv(&cfg, lookup)...) // pack-delta (§16.1)
	// ---- end code search v2 ----
	errs = append(errs, tenantsFromEnv(&cfg, lookup)...) // tenant lifecycle (§16.4)

	// hardening (§15.10)
	if v, ok := lookup("WT_PPROF"); ok && v != "" {
		b, err := strconv.ParseBool(v)
		if err != nil {
			errs = append(errs, fmt.Errorf("WT_PPROF: invalid boolean %q", v))
		}
		cfg.Pprof = b
	}

	// hardening (§15.10): for tests
	if v, ok := lookup("WT_WEBHOOK_RETRY_SCHEDULE"); ok && v != "" {
		s, err := ParseRetrySchedule(v)
		if err != nil {
			errs = append(errs, fmt.Errorf("WT_WEBHOOK_RETRY_SCHEDULE: %w", err))
		}
		cfg.WebhookRetrySchedule = s
	}
	// end hardening (§15.10): for tests
	return cfg, errors.Join(errs...)
}

// newFlagSet binds flags to cfg; the current cfg values are the defaults
// so that environment values apply unless a flag is given explicitly.
func newFlagSet(cfg *Config, out io.Writer) *flag.FlagSet {
	fs := flag.NewFlagSet("worktree-server", flag.ContinueOnError)
	fs.SetOutput(out)
	fs.StringVar(&cfg.Addr, "addr", cfg.Addr, "`host:port` to listen on for gRPC (h2c) and HTTP (env WT_ADDR)")
	fs.StringVar(&cfg.DataDir, "data", cfg.DataDir, "data `directory` holding meta.db and objects/ (env WT_DATA)")
	fs.StringVar(&cfg.AdminUser, "admin-user", cfg.AdminUser, "server admin `username` (env WT_ADMIN_USER)")
	fs.StringVar(&cfg.AdminPassword, "admin-password", cfg.AdminPassword, "if set, ensure the admin account exists with this `password` (env WT_ADMIN_PASSWORD; prefer the env var)")
	fs.StringVar(&cfg.AdminDir, "admin-dir", cfg.AdminDir, "static admin panel build `directory` served at /admin/ (env WT_ADMIN_DIR)")
	fs.BoolVar(&cfg.AllowRegistration, "allow-registration", cfg.AllowRegistration, "allow self-service account registration (env WT_ALLOW_REGISTRATION)")
	fs.TextVar(&cfg.LogLevel, "log-level", cfg.LogLevel, "minimum log `level`: debug, info, warn, error (env WT_LOG_LEVEL)")
	fs.StringVar(&cfg.LogFormat, "log-format", cfg.LogFormat, "log `format`: text or json (env WT_LOG_FORMAT)")
	fs.Func("trusted-proxies", "comma-separated reverse-proxy `addresses` (IPs or CIDRs) whose X-Forwarded-For is trusted (env WT_TRUSTED_PROXIES)", func(v string) error {
		cfg.TrustedProxies = splitList(v)
		return nil
	})
	fs.Func("cors-origins", "comma-separated extra CORS `origins`; * allows any (env WT_CORS_ORIGINS)", func(v string) error {
		cfg.CORSOrigins = splitList(v)
		return nil
	})
	fs.DurationVar(&cfg.ShutdownTimeout, "shutdown-timeout", cfg.ShutdownTimeout, "graceful shutdown `timeout` (env WT_SHUTDOWN_TIMEOUT)")
	fs.StringVar(&cfg.PublicURL, "public-url", cfg.PublicURL, "externally visible base `URL` of the server, used for the default SSO redirect URL (env WT_PUBLIC_URL)")
	fs.BoolVar(&cfg.ShowVersion, "version", false, "print the version and exit")

	// Operations (§13.3).
	fs.StringVar(&cfg.TLSCert, "tls-cert", cfg.TLSCert, "PEM certificate `file`; with --tls-key serves HTTPS and gRPC over TLS (env WT_TLS_CERT)")
	fs.StringVar(&cfg.TLSKey, "tls-key", cfg.TLSKey, "PEM private key `file` for --tls-cert (env WT_TLS_KEY)")
	fs.StringVar(&cfg.TLSClientCA, "tls-client-ca", cfg.TLSClientCA, "PEM CA bundle `file`; requires client certificates signed by it (mTLS) (env WT_TLS_CLIENT_CA)")
	fs.DurationVar(&cfg.GCInterval, "gc-interval", cfg.GCInterval, "garbage-collection `period`; 0 disables automatic runs (env WT_GC_INTERVAL)")
	fs.DurationVar(&cfg.GCGrace, "gc-grace", cfg.GCGrace, "how long unreachable objects stay marked before deletion (env WT_GC_GRACE)")
	// staged-v2 (§16.8): --staged-ttl is the preferred name; --staged-retention is kept as an alias.
	fs.DurationVar(&cfg.StagedRetention, "staged-ttl", cfg.StagedRetention, "expire staged snapshots older than this `age` (status expired, kept 30 days in the staged history); 0 keeps them (env WT_STAGED_TTL)")
	fs.DurationVar(&cfg.StagedRetention, "staged-retention", cfg.StagedRetention, "alias of --staged-ttl (env WT_STAGED_RETENTION)")
	fs.DurationVar(&cfg.AuditRetention, "audit-retention", cfg.AuditRetention, "delete audit events older than this `age`; 0 keeps them (env WT_AUDIT_RETENTION)")
	fs.IntVar(&cfg.RateLimit, "rate-limit", cfg.RateLimit, "requests per minute per principal or anonymous IP; 0 disables (env WT_RATE_LIMIT)")
	fs.IntVar(&cfg.DefaultMaxTrees, "default-max-trees", cfg.DefaultMaxTrees, "default maximum trees per tenant; 0 = unlimited (env WT_DEFAULT_MAX_TREES)")
	fs.Int64Var(&cfg.DefaultMaxStorage, "default-max-storage", cfg.DefaultMaxStorage, "default maximum stored `bytes` per tenant; 0 = unlimited (env WT_DEFAULT_MAX_STORAGE)")

	// webhooks (§14.2).
	fs.BoolVar(&cfg.WebhookAllowPrivate, "webhook-allow-private", cfg.WebhookAllowPrivate, "allow webhook targets on loopback, link-local and private addresses (env WT_WEBHOOK_ALLOW_PRIVATE)")

	// Mirrors (§15.6).
	fs.StringVar(&cfg.GitBinary, "git-binary", cfg.GitBinary, "git `executable` for Git mirrors; mirrors are disabled when it is missing (env WT_GIT_BINARY)")
	fs.BoolVar(&cfg.MirrorAllowLocal, "mirror-allow-local", cfg.MirrorAllowLocal, "allow file:// mirror remotes and mirror hosts on loopback, link-local and private addresses (env WT_MIRROR_ALLOW_LOCAL)")

	// Storage (§14.6).
	fs.BoolVar(&cfg.CompressObjects, "compress-objects", cfg.CompressObjects, "store new objects zstd-compressed when that saves at least 10% (env WT_COMPRESS_OBJECTS)")

	// Storage backends and backups (§15.2).
	backendFlags(fs, cfg)
	// Notifications (§15.3).
	fs.StringVar(&cfg.SMTPURL, "smtp-url", cfg.SMTPURL, "send notification emails through this SMTP server: smtp://user:pass@host:587 (STARTTLS) or smtps://... (env WT_SMTP_URL; prefer the env var)")
	fs.StringVar(&cfg.SMTPFrom, "smtp-from", cfg.SMTPFrom, "From `address` of notification emails, e.g. \"W0rkTree <wt@example.com>\" (env WT_SMTP_FROM)")
	// Code search v2 (§15.9).
	codeIndexFlags(fs, cfg)
	// Tenant lifecycle (§16.4).
	tenantFlags(fs, cfg)
	packFlags(fs, cfg) // pack-delta (§16.1)
	// hardening (§15.10)
	fs.BoolVar(&cfg.Pprof, "pprof", cfg.Pprof, "serve Go runtime profiles at /debug/pprof to server admins (env WT_PPROF)")

	// hardening (§15.10): for tests. The flag set has no hidden-flag
	// mechanism, so the help text says it is for testing only.
	fs.Func("webhook-retry-schedule", "(testing only) comma-separated webhook retry `delays`, e.g. 1s,2s, replacing 1m,5m,30m,2h (env WT_WEBHOOK_RETRY_SCHEDULE)", func(v string) error {
		s, err := ParseRetrySchedule(v)
		if err != nil {
			return err
		}
		cfg.WebhookRetrySchedule = s
		return nil
	})
	// end hardening (§15.10): for tests
	return fs
}

// hardening (§15.10): for tests

// MaxRetrySchedule bounds the number of entries of --webhook-retry-schedule.
const MaxRetrySchedule = 10

// ParseRetrySchedule parses a comma-separated list of positive Go
// durations of at most 24h each (at most MaxRetrySchedule entries).
func ParseRetrySchedule(v string) ([]time.Duration, error) {
	parts := splitList(v)
	if len(parts) == 0 {
		return nil, fmt.Errorf("retry schedule %q: want comma-separated durations such as 1s,2s", v)
	}
	if len(parts) > MaxRetrySchedule {
		return nil, fmt.Errorf("retry schedule %q: at most %d delays are allowed", v, MaxRetrySchedule)
	}
	out := make([]time.Duration, 0, len(parts))
	for _, p := range parts {
		d, err := time.ParseDuration(p)
		if err != nil {
			return nil, fmt.Errorf("retry schedule %q: invalid duration %q", v, p)
		}
		if d <= 0 || d > 24*time.Hour {
			return nil, fmt.Errorf("retry schedule %q: delay %s must be positive and at most 24h", v, d)
		}
		out = append(out, d)
	}
	return out, nil
}

// WebhookPollInterval is the webhook retry poll interval that suits
// WebhookRetrySchedule: a quarter of its shortest delay, at least 50ms and
// at most webhooks' default 5s; 0 (the default) when no schedule is set.
func (c *Config) WebhookPollInterval() time.Duration {
	if len(c.WebhookRetrySchedule) == 0 {
		return 0
	}
	short := c.WebhookRetrySchedule[0]
	for _, d := range c.WebhookRetrySchedule[1:] {
		short = min(short, d)
	}
	return min(max(short/4, 50*time.Millisecond), 5*time.Second)
}

// end hardening (§15.10): for tests

// Usage writes the flag documentation to w.
func Usage(w io.Writer) {
	cfg, _ := fromEnv(func(string) (string, bool) { return "", false })
	fs := newFlagSet(&cfg, w)
	fmt.Fprintf(w, "Usage: worktree-server [flags]\n\nRuns the W0rkTree server (gRPC sync + REST API + admin panel) on one port.\n\nFlags:\n")
	fs.PrintDefaults()
	restoreUsageHint(w) // storage backends (§15.2): `worktree-server restore`
}

func splitList(v string) []string {
	var out []string
	for _, s := range strings.Split(v, ",") {
		if s = strings.TrimSpace(s); s != "" {
			out = append(out, s)
		}
	}
	return out
}

var usernameRe = regexp.MustCompile(`^[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?$`)

// Validate checks the configuration for consistency.
func (c *Config) Validate() error {
	var errs []error
	if c.Addr == "" {
		errs = append(errs, errors.New("addr must not be empty"))
	} else if _, port, err := net.SplitHostPort(c.Addr); err != nil {
		errs = append(errs, fmt.Errorf("addr %q: %v", c.Addr, err))
	} else if n, err := strconv.Atoi(port); err != nil || n < 0 || n > 65535 {
		errs = append(errs, fmt.Errorf("addr %q: invalid port", c.Addr))
	}
	if strings.TrimSpace(c.DataDir) == "" {
		errs = append(errs, errors.New("data directory must not be empty"))
	}
	if c.AdminPassword != "" && !usernameRe.MatchString(c.AdminUser) {
		errs = append(errs, fmt.Errorf("admin-user %q must be 1-63 lowercase letters, digits or '-'", c.AdminUser))
	}
	if c.LogFormat != "text" && c.LogFormat != "json" {
		errs = append(errs, fmt.Errorf("log-format %q: want text or json", c.LogFormat))
	}
	if c.ShutdownTimeout <= 0 {
		errs = append(errs, fmt.Errorf("shutdown-timeout must be positive, got %s", c.ShutdownTimeout))
	}
	if _, err := ParseProxies(c.TrustedProxies); err != nil {
		errs = append(errs, err)
	}
	for _, o := range c.CORSOrigins {
		if o == "*" {
			continue
		}
		u, err := url.Parse(o)
		if err != nil || (u.Scheme != "http" && u.Scheme != "https") || u.Host == "" || (u.Path != "" && u.Path != "/") || u.RawQuery != "" {
			errs = append(errs, fmt.Errorf("cors origin %q: want scheme://host[:port]", o))
		}
	}
	if c.PublicURL != "" {
		u, err := url.Parse(c.PublicURL)
		if err != nil || (u.Scheme != "http" && u.Scheme != "https") || u.Host == "" || u.User != nil ||
			u.RawQuery != "" || u.Fragment != "" {
			errs = append(errs, fmt.Errorf("public-url %q: want scheme://host[:port][/path]", c.PublicURL))
		}
	}
	errs = append(errs, c.validateOps()...)
	errs = append(errs, c.validateBackends()...)  // storage backends (§15.2)
	errs = append(errs, c.validateSMTP()...)      // notifications (§15.3)
	errs = append(errs, c.validateCodeIndex()...) // code search v2 (§15.9)
	errs = append(errs, c.validateTenants()...)   // tenant lifecycle (§16.4)
	errs = append(errs, c.validatePacks()...)     // pack-delta (§16.1)
	return errors.Join(errs...)
}

// validateOps checks the §13.3 operations settings.
func (c *Config) validateOps() []error {
	var errs []error
	if (c.TLSCert == "") != (c.TLSKey == "") {
		errs = append(errs, errors.New("tls-cert and tls-key must be given together"))
	}
	if c.TLSClientCA != "" && c.TLSCert == "" {
		errs = append(errs, errors.New("tls-client-ca requires tls-cert and tls-key"))
	}
	for _, d := range []struct {
		name string
		v    time.Duration
	}{{"gc-interval", c.GCInterval}, {"gc-grace", c.GCGrace}, {"staged-retention", c.StagedRetention}, {"audit-retention", c.AuditRetention}} {
		if d.v < 0 {
			errs = append(errs, fmt.Errorf("%s must not be negative, got %s", d.name, d.v))
		}
	}
	if c.RateLimit < 0 {
		errs = append(errs, fmt.Errorf("rate-limit must not be negative, got %d", c.RateLimit))
	}
	if c.DefaultMaxTrees < 0 {
		errs = append(errs, fmt.Errorf("default-max-trees must not be negative, got %d", c.DefaultMaxTrees))
	}
	if c.DefaultMaxStorage < 0 {
		errs = append(errs, fmt.Errorf("default-max-storage must not be negative, got %d", c.DefaultMaxStorage))
	}
	return errs
}

// ---- notifications (§15.3) ----

// validateSMTP checks --smtp-url and --smtp-from.
func (c *Config) validateSMTP() []error {
	var errs []error
	if c.SMTPURL != "" {
		u, err := url.Parse(c.SMTPURL)
		if err != nil || (u.Scheme != "smtp" && u.Scheme != "smtps") || u.Hostname() == "" || (u.Path != "" && u.Path != "/") {
			// Never echo the URL: it may contain a password.
			errs = append(errs, errors.New("smtp-url: want smtp://[user:pass@]host[:port] or smtps://[user:pass@]host[:port]"))
		}
	}
	if c.SMTPFrom != "" {
		if _, err := mail.ParseAddress(c.SMTPFrom); err != nil {
			errs = append(errs, fmt.Errorf("smtp-from %q: %v", c.SMTPFrom, err))
		}
	}
	if c.SMTPFrom != "" && c.SMTPURL == "" {
		errs = append(errs, errors.New("smtp-from requires smtp-url"))
	}
	return errs
}

// ---- end notifications ----

// TLSEnabled reports whether the server serves TLS.
func (c *Config) TLSEnabled() bool { return c.TLSCert != "" && c.TLSKey != "" }

// CleartextExposed reports whether the server listens without TLS on an
// address reachable from other hosts (anything but a loopback host).
func (c *Config) CleartextExposed() bool {
	if c.TLSEnabled() {
		return false
	}
	host, _, err := net.SplitHostPort(c.Addr)
	if err != nil {
		return true
	}
	if host == "localhost" {
		return false
	}
	ip := net.ParseIP(host)
	return ip == nil || !ip.IsLoopback()
}

// ParseProxies parses trusted proxy addresses (IPs or CIDRs).
func ParseProxies(list []string) ([]netip.Prefix, error) {
	out := make([]netip.Prefix, 0, len(list))
	for _, s := range list {
		if p, err := netip.ParsePrefix(s); err == nil {
			out = append(out, p.Masked())
			continue
		}
		a, err := netip.ParseAddr(s)
		if err != nil {
			return nil, fmt.Errorf("trusted-proxies %q: want an IP address or CIDR", s)
		}
		out = append(out, netip.PrefixFrom(a.Unmap(), a.Unmap().BitLen()))
	}
	return out, nil
}
