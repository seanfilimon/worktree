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

	// Operations defaults (§13.3).
	DefaultGCInterval      = 24 * time.Hour
	DefaultGCGrace         = 720 * time.Hour
	DefaultStagedRetention = 168 * time.Hour
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
	// them). Flag --staged-retention, env WT_STAGED_RETENTION.
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

	// ---- storage (§14.6) ----

	// CompressObjects stores new objects zstd-compressed (WTZ1) when that
	// saves at least 10 %; existing objects are read either way. Flag
	// --compress-objects, env WT_COMPRESS_OBJECTS (default true).
	CompressObjects bool

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
	fs.DurationVar(&cfg.StagedRetention, "staged-retention", cfg.StagedRetention, "expire staged snapshots older than this `age`; 0 keeps them (env WT_STAGED_RETENTION)")
	fs.DurationVar(&cfg.AuditRetention, "audit-retention", cfg.AuditRetention, "delete audit events older than this `age`; 0 keeps them (env WT_AUDIT_RETENTION)")
	fs.IntVar(&cfg.RateLimit, "rate-limit", cfg.RateLimit, "requests per minute per principal or anonymous IP; 0 disables (env WT_RATE_LIMIT)")
	fs.IntVar(&cfg.DefaultMaxTrees, "default-max-trees", cfg.DefaultMaxTrees, "default maximum trees per tenant; 0 = unlimited (env WT_DEFAULT_MAX_TREES)")
	fs.Int64Var(&cfg.DefaultMaxStorage, "default-max-storage", cfg.DefaultMaxStorage, "default maximum stored `bytes` per tenant; 0 = unlimited (env WT_DEFAULT_MAX_STORAGE)")

	// Storage (§14.6).
	fs.BoolVar(&cfg.CompressObjects, "compress-objects", cfg.CompressObjects, "store new objects zstd-compressed when that saves at least 10% (env WT_COMPRESS_OBJECTS)")
	return fs
}

// Usage writes the flag documentation to w.
func Usage(w io.Writer) {
	cfg, _ := fromEnv(func(string) (string, bool) { return "", false })
	fs := newFlagSet(&cfg, w)
	fmt.Fprintf(w, "Usage: worktree-server [flags]\n\nRuns the W0rkTree server (gRPC sync + REST API + admin panel) on one port.\n\nFlags:\n")
	fs.PrintDefaults()
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
