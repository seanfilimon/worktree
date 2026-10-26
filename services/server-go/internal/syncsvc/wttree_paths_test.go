package syncsvc

import (
	"errors"
	"testing"

	"google.golang.org/protobuf/proto"

	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
)

// hardening (docs/ENGINEERING_PLAN.md §15.10): a tree's own `.wt-tree/`
// shared files are valid manifest paths; `.wt/` and nested markers are not.

func TestValidateManifestPathSharedTreeConfig(t *testing.T) {
	for _, ok := range []string{
		"a", "a/b.txt", ".wtignore", "x/.hidden", "a..b", ".wtx/y", "a/.wt-treex",
		".wt-tree/config.toml",
		".wt-tree/ignore",
		".wt-tree/hooks/pre-snapshot",
		".wt-tree/hooks/lib/common.sh",
		".wt-tree/access/policies.toml",
	} {
		if err := validateManifestPath(ok); err != nil {
			t.Errorf("validateManifestPath(%q) = %v, want nil", ok, err)
		}
	}
	for _, bad := range []string{
		"", "/a", "a/", "a//b", "./a", "a/../b", "..", "a\\b",
		".wt", ".wt/config.toml", "x/.wt/y",
		".wt-tree", ".wt-tree/local.toml", ".wt-tree/other/x", ".wt-tree/hooks",
		".wt-tree/hooks/.wt/x", ".wt-tree/access/.wt-tree/x", ".wt-tree/.wt-tree/config.toml",
		"a/.wt-tree/config.toml", "a/.wt-tree/hooks/pre-snapshot", "x/.wt-tree/y",
	} {
		if err := validateManifestPath(bad); err == nil {
			t.Errorf("validateManifestPath(%q) = nil, want an error", bad)
		}
	}
}

func TestIsSharedTreeConfigPath(t *testing.T) {
	cases := map[string]bool{
		".wt-tree/config.toml":          true,
		".wt-tree/ignore":               true,
		".wt-tree/hooks/post-snapshot":  true,
		".wt-tree/access/policies.toml": true,
		".wt-tree/local.toml":           false,
		".wt-tree/hooks/":               false,
		"hooks/x":                       false,
		"a/.wt-tree/config.toml":        false,
	}
	for p, want := range cases {
		if got := IsSharedTreeConfigPath(p); got != want {
			t.Errorf("IsSharedTreeConfigPath(%q) = %v, want %v", p, got, want)
		}
	}
}

func TestDecodeManifestReservedPaths(t *testing.T) {
	enc := func(paths ...string) []byte {
		m := &syncpb.ManifestRecord{}
		for _, p := range paths {
			m.Entries = append(m.Entries, &syncpb.ManifestEntry{Path: p, BlobHash: hashToPB(hash.Sum([]byte(p)))})
		}
		b, err := proto.Marshal(m)
		if err != nil {
			t.Fatal(err)
		}
		return b
	}
	if _, err := decodeManifest(enc(".wt-tree/config.toml", ".wt-tree/hooks/pre-snapshot", "src/main.go")); err != nil {
		t.Fatalf("shared tree config rejected: %v", err)
	}
	for _, bad := range []string{".wt/objects/x", "sub/.wt-tree/config.toml", ".wt-tree/local.toml"} {
		_, err := decodeManifest(enc(bad))
		if !errors.Is(err, errInvalidManifest) {
			t.Errorf("decodeManifest(%q) = %v, want errInvalidManifest", bad, err)
		}
	}
}
