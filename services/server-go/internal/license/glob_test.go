package license

import (
	"testing"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

func TestMatch(t *testing.T) {
	tests := []struct {
		glob, path string
		want       bool
	}{
		// literals and directory prefixes
		{"README.md", "README.md", true},
		{"README.md", "docs/README.md", false},
		{"services/billing", "services/billing", true},
		{"services/billing", "services/billing/src/main.rs", true},
		{"services/billing", "services/billing-engine/x", false},
		{"services/billing/", "services/billing/a", true},
		{"vendor/**", "vendor/a/b/c.go", true},
		{"vendor/**", "vendor", true},
		{"vendor/**", "vendors/a", false},
		{"/vendor", "vendor/x", true},
		{"./vendor", "vendor/x", true},
		{`vendor\sdk`, "vendor/sdk/a.go", true},
		{"vendor", `vendor\sdk\a.go`, true},
		{"vendor", "/vendor/a", true},
		{"vendor", "./vendor/a", true},
		{"vendor", "x/../vendor/a", true},

		// single-segment wildcards
		{"*.rs", "main.rs", true},
		{"*.rs", "src/main.rs", false},
		{"src/*.rs", "src/main.rs", true},
		{"src/*.rs", "src/a/main.rs", false},
		{"src/*", "src/a/main.rs", true}, // matches directory "src/a"
		{"src/?.go", "src/a.go", true},
		{"src/?.go", "src/ab.go", false},
		{"src/?", "src/", false},
		{"src/[ab].go", "src/b.go", true},
		{"src/[ab].go", "src/c.go", false},
		{"*", "anything/at/all", true},

		// any depth
		{"**", "a", true},
		{"**", "a/b/c", true},
		{"**/*.rs", "main.rs", true},
		{"**/*.rs", "a/b/main.rs", true},
		{"**/*.rs", "a/b/main.go", false},
		{"src/**/test.rs", "src/test.rs", true},
		{"src/**/test.rs", "src/a/b/test.rs", true},
		{"src/**/test.rs", "lib/a/test.rs", false},
		{"a/**/b/**/c", "a/x/b/y/z/c", true},
		{"a/**/b/**/c", "a/x/y/c", false},
		{"a/**/**/b", "a/b", true},
		{"**/secret/**", "deep/in/secret/file", true},
		{"**/secret/**", "deep/in/secrets/file", false},

		// invalid input never matches
		{"", "a", false},
		{"[", "[", false},
		{"../x", "x", false},
		{"x", "../x", false},
		{"x", "", false},
		{"x", "..", false},
	}
	for _, tt := range tests {
		if got := Match(tt.glob, tt.path); got != tt.want {
			t.Errorf("Match(%q, %q) = %v, want %v", tt.glob, tt.path, got, tt.want)
		}
	}
}

func TestMatchPathologicalPatternIsFast(t *testing.T) {
	// Many "**" segments must not explode combinatorially (memoized).
	pattern := "**/a/**/a/**/a/**/a/**/a/**/a/**/a/**/b"
	p := ""
	for i := 0; i < 60; i++ {
		p += "a/"
	}
	p += "c"
	if Match(pattern, p) {
		t.Fatal("unexpected match")
	}
}

func TestSpecificity(t *testing.T) {
	tests := []struct {
		name  string
		globs []string
		path  string
		want  string
	}{
		{"exact file beats directory", []string{"src/**", "src/main.rs"}, "src/main.rs", "src/main.rs"},
		{"exact file beats its /** form", []string{"src/main.rs/**", "src/main.rs"}, "src/main.rs", "src/main.rs"},
		{"deeper directory beats shallower", []string{"services", "services/billing"}, "services/billing/a.rs", "services/billing"},
		{"deeper glob dir beats shallower", []string{"vendor/**", "vendor/sdk/**"}, "vendor/sdk/x.go", "vendor/sdk/**"},
		{"star-suffix beats double star", []string{"src/**", "src/*.rs"}, "src/main.rs", "src/*.rs"},
		{"longer literal prefix wins", []string{"src/*", "src/m*"}, "src/main.rs", "src/m*"},
		{"fewer wildcards on equal prefix", []string{"src/*/*.rs", "src/*"}, "src/a/b.rs", "src/*"},
		{"more literals on equal prefix and wildcards", []string{"src/*", "src/*.rs"}, "src/a.rs", "src/*.rs"},
		{"catch-all loses", []string{"**", "docs"}, "docs/a.md", "docs"},
		{"catch-all applies when nothing else", []string{"**", "docs"}, "src/a.rs", "**"},
		{"bare dir beats trailing-slash form", []string{"vendor/", "vendor"}, "vendor/a", "vendor"},
		{"no match", []string{"docs"}, "src/a.rs", ""},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			var rules []domain.LicenseRule
			for _, g := range tt.globs {
				rules = append(rules, domain.LicenseRule{PathGlob: g, SPDX: "MIT"})
			}
			// Order of input must not matter.
			for _, rs := range [][]domain.LicenseRule{rules, reversed(rules)} {
				got, ok := Resolve(rs, tt.path)
				if tt.want == "" {
					if ok {
						t.Fatalf("Resolve matched %q, want none", got.PathGlob)
					}
					continue
				}
				if !ok || got.PathGlob != tt.want {
					t.Fatalf("Resolve(%v, %q) = %q (ok=%v), want %q", rs, tt.path, got.PathGlob, ok, tt.want)
				}
			}
		})
	}
}

func TestResolveIgnoresMalformedAndInvalidPaths(t *testing.T) {
	rules := []domain.LicenseRule{{PathGlob: "src/[", SPDX: "MIT"}, {PathGlob: "src", SPDX: "MIT"}}
	if r, ok := Resolve(rules, "src/["); !ok || r.PathGlob != "src" {
		t.Fatalf("got %q %v", r.PathGlob, ok)
	}
	if _, ok := Resolve(rules, "../src/a"); ok {
		t.Fatal("escaping path must not resolve")
	}
}

func reversed(in []domain.LicenseRule) []domain.LicenseRule {
	out := make([]domain.LicenseRule, len(in))
	for i, r := range in {
		out[len(in)-1-i] = r
	}
	return out
}

func TestSpecificityKeys(t *testing.T) {
	tests := []struct {
		glob                   string
		prefix, wild, literals int
	}{
		{"vendor/**", 6, 1, 6},
		{"vendor", 6, 0, 6},
		{"src/*.rs", 4, 1, 6},
		{"**", 0, 1, 0},
		{"a/?b/[xy]*", 2, 3, 2},
	}
	for _, tt := range tests {
		g, err := compileGlob(tt.glob)
		if err != nil {
			t.Fatal(err)
		}
		if g.prefix != tt.prefix || g.wild != tt.wild || g.literals != tt.literals {
			t.Errorf("%q: prefix=%d wild=%d literals=%d, want %d %d %d",
				tt.glob, g.prefix, g.wild, g.literals, tt.prefix, tt.wild, tt.literals)
		}
	}
}
