package config

import (
	"strings"
	"testing"
	"time"
)

func TestOpsDefaults(t *testing.T) {
	cfg, err := load(nil, env(nil))
	if err != nil {
		t.Fatal(err)
	}
	if cfg.GCInterval != 24*time.Hour || cfg.GCGrace != 720*time.Hour || cfg.StagedRetention != 720*time.Hour ||
		cfg.AuditRetention != 0 || cfg.RateLimit != 1200 || cfg.DefaultMaxTrees != 0 || cfg.DefaultMaxStorage != 0 ||
		cfg.TLSCert != "" || cfg.TLSKey != "" || cfg.TLSClientCA != "" || cfg.TLSEnabled() {
		t.Fatalf("ops defaults = %+v", cfg)
	}
}

func TestOpsEnvAndFlags(t *testing.T) {
	tests := []struct {
		name  string
		args  []string
		env   map[string]string
		check func(t *testing.T, c Config)
	}{
		{"env", nil, map[string]string{
			"WT_TLS_CERT": "c.pem", "WT_TLS_KEY": "k.pem", "WT_TLS_CLIENT_CA": "ca.pem",
			"WT_GC_INTERVAL": "1h", "WT_GC_GRACE": "2h", "WT_STAGED_RETENTION": "0", "WT_AUDIT_RETENTION": "2160h",
			"WT_RATE_LIMIT": "60", "WT_DEFAULT_MAX_TREES": "5", "WT_DEFAULT_MAX_STORAGE": "1048576",
		}, func(t *testing.T, c Config) {
			if c.TLSCert != "c.pem" || c.TLSKey != "k.pem" || c.TLSClientCA != "ca.pem" || !c.TLSEnabled() {
				t.Fatalf("tls = %+v", c)
			}
			if c.GCInterval != time.Hour || c.GCGrace != 2*time.Hour || c.StagedRetention != 0 || c.AuditRetention != 2160*time.Hour {
				t.Fatalf("durations = %+v", c)
			}
			if c.RateLimit != 60 || c.DefaultMaxTrees != 5 || c.DefaultMaxStorage != 1<<20 {
				t.Fatalf("limits = %+v", c)
			}
		}},
		{"flags beat env", []string{"--gc-interval=0", "--rate-limit=0", "--default-max-trees=2", "--default-max-storage=99", "--audit-retention=48h"},
			map[string]string{"WT_GC_INTERVAL": "1h", "WT_RATE_LIMIT": "60"},
			func(t *testing.T, c Config) {
				if c.GCInterval != 0 || c.RateLimit != 0 || c.DefaultMaxTrees != 2 || c.DefaultMaxStorage != 99 || c.AuditRetention != 48*time.Hour {
					t.Fatalf("%+v", c)
				}
			}},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			cfg, err := load(tc.args, env(tc.env))
			if err != nil {
				t.Fatal(err)
			}
			tc.check(t, cfg)
		})
	}
}

func TestOpsInvalid(t *testing.T) {
	tests := []struct {
		name string
		args []string
		env  map[string]string
		want string
	}{
		{"bad env duration", nil, map[string]string{"WT_GC_GRACE": "soon"}, "WT_GC_GRACE"},
		{"bad env integer", nil, map[string]string{"WT_RATE_LIMIT": "lots"}, "WT_RATE_LIMIT"},
		{"cert without key", []string{"--tls-cert=c.pem"}, nil, "together"},
		{"key without cert", []string{"--tls-key=k.pem"}, nil, "together"},
		{"client ca without tls", []string{"--tls-client-ca=ca.pem"}, nil, "tls-client-ca"},
		{"negative interval", []string{"--gc-interval=-1h"}, nil, "gc-interval"},
		{"negative retention", []string{"--staged-retention=-1s"}, nil, "staged-retention"},
		{"negative rate", []string{"--rate-limit=-5"}, nil, "rate-limit"},
		{"negative trees", []string{"--default-max-trees=-1"}, nil, "default-max-trees"},
		{"negative storage", []string{"--default-max-storage=-1"}, nil, "default-max-storage"},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			_, err := load(tc.args, env(tc.env))
			if err == nil || !strings.Contains(err.Error(), tc.want) {
				t.Fatalf("err = %v, want mention of %q", err, tc.want)
			}
		})
	}
}

func TestCleartextExposed(t *testing.T) {
	tests := []struct {
		addr string
		tls  bool
		want bool
	}{
		{":7420", false, true},
		{"0.0.0.0:7420", false, true},
		{"10.1.2.3:7420", false, true},
		{"example.com:7420", false, true},
		{"127.0.0.1:7420", false, false},
		{"[::1]:7420", false, false},
		{"localhost:7420", false, false},
		{":7420", true, false},
	}
	for _, tc := range tests {
		c := Config{Addr: tc.addr}
		if tc.tls {
			c.TLSCert, c.TLSKey = "c", "k"
		}
		if got := c.CleartextExposed(); got != tc.want {
			t.Errorf("CleartextExposed(%q, tls=%v) = %v, want %v", tc.addr, tc.tls, got, tc.want)
		}
	}
}
