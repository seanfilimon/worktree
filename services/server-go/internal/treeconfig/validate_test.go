package treeconfig

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"testing"
)

// conformanceDir holds the cases shared with the Rust validator.
var conformanceDir = filepath.Join("..", "..", "..", "..", "crates", "worktree-protocol", "testdata", "config_as_code")

// TestConformanceCases validates the shared cases and compares codes and
// positions with the .expected files (the Rust test reads the same files).
func TestConformanceCases(t *testing.T) {
	entries, err := os.ReadDir(conformanceDir)
	if err != nil {
		t.Skipf("conformance cases not available: %v", err)
	}
	n := 0
	for _, e := range entries {
		name := e.Name()
		if !strings.HasSuffix(name, ".toml") {
			continue
		}
		parts := strings.Split(strings.TrimSuffix(name, ".toml"), ".")
		if len(parts) != 2 {
			t.Fatalf("bad case file name %s", name)
		}
		caseName, kind := parts[0], parts[1]
		src, err := os.ReadFile(filepath.Join(conformanceDir, name))
		if err != nil {
			t.Fatal(err)
		}
		want, err := os.ReadFile(filepath.Join(conformanceDir, caseName+".expected"))
		if err != nil {
			t.Fatal(err)
		}
		path := Dir + "/" + kind + ".toml"
		got := Validate(map[string][]byte{path: src}).Diagnostics
		var gotLines []string
		for _, d := range got {
			if d.File != path {
				t.Errorf("%s: diagnostic for %s", caseName, d.File)
			}
			gotLines = append(gotLines, fmt.Sprintf("%d:%d %s", d.Line, d.Col, d.Code))
		}
		wantLines := strings.Fields(strings.ReplaceAll(string(want), "\r", ""))
		var wl []string
		for i := 0; i+1 < len(wantLines); i += 2 {
			wl = append(wl, wantLines[i]+" "+wantLines[i+1])
		}
		if !matchExpected(gotLines, wl) {
			t.Errorf("%s: got\n  %s\nwant\n  %s\n(%v)", caseName, strings.Join(gotLines, "\n  "), strings.Join(wl, "\n  "), got)
		}
		n++
	}
	if n < 5 {
		t.Fatalf("only %d conformance cases found", n)
	}
}

func matchExpected(got, want []string) bool {
	if len(got) != len(want) {
		return false
	}
	for i := range got {
		g, w := strings.Fields(got[i]), strings.Fields(want[i])
		if g[1] != w[1] {
			return false
		}
		gp, wp := strings.Split(g[0], ":"), strings.Split(w[0], ":")
		for j := range 2 {
			if wp[j] != "?" && wp[j] != gp[j] {
				return false
			}
		}
	}
	return true
}

func codes(ds []Diagnostic) []string {
	out := make([]string, 0, len(ds))
	for _, d := range ds {
		out = append(out, d.Code)
	}
	sort.Strings(out)
	return out
}

func TestValidateBranchesAndProtections(t *testing.T) {
	p := Validate(map[string][]byte{
		BranchesFile:    []byte("[[branch]]\nname = \"main\"\ndescription = \"  Prod  \"\nowner = \"user:alice\"\n"),
		ProtectionsFile: []byte("[[protection]]\npattern = \"main\"\nrequired_approvals = 2\nmerge_methods = [\"squash\", \"merge\", \"squash\"]\nallowed_pushers = \"alice\"\n"),
		"README.md":     []byte("ignored"),
	})
	if p.HasErrors() {
		t.Fatalf("diagnostics: %v", p.Diagnostics)
	}
	if len(p.Branches) != 1 || p.Branches[0] != (BranchDecl{Name: "main", Description: "Prod", Owner: "user:alice"}) {
		t.Errorf("branches = %+v", p.Branches)
	}
	pr := p.Protections[0]
	if pr.Pattern != "main" || pr.RequiredApprovals != 2 || strings.Join(pr.MergeMethods, ",") != "merge,squash" || strings.Join(pr.AllowedPushers, ",") != "alice" {
		t.Errorf("protection = %+v", pr)
	}
}

func TestValidateAccessPolicies(t *testing.T) {
	src := `[[policy]]
name = "no-push"
[[policy.statement]]
effect = "Deny"
principals = "user:bob"
actions = ["sync:push"]
resources = ["wt:branch/acme/web/main"]
conditions = { mfa = true, not_before = 2026-01-01T00:00:00Z, time_window = { days = ["mon"], start = "09:00", end = "17:00" } }
`
	p := Validate(map[string][]byte{PoliciesFile: []byte(src)})
	if p.HasErrors() {
		t.Fatalf("diagnostics: %v", p.Diagnostics)
	}
	if got := codes(p.Diagnostics); strings.Join(got, ",") != WAccessAlias {
		t.Errorf("codes = %v", got)
	}
	var doc map[string]any
	if err := json.Unmarshal(p.Access, &doc); err != nil {
		t.Fatal(err)
	}
	st := doc["policies"].([]any)[0].(map[string]any)["statements"].([]any)[0].(map[string]any)
	if st["effect"] != "deny" || st["actions"].([]any)[0] != "branch:push" || st["principals"].([]any)[0] != "user:bob" {
		t.Errorf("statement = %v", st)
	}
	cond := st["conditions"].(map[string]any)
	if cond["require_mfa"] != true || cond["not_before"] != "2026-01-01T00:00:00Z" || cond["time_window"] == nil {
		t.Errorf("conditions = %v", cond)
	}

	bad := Validate(map[string][]byte{
		PoliciesFile: []byte("[[policy]]\nname = \"x\"\nwhat = 1\n[[policy.statement]]\nconditions = 3\n"),
		RolesFile:    []byte("[[role]]\nname = \"r\"\n"),
	})
	if got := strings.Join(codes(bad.Diagnostics), ","); got != "E2002,E2002,E2013" {
		t.Errorf("codes = %s (%v)", got, bad.Diagnostics)
	}
	if bad.Access != nil {
		t.Error("an invalid document compiled")
	}
	syn := Validate(map[string][]byte{PoliciesFile: []byte("[[policy]\n")})
	if got := codes(syn.Diagnostics); len(got) != 1 || got[0] != EAccessSyntax {
		t.Errorf("codes = %v", got)
	}
	local := Validate(map[string][]byte{PoliciesFile: []byte("[[policy]]\nname = \"x\"\n[[policy.statement]]\nconditions = { not_after = 2026-01-01T00:00:00 }\n")})
	if got := codes(local.Diagnostics); len(got) != 1 || got[0] != "E2011" {
		t.Errorf("codes = %v", got)
	}
}

func TestValidateLimitsAndMisc(t *testing.T) {
	var b strings.Builder
	for i := range MaxBranches + 1 {
		fmt.Fprintf(&b, "[[branch]]\nname = \"b%d\"\n", i)
	}
	p := Validate(map[string][]byte{BranchesFile: []byte(b.String())})
	if got := codes(p.Diagnostics); len(got) != 1 || got[0] != ELimit {
		t.Errorf("codes = %v", got)
	}
	big := Validate(map[string][]byte{ConfigFile: make([]byte, MaxFileSize+1)})
	if got := codes(big.Diagnostics); len(got) != 1 || got[0] != ELimit {
		t.Errorf("codes = %v", got)
	}
	long := Validate(map[string][]byte{BranchesFile: []byte("[[branch]]\nname = \"m\"\ndescription = \"" + strings.Repeat("é", MaxDescription+1) + "\"\n")})
	if got := codes(long.Diagnostics); len(got) != 1 || got[0] != ELimit {
		t.Errorf("codes = %v", got)
	}
	for owner, ok := range map[string]bool{"user:alice": true, "team:core-dev": true, "user:": false, "team:Core": false, "group:x": false, "alice": false} {
		if (CheckOwner(owner) == "") != ok {
			t.Errorf("CheckOwner(%q) = %q", owner, CheckOwner(owner))
		}
	}
	for _, p := range []string{ConfigFile, BranchesFile, ProtectionsFile, PoliciesFile, AccessDir + "x/y"} {
		if !IsConfigPath(p) {
			t.Errorf("IsConfigPath(%q) = false", p)
		}
	}
	for _, p := range []string{".wt-tree/ignore", ".wt-tree/hooks/x", AccessDir, "branches.toml"} {
		if IsConfigPath(p) {
			t.Errorf("IsConfigPath(%q) = true", p)
		}
	}
	d := newDiag(EOwner, BranchesFile, Pos{3, 7}, "owner %s", "x")
	if d.String() != ".wt-tree/branches.toml:3:7: E3008 owner x" || !d.IsError() {
		t.Errorf("String() = %q", d.String())
	}
	if w := newDiag(WSection, ConfigFile, Pos{}, "w"); w.IsError() || w.String() != ".wt-tree/config.toml: W3103 w" {
		t.Errorf("warning = %+v", w)
	}
	many := make([]Diagnostic, 12)
	for i := range many {
		many[i] = newDiag(ESyntax, ConfigFile, Pos{i + 1, 1}, "e")
	}
	if s := summarize(many); !strings.Contains(s, "... and 2 more") {
		t.Errorf("summarize = %q", s)
	}
}
