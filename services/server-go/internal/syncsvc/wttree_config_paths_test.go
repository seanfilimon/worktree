package syncsvc

import "testing"

// config-as-code (docs/ENGINEERING_PLAN.md §16.3): branches.toml and
// protections.toml are shared `.wt-tree/` files.
func TestConfigAsCodePathsAreShared(t *testing.T) {
	for _, p := range []string{".wt-tree/branches.toml", ".wt-tree/protections.toml"} {
		if !IsSharedTreeConfigPath(p) {
			t.Errorf("IsSharedTreeConfigPath(%q) = false", p)
		}
		if err := validateManifestPath(p); err != nil {
			t.Errorf("validateManifestPath(%q) = %v", p, err)
		}
	}
	for _, p := range []string{".wt-tree/branches.toml/x", "a/.wt-tree/branches.toml", ".wt-tree/protection.toml"} {
		if err := validateManifestPath(p); err == nil {
			t.Errorf("validateManifestPath(%q) = nil, want an error", p)
		}
	}
}
