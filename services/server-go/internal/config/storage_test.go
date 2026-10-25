package config

import "testing"

// Storage flags (docs/ENGINEERING_PLAN.md §14.6).

func TestCompressObjectsFlag(t *testing.T) {
	env := func(m map[string]string) lookupFunc {
		return func(k string) (string, bool) { v, ok := m[k]; return v, ok }
	}
	cfg, err := load(nil, env(nil))
	if err != nil || !cfg.CompressObjects {
		t.Fatalf("default: %v %v", cfg.CompressObjects, err)
	}
	cfg, err = load(nil, env(map[string]string{"WT_COMPRESS_OBJECTS": "false"}))
	if err != nil || cfg.CompressObjects {
		t.Fatalf("env false: %v %v", cfg.CompressObjects, err)
	}
	cfg, err = load([]string{"--compress-objects=true"}, env(map[string]string{"WT_COMPRESS_OBJECTS": "0"}))
	if err != nil || !cfg.CompressObjects {
		t.Fatalf("flag overrides env: %v %v", cfg.CompressObjects, err)
	}
	cfg, err = load([]string{"--compress-objects=false"}, env(nil))
	if err != nil || cfg.CompressObjects {
		t.Fatalf("flag false: %v %v", cfg.CompressObjects, err)
	}
	if _, err := load(nil, env(map[string]string{"WT_COMPRESS_OBJECTS": "maybe"})); err == nil {
		t.Fatal("invalid boolean accepted")
	}
}
