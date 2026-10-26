package config

import (
	"bytes"
	"strings"
	"testing"
)

// Code search v2 (§15.9): --code-index-memory / --code-index-branches.
func TestCodeIndexSettings(t *testing.T) {
	cfg, err := load(nil, env(nil))
	if err != nil || cfg.CodeIndexMemory != 256<<20 || cfg.CodeIndexBranches != 10 {
		t.Fatalf("defaults = %d %d, %v", cfg.CodeIndexMemory, cfg.CodeIndexBranches, err)
	}
	cfg, err = load(nil, env(map[string]string{"WT_CODE_INDEX_MEMORY": "1GiB", "WT_CODE_INDEX_BRANCHES": "3"}))
	if err != nil || cfg.CodeIndexMemory != 1<<30 || cfg.CodeIndexBranches != 3 {
		t.Fatalf("env = %d %d, %v", cfg.CodeIndexMemory, cfg.CodeIndexBranches, err)
	}
	cfg, err = load([]string{"--code-index-memory", "64M", "--code-index-branches=25"}, env(map[string]string{"WT_CODE_INDEX_MEMORY": "1G"}))
	if err != nil || cfg.CodeIndexMemory != 64<<20 || cfg.CodeIndexBranches != 25 {
		t.Fatalf("flags = %d %d, %v", cfg.CodeIndexMemory, cfg.CodeIndexBranches, err)
	}
	cfg, err = load([]string{"--code-index-memory=0"}, env(nil))
	if err != nil || cfg.CodeIndexMemory != 0 {
		t.Fatalf("unlimited = %d, %v", cfg.CodeIndexMemory, err)
	}
	for _, bad := range [][]string{{"--code-index-memory=lots"}, {"--code-index-branches=0"}, {"--code-index-branches=5000"}} {
		if _, err := load(bad, env(nil)); err == nil {
			t.Errorf("%v accepted", bad)
		}
	}
	for _, bad := range []map[string]string{{"WT_CODE_INDEX_MEMORY": "-1"}, {"WT_CODE_INDEX_BRANCHES": "x"}} {
		if _, err := load(nil, env(bad)); err == nil {
			t.Errorf("%v accepted", bad)
		}
	}
	var b bytes.Buffer
	Usage(&b)
	if !strings.Contains(b.String(), "-code-index-memory") || !strings.Contains(b.String(), "-code-index-branches") {
		t.Fatalf("usage lacks the code index flags:\n%s", b.String())
	}
}

func TestParseByteSize(t *testing.T) {
	cases := map[string]int64{"0": 0, "1024": 1024, "10B": 10, "4K": 4 << 10, "4KiB": 4 << 10, "4KB": 4000,
		"256MiB": 256 << 20, "256 MB": 256e6, "1.5G": 3 << 29, "2TiB": 2 << 40, "1tb": 1e12}
	for in, want := range cases {
		if got, err := ParseByteSize(in); err != nil || got != want {
			t.Errorf("ParseByteSize(%q) = %d, %v; want %d", in, got, err, want)
		}
	}
	for _, bad := range []string{"", "MiB", "-1", "1XB", "1e400", "abc"} {
		if _, err := ParseByteSize(bad); err == nil {
			t.Errorf("ParseByteSize(%q) accepted", bad)
		}
	}
}
