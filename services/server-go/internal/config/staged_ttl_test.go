package config

// staged-v2 (§16.8): --staged-ttl / WT_STAGED_TTL (alias of
// --staged-retention / WT_STAGED_RETENTION), default 30 days.

import (
	"testing"
	"time"
)

func TestStagedTTL(t *testing.T) {
	for _, tc := range []struct {
		name string
		args []string
		env  map[string]string
		want time.Duration
	}{
		{"default", nil, nil, 30 * 24 * time.Hour},
		{"flag", []string{"--staged-ttl=48h"}, nil, 48 * time.Hour},
		{"alias flag", []string{"--staged-retention=2h"}, nil, 2 * time.Hour},
		{"env", nil, map[string]string{"WT_STAGED_TTL": "3h"}, 3 * time.Hour},
		{"ttl env beats retention env", nil, map[string]string{"WT_STAGED_RETENTION": "1h", "WT_STAGED_TTL": "5h"}, 5 * time.Hour},
		{"flag beats env", []string{"--staged-ttl=0"}, map[string]string{"WT_STAGED_TTL": "3h"}, 0},
	} {
		t.Run(tc.name, func(t *testing.T) {
			cfg, err := load(tc.args, env(tc.env))
			if err != nil {
				t.Fatal(err)
			}
			if cfg.StagedRetention != tc.want {
				t.Fatalf("staged ttl = %v, want %v", cfg.StagedRetention, tc.want)
			}
		})
	}
	if _, err := load([]string{"--staged-ttl=-1s"}, env(nil)); err == nil {
		t.Fatal("negative ttl accepted")
	}
}
