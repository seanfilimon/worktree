package review

import (
	"errors"
	"reflect"
	"strings"
	"testing"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

func TestMatchPattern(t *testing.T) {
	tests := []struct {
		pattern, branch string
		want            bool
	}{
		{"main", "main", true},
		{"main", "mainline", false},
		{"main", "dev/main", false},
		{"", "main", false},
		{"main", "", false},
		{"release/*", "release/1.0", true},
		{"release/*", "release/1.0/hotfix", false},
		{"release/*", "release", false},
		{"release-*", "release-2", true},
		{"rel?ase", "release", true},
		{"[a-c]*", "bugfix", true},
		{"[a-c]*", "dev", false},
		{"*", "main", true},
		{"*", "feature/x", false},
		{"**", "main", true},
		{"**", "feature/x/y", true},
		{"release/**", "release/1.0", true},
		{"release/**", "release/1.x/rc1", true},
		{"release/**", "release", false},
		{"release/**", "releases/1.0", false},
		{"team-*/**", "team-a/feat/x", true},
		{"team-*/**", "teamb/feat", false},
		{"a/**/b", "a/b", true},
		{"a/**/b", "a/x/y/b", true},
		{"a/**/b", "a/x/y/c", false},
		{"a/**/**/b", "a/x/b", true},
		{"**/hotfix", "hotfix", true},
		{"**/hotfix", "release/1/hotfix", true},
		{"**/hotfix", "release/1/hotfixes", false},
		{"release/**x", "release/1x", true}, // "**x" is not a globstar segment
		{"release/**x", "release/a/1x", false},
		// malformed patterns only match literally
		{"[", "[", true},
		{"[", "a", false},
		{"rel/[/**", "rel/a/b", false},
	}
	for _, tt := range tests {
		if got := MatchPattern(tt.pattern, tt.branch); got != tt.want {
			t.Errorf("MatchPattern(%q, %q) = %v, want %v", tt.pattern, tt.branch, got, tt.want)
		}
	}
}

func TestMatchPatternManyGlobstarsIsFast(t *testing.T) {
	pattern := strings.Repeat("**/", 40) + "z"
	branch := strings.Repeat("a/", 60) + "b"
	if MatchPattern(pattern, branch) {
		t.Fatal("unexpected match")
	}
}

func TestResolve(t *testing.T) {
	rules := []domain.BranchProtection{
		{Pattern: "main", NoDirectPush: true, RequiredApprovals: 1},
		{Pattern: "*", RequiredApprovals: 2, AllowedPushers: []string{"alice", "bob", "carol"}},
		{Pattern: "ma*", NoDelete: true, AllowedPushers: []string{"bob", " carol ", ""}},
		{Pattern: "release/**", RequiredApprovals: -3, AllowedPushers: []string{"zed"}},
		{Pattern: "release/1.0", AllowedPushers: []string{"amy"}},
		{Pattern: "dev/**", AllowedPushers: []string{"", "  "}},
	}
	tests := []struct {
		branch string
		want   Effective
	}{
		{"main", Effective{
			Branch: "main", Patterns: []string{"main", "*", "ma*"},
			NoDirectPush: true, RequiredApprovals: 2, NoDelete: true,
			RestrictPushers: true, AllowedPushers: []string{"bob", "carol"}, RequiredChecks: []string{},
		}},
		{"feature", Effective{
			Branch: "feature", Patterns: []string{"*"}, RequiredApprovals: 2,
			RestrictPushers: true, AllowedPushers: []string{"alice", "bob", "carol"}, RequiredChecks: []string{},
		}},
		{"release/2.0", Effective{
			Branch: "release/2.0", Patterns: []string{"release/**"},
			RestrictPushers: true, AllowedPushers: []string{"zed"}, RequiredChecks: []string{},
		}},
		{"release/1.0", Effective{ // disjoint lists: nobody may push
			Branch: "release/1.0", Patterns: []string{"release/**", "release/1.0"},
			RestrictPushers: true, AllowedPushers: []string{}, RequiredChecks: []string{},
		}},
		{"dev/x", Effective{ // blank names add no restriction
			Branch: "dev/x", Patterns: []string{"dev/**"}, AllowedPushers: []string{}, RequiredChecks: []string{},
		}},
		{"hotfix/a/b", Effective{Branch: "hotfix/a/b", Patterns: []string{}, AllowedPushers: []string{}, RequiredChecks: []string{}}},
	}
	for _, tt := range tests {
		t.Run(tt.branch, func(t *testing.T) {
			got := Resolve(rules, tt.branch)
			if !reflect.DeepEqual(got, tt.want) {
				t.Errorf("Resolve(%q)\n got %+v\nwant %+v", tt.branch, got, tt.want)
			}
		})
	}
}

func TestEffectiveHelpers(t *testing.T) {
	none := Resolve(nil, "main")
	if none.Protected() || !none.MayPush("anyone") || !none.MayPush("") {
		t.Errorf("unprotected branch: %+v", none)
	}
	eff := Resolve([]domain.BranchProtection{{Pattern: "main", AllowedPushers: []string{"carol", "alice"}}}, "main")
	if !eff.Protected() {
		t.Error("Protected() = false")
	}
	for user, want := range map[string]bool{"alice": true, "carol": true, "bob": false, "": false} {
		if got := eff.MayPush(user); got != want {
			t.Errorf("MayPush(%q) = %v, want %v", user, got, want)
		}
	}
}

func TestValidatePattern(t *testing.T) {
	tests := []struct {
		pattern string
		ok      bool
	}{
		{"main", true},
		{"release/*", true},
		{"release/**", true},
		{"a/**/b", true},
		{"[a-z]*", true},
		{"", false},
		{"   ", false},
		{"[", false},
		{"release/[a-", false},
		{"release//x", false},
		{"/main", false},
		{"main/", false},
	}
	for _, tt := range tests {
		err := ValidatePattern(tt.pattern)
		if (err == nil) != tt.ok {
			t.Errorf("ValidatePattern(%q) = %v, want ok=%v", tt.pattern, err, tt.ok)
		}
		if err != nil && !errors.Is(err, domain.ErrInvalid) {
			t.Errorf("ValidatePattern(%q) error %v does not wrap ErrInvalid", tt.pattern, err)
		}
	}
}
