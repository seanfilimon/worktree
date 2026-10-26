package config

import (
	"strings"
	"testing"
)

func TestPackSettings(t *testing.T) {
	cfg, err := load(nil, env(nil))
	if err != nil {
		t.Fatal(err)
	}
	if cfg.Packs.Disabled || cfg.Packs.MaxPackSize != DefaultMaxPackSize {
		t.Fatalf("defaults: %+v", cfg.Packs)
	}
	cfg, err = load(nil, env(map[string]string{"WT_DISABLE_PACKS": "true", "WT_MAX_PACK_SIZE": "1GiB"}))
	if err != nil {
		t.Fatal(err)
	}
	if !cfg.Packs.Disabled || cfg.Packs.MaxPackSize != 1<<30 {
		t.Fatalf("env: %+v", cfg.Packs)
	}
	cfg, err = load([]string{"--disable-packs=false", "--max-pack-size", "512MiB"}, env(map[string]string{"WT_DISABLE_PACKS": "1"}))
	if err != nil {
		t.Fatal(err)
	}
	if cfg.Packs.Disabled || cfg.Packs.MaxPackSize != 512<<20 {
		t.Fatalf("flags: %+v", cfg.Packs)
	}
	if _, err := load([]string{"--max-pack-size", "10KiB"}, env(nil)); err == nil || !strings.Contains(err.Error(), "max-pack-size") {
		t.Fatalf("small size: %v", err)
	}
	if _, err := load(nil, env(map[string]string{"WT_DISABLE_PACKS": "maybe"})); err == nil {
		t.Fatal("bad boolean accepted")
	}
	if _, err := load(nil, env(map[string]string{"WT_MAX_PACK_SIZE": "lots"})); err == nil {
		t.Fatal("bad size accepted")
	}
}
