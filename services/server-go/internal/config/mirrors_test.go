package config

import "testing"

// mirrors (§15.6): --git-binary / WT_GIT_BINARY and --mirror-allow-local /
// WT_MIRROR_ALLOW_LOCAL.
func TestMirrorFlags(t *testing.T) {
	cfg, err := load(nil, env(nil))
	if err != nil || cfg.GitBinary != "git" || cfg.MirrorAllowLocal {
		t.Fatalf("defaults = %q %v, %v", cfg.GitBinary, cfg.MirrorAllowLocal, err)
	}
	cfg, err = load(nil, env(map[string]string{"WT_GIT_BINARY": "/opt/git/bin/git", "WT_MIRROR_ALLOW_LOCAL": "true"}))
	if err != nil || cfg.GitBinary != "/opt/git/bin/git" || !cfg.MirrorAllowLocal {
		t.Fatalf("env = %q %v, %v", cfg.GitBinary, cfg.MirrorAllowLocal, err)
	}
	cfg, err = load([]string{"--git-binary", "git2", "--mirror-allow-local=false"}, env(map[string]string{"WT_GIT_BINARY": "x", "WT_MIRROR_ALLOW_LOCAL": "1"}))
	if err != nil || cfg.GitBinary != "git2" || cfg.MirrorAllowLocal {
		t.Fatalf("flags over env = %q %v, %v", cfg.GitBinary, cfg.MirrorAllowLocal, err)
	}
	if _, err := load(nil, env(map[string]string{"WT_MIRROR_ALLOW_LOCAL": "maybe"})); err == nil {
		t.Fatal("invalid boolean accepted")
	}
}
