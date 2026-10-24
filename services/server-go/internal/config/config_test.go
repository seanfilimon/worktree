package config

import (
	"errors"
	"flag"
	"log/slog"
	"slices"
	"strings"
	"testing"
	"time"
)

func env(m map[string]string) lookupFunc {
	return func(k string) (string, bool) {
		v, ok := m[k]
		return v, ok
	}
}

func TestDefaults(t *testing.T) {
	cfg, err := load(nil, env(nil))
	if err != nil {
		t.Fatal(err)
	}
	want := Config{
		Addr:              ":7420",
		DataDir:           "./wt-data",
		AdminUser:         "admin",
		AllowRegistration: true,
		LogLevel:          slog.LevelInfo,
		LogFormat:         "text",
		ShutdownTimeout:   15 * time.Second,
	}
	if cfg.Addr != want.Addr || cfg.DataDir != want.DataDir || cfg.AdminUser != want.AdminUser ||
		cfg.AllowRegistration != want.AllowRegistration || cfg.LogLevel != want.LogLevel ||
		cfg.LogFormat != want.LogFormat || cfg.ShutdownTimeout != want.ShutdownTimeout ||
		cfg.AdminPassword != "" || cfg.AdminDir != "" || cfg.CORSOrigins != nil || cfg.ShowVersion {
		t.Fatalf("defaults = %+v", cfg)
	}
}

func TestPrecedence(t *testing.T) {
	tests := []struct {
		name  string
		args  []string
		env   map[string]string
		check func(t *testing.T, c Config)
	}{
		{"env overrides default", nil, map[string]string{"WT_ADDR": "127.0.0.1:9000", "WT_DATA": "/srv/wt"},
			func(t *testing.T, c Config) {
				if c.Addr != "127.0.0.1:9000" || c.DataDir != "/srv/wt" {
					t.Fatalf("%+v", c)
				}
			}},
		{"flag overrides env", []string{"--addr", ":8000", "-data=flagdir"}, map[string]string{"WT_ADDR": "127.0.0.1:9000", "WT_DATA": "envdir"},
			func(t *testing.T, c Config) {
				if c.Addr != ":8000" || c.DataDir != "flagdir" {
					t.Fatalf("%+v", c)
				}
			}},
		{"empty env ignored", nil, map[string]string{"WT_ADDR": ""},
			func(t *testing.T, c Config) {
				if c.Addr != DefaultAddr {
					t.Fatalf("addr = %q", c.Addr)
				}
			}},
		{"admin settings from env", nil, map[string]string{"WT_ADMIN_USER": "root", "WT_ADMIN_PASSWORD": "s3cret", "WT_ADMIN_DIR": "apps/admin/dist"},
			func(t *testing.T, c Config) {
				if c.AdminUser != "root" || c.AdminPassword != "s3cret" || c.AdminDir != "apps/admin/dist" {
					t.Fatalf("%+v", c)
				}
			}},
		{"admin flags beat env", []string{"--admin-user=boss", "--admin-password=pw", "--admin-dir=d"}, map[string]string{"WT_ADMIN_USER": "root", "WT_ADMIN_PASSWORD": "s3cret"},
			func(t *testing.T, c Config) {
				if c.AdminUser != "boss" || c.AdminPassword != "pw" || c.AdminDir != "d" {
					t.Fatalf("%+v", c)
				}
			}},
		{"registration env false", nil, map[string]string{"WT_ALLOW_REGISTRATION": "false"},
			func(t *testing.T, c Config) {
				if c.AllowRegistration {
					t.Fatal("registration should be disabled")
				}
			}},
		{"registration flag beats env", []string{"--allow-registration=true"}, map[string]string{"WT_ALLOW_REGISTRATION": "0"},
			func(t *testing.T, c Config) {
				if !c.AllowRegistration {
					t.Fatal("registration should be enabled")
				}
			}},
		{"registration flag false", []string{"--allow-registration=false"}, nil,
			func(t *testing.T, c Config) {
				if c.AllowRegistration {
					t.Fatal("registration should be disabled")
				}
			}},
		{"log level env", nil, map[string]string{"WT_LOG_LEVEL": "warn", "WT_LOG_FORMAT": "json"},
			func(t *testing.T, c Config) {
				if c.LogLevel != slog.LevelWarn || c.LogFormat != "json" {
					t.Fatalf("%+v", c)
				}
			}},
		{"log level flag beats env", []string{"--log-level", "DEBUG"}, map[string]string{"WT_LOG_LEVEL": "error"},
			func(t *testing.T, c Config) {
				if c.LogLevel != slog.LevelDebug {
					t.Fatalf("level = %v", c.LogLevel)
				}
			}},
		{"cors env", nil, map[string]string{"WT_CORS_ORIGINS": " https://a.example.com, http://b.example.com:8080 ,"},
			func(t *testing.T, c Config) {
				if !slices.Equal(c.CORSOrigins, []string{"https://a.example.com", "http://b.example.com:8080"}) {
					t.Fatalf("cors = %q", c.CORSOrigins)
				}
			}},
		{"cors flag beats env", []string{"--cors-origins=*"}, map[string]string{"WT_CORS_ORIGINS": "https://a.example.com"},
			func(t *testing.T, c Config) {
				if !slices.Equal(c.CORSOrigins, []string{"*"}) {
					t.Fatalf("cors = %q", c.CORSOrigins)
				}
			}},
		{"shutdown timeout", []string{"--shutdown-timeout=3s"}, map[string]string{"WT_SHUTDOWN_TIMEOUT": "1m"},
			func(t *testing.T, c Config) {
				if c.ShutdownTimeout != 3*time.Second {
					t.Fatalf("timeout = %v", c.ShutdownTimeout)
				}
			}},
		{"shutdown timeout env", nil, map[string]string{"WT_SHUTDOWN_TIMEOUT": "1m"},
			func(t *testing.T, c Config) {
				if c.ShutdownTimeout != time.Minute {
					t.Fatalf("timeout = %v", c.ShutdownTimeout)
				}
			}},
		{"version", []string{"--version"}, map[string]string{"WT_ADDR": "not-an-addr"},
			func(t *testing.T, c Config) {
				if !c.ShowVersion {
					t.Fatal("ShowVersion not set")
				}
			}},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()
			cfg, err := load(tc.args, env(tc.env))
			if err != nil {
				t.Fatal(err)
			}
			tc.check(t, cfg)
		})
	}
}

func TestInvalid(t *testing.T) {
	tests := []struct {
		name    string
		args    []string
		env     map[string]string
		wantErr string
	}{
		{"bad bool env", nil, map[string]string{"WT_ALLOW_REGISTRATION": "maybe"}, "WT_ALLOW_REGISTRATION"},
		{"bad level env", nil, map[string]string{"WT_LOG_LEVEL": "loud"}, "WT_LOG_LEVEL"},
		{"bad duration env", nil, map[string]string{"WT_SHUTDOWN_TIMEOUT": "soon"}, "WT_SHUTDOWN_TIMEOUT"},
		{"bad level flag", []string{"--log-level=loud"}, nil, "log-level"},
		{"unknown flag", []string{"--nope"}, nil, "nope"},
		{"positional arg", []string{"extra"}, nil, "unexpected argument"},
		{"bad addr", []string{"--addr=localhost"}, nil, "addr"},
		{"bad port", []string{"--addr=:99999"}, nil, "invalid port"},
		{"empty data", []string{"--data="}, nil, "data directory"},
		{"bad admin user", []string{"--admin-user=Root User", "--admin-password=x"}, nil, "admin-user"},
		{"bad log format", []string{"--log-format=xml"}, nil, "log-format"},
		{"bad timeout", []string{"--shutdown-timeout=0s"}, nil, "shutdown-timeout"},
		{"bad cors origin", []string{"--cors-origins=localhost:3000"}, nil, "cors origin"},
		{"cors origin with path", []string{"--cors-origins=https://a.example.com/app"}, nil, "cors origin"},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()
			_, err := load(tc.args, env(tc.env))
			if err == nil || !strings.Contains(err.Error(), tc.wantErr) {
				t.Fatalf("err = %v, want containing %q", err, tc.wantErr)
			}
		})
	}
}

func TestHelp(t *testing.T) {
	if _, err := load([]string{"-h"}, env(nil)); !errors.Is(err, flag.ErrHelp) {
		t.Fatalf("err = %v, want flag.ErrHelp", err)
	}
	var b strings.Builder
	Usage(&b)
	for _, f := range []string{"-addr", "-data", "-admin-user", "-admin-password", "-admin-dir", "-allow-registration", "-log-level", "-version", "WT_ADDR"} {
		if !strings.Contains(b.String(), f) {
			t.Errorf("usage missing %s:\n%s", f, b.String())
		}
	}
}

func TestLoadUsesProcessEnv(t *testing.T) {
	t.Setenv("WT_ADDR", "127.0.0.1:7777")
	cfg, err := Load(nil)
	if err != nil {
		t.Fatal(err)
	}
	if cfg.Addr != "127.0.0.1:7777" {
		t.Fatalf("addr = %q", cfg.Addr)
	}
}
