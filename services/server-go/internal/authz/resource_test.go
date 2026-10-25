package authz

import (
	"testing"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

func TestResourceStrings(t *testing.T) {
	tests := []struct{ got, want string }{
		{TenantResource("acme"), "wt:tenant/acme"},
		{TreeResource("acme", "repo"), "wt:tree/acme/repo"},
		{BranchResource("acme", "repo", "release/1.0"), "wt:branch/acme/repo/release/1.0"},
		{PathResource("acme", "repo", "/src/main.go"), "wt:path/acme/repo/src/main.go"},
	}
	for _, tt := range tests {
		if tt.got != tt.want {
			t.Errorf("got %q, want %q", tt.got, tt.want)
		}
		if err := validateResourcePattern(tt.got); err != nil {
			t.Errorf("%q does not validate: %v", tt.got, err)
		}
	}
}

func TestResourcePatternMatches(t *testing.T) {
	tests := []struct {
		pattern, res string
		want         bool
	}{
		{"*", "wt:tree/acme/repo", true},
		{"wt:tree/acme/repo", "wt:tree/acme/repo", true},
		{"wt:tree/acme/repo", "wt:tree/acme/repo2", false},
		{"wt:tree/acme/*", "wt:tree/acme/repo", true},
		{"wt:tree/*/repo", "wt:tree/beta/repo", true},
		{"wt:tree/acme/*", "wt:branch/acme/repo/main", false}, // kinds differ
		{"wt:tree/acme/re*o", "wt:tree/acme/repo", true},
		{"wt:tree/acme/*-svc", "wt:tree/acme/api-svc", true},
		{"wt:tree/acme/*-svc", "wt:tree/acme/api-svc-old", false},
		{"wt:tree/acme/a*b*c", "wt:tree/acme/abxbc", true},
		{"wt:tree/acme/a*b*b", "wt:tree/acme/ab", false},
		{"wt:branch/acme/repo/*", "wt:branch/acme/repo/release/1.0", false},
		{"wt:branch/acme/repo/**", "wt:branch/acme/repo/release/1.0", true},
		{"wt:branch/acme/repo/release/**", "wt:branch/acme/repo/release", true}, // ** matches zero segments
		{"wt:branch/acme/**/main", "wt:branch/acme/repo/main", true},
		{"wt:branch/acme/**/main", "wt:branch/acme/repo/x/main", true},
		{"wt:branch/acme/**/main", "wt:branch/acme/repo/maintenance", false},
		{"wt:path/acme/repo/**/**/x.go", "wt:path/acme/repo/x.go", true},
		{"wt:path/**", "wt:path/acme/repo/deep/file", true},
		{"wt:path/acme/repo/docs/*.md", "wt:path/acme/repo/docs/a.md", true},
		{"wt:path/acme/repo/docs/*.md", "wt:path/acme/repo/docs/sub/a.md", false},
	}
	for _, tt := range tests {
		p, err := parseResource(tt.pattern)
		if err != nil {
			t.Fatalf("parse %q: %v", tt.pattern, err)
		}
		r, err := parseResource(tt.res)
		if err != nil {
			t.Fatalf("parse %q: %v", tt.res, err)
		}
		if got := resourcePatternMatches(p, r); got != tt.want {
			t.Errorf("%q matches %q = %v, want %v", tt.pattern, tt.res, got, tt.want)
		}
	}
}

func TestActionPatternMatches(t *testing.T) {
	tests := []struct {
		pattern string
		act     domain.Action
		want    bool
	}{
		{"*", domain.ActServerAdmin, true},
		{"tree:*", domain.ActTreeAdmin, true},
		{"tree:*", domain.ActTenantRead, false},
		{"branch:*", domain.ActBranchPush, true},
		{"tree:read", domain.ActTreeRead, true},
		{" tree:read ", domain.ActTreeRead, true},
		{"tree:read", domain.ActTreeWrite, false},
		{"tree", domain.ActTreeRead, false},
	}
	for _, tt := range tests {
		if got := actionPatternMatches(tt.pattern, tt.act); got != tt.want {
			t.Errorf("actionPatternMatches(%q, %s) = %v, want %v", tt.pattern, tt.act, got, tt.want)
		}
	}
}

func TestRequestResources(t *testing.T) {
	tr := &domain.Tree{ID: "t", TenantID: "x", Name: "repo"}
	base, branch := requestResources("acme", domain.Resource{Tree: tr, Branch: "feat/a", Path: "/src//a.go"})
	if len(base) != 3 || base[0].kind != kindTenant || base[1].kind != kindTree || base[2].kind != kindPath {
		t.Fatalf("base = %+v", base)
	}
	if got := base[2].segs; len(got) != 4 || got[2] != "src" || got[3] != "a.go" {
		t.Fatalf("path segments = %v", got)
	}
	if len(branch) != 1 || len(branch[0].segs) != 4 || branch[0].segs[3] != "a" {
		t.Fatalf("branch = %+v", branch)
	}
	base, branch = requestResources("acme", domain.Resource{})
	if len(base) != 1 || branch != nil {
		t.Fatalf("tenant-only = %+v %+v", base, branch)
	}
}
