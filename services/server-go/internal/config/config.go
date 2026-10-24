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
	// ShutdownTimeout bounds graceful shutdown. Flag --shutdown-timeout, env
	// WT_SHUTDOWN_TIMEOUT.
	ShutdownTimeout time.Duration
	// ShowVersion requests printing the version and exiting (--version).
	ShowVersion bool

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
	if v, ok := lookup("WT_CORS_ORIGINS"); ok {
		cfg.CORSOrigins = splitList(v)
	}
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
	fs.Func("cors-origins", "comma-separated extra CORS `origins`; * allows any (env WT_CORS_ORIGINS)", func(v string) error {
		cfg.CORSOrigins = splitList(v)
		return nil
	})
	fs.DurationVar(&cfg.ShutdownTimeout, "shutdown-timeout", cfg.ShutdownTimeout, "graceful shutdown `timeout` (env WT_SHUTDOWN_TIMEOUT)")
	fs.BoolVar(&cfg.ShowVersion, "version", false, "print the version and exit")
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
	for _, o := range c.CORSOrigins {
		if o == "*" {
			continue
		}
		u, err := url.Parse(o)
		if err != nil || (u.Scheme != "http" && u.Scheme != "https") || u.Host == "" || (u.Path != "" && u.Path != "/") || u.RawQuery != "" {
			errs = append(errs, fmt.Errorf("cors origin %q: want scheme://host[:port]", o))
		}
	}
	return errors.Join(errs...)
}
