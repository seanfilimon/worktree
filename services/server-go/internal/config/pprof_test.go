package config

import "testing"

// hardening (§15.10): --pprof / WT_PPROF.
func TestPprofFlag(t *testing.T) {
	cfg, err := load(nil, env(nil))
	if err != nil || cfg.Pprof {
		t.Fatalf("default = %v, %v", cfg.Pprof, err)
	}
	cfg, err = load(nil, env(map[string]string{"WT_PPROF": "true"}))
	if err != nil || !cfg.Pprof {
		t.Fatalf("env = %v, %v", cfg.Pprof, err)
	}
	cfg, err = load([]string{"--pprof=false"}, env(map[string]string{"WT_PPROF": "1"}))
	if err != nil || cfg.Pprof {
		t.Fatalf("flag over env = %v, %v", cfg.Pprof, err)
	}
	cfg, err = load([]string{"--pprof"}, env(nil))
	if err != nil || !cfg.Pprof {
		t.Fatalf("flag = %v, %v", cfg.Pprof, err)
	}
	if _, err := load(nil, env(map[string]string{"WT_PPROF": "maybe"})); err == nil {
		t.Fatal("invalid boolean accepted")
	}
}
