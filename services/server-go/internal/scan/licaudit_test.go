package scan

import (
	"strings"
	"testing"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

func TestSPDXListIsEmbedded(t *testing.T) {
	if SPDXListVersion() == "" || len(spdx.licenses) < 500 || len(spdx.exceptions) < 50 || len(spdx.deprecated) < 20 {
		t.Fatalf("list %s: %d licenses, %d deprecated, %d exceptions", SPDXListVersion(), len(spdx.licenses), len(spdx.deprecated), len(spdx.exceptions))
	}
	for _, id := range []string{"MIT", "Apache-2.0", "GPL-3.0-only", "BSD-3-Clause", "MPL-2.0", "CC0-1.0", "Unlicense"} {
		if spdx.licenses[strings.ToLower(id)] != id {
			t.Errorf("%s missing", id)
		}
	}
}

func TestStrictSPDX(t *testing.T) {
	for _, ok := range []string{
		"MIT",
		"MIT OR Apache-2.0",
		"(MIT AND BSD-3-Clause) OR GPL-2.0-or-later",
		"GPL-2.0-only WITH Classpath-exception-2.0",
		"Apache-2.0 WITH LLVM-exception",
		"LicenseRef-Acme-Proprietary",
		"DocumentRef-spdx:LicenseRef-x",
		"Proprietary",
		"LGPL-2.1-or-later AND MIT",
		"EPL-1.0+",
	} {
		if p := StrictSPDX(ok); len(p) != 0 {
			t.Errorf("StrictSPDX(%q) = %v", ok, p)
		}
	}
	for expr, want := range map[string]string{
		"FooBar-1.0":                       "not on the SPDX License List",
		"mit":                              `should be written "MIT"`,
		"GPL-2.0":                          "deprecated",
		"GPL-2.0+":                         "deprecated",
		"MIT WITH Nope-exception":          "not an exception",
		"GPL-2.0-only WITH llvm-exception": `should be written "LLVM-exception"`,
		"MIT and Apache-2.0":               "unexpected",
		"":                                 "empty",
	} {
		p := StrictSPDX(expr)
		if len(p) == 0 || !strings.Contains(strings.Join(p, "; "), want) {
			t.Errorf("StrictSPDX(%q) = %v, want a problem containing %q", expr, p, want)
		}
	}
}

func TestAuditLicenses(t *testing.T) {
	f := newFixture(t)
	s1 := f.snap("",
		entry{path: "README.md", content: "readme\n"},
		entry{path: "src/lib.rs", content: "lib\n"},
		entry{path: "src/main.rs", content: "main\n"},
		entry{path: "vendor/blob.c", content: "blob\n"},
		entry{path: "vendor/open/x.c", content: "x\n"},
		entry{path: "tools/run.sh", content: "run\n"},
	)
	f.store_(s1)
	past := time.Now().Add(-time.Hour)
	for _, r := range []domain.LicenseRule{
		{PathGlob: "src/**", SPDX: "MIT"},
		{PathGlob: "vendor/**", SPDX: "LicenseRef-Proprietary"},
		{PathGlob: "vendor/open/**", SPDX: "LicenseRef-Proprietary", Grant: "read-only"},
		{PathGlob: "vendor/blob.c", SPDX: "LicenseRef-Proprietary", Grant: "modify", Grantee: "partner"},
		{PathGlob: "tools/**", SPDX: "GPL-2.0"},
		{PathGlob: "docs/**", SPDX: "MIT", ExpiresAt: &past},
	} {
		r.TreeID = f.tree.ID
		must(t, f.store.PutLicenseRule(f.ctx, &r))
	}
	a, err := f.svc.AuditLicenses(f.ctx, f.tree, "")
	must(t, err)
	if a.SnapshotID != s1.ID || a.Files != 6 || a.Ref != "main" || a.SPDXListVersion == "" {
		t.Fatalf("audit = %+v", a)
	}
	if a.UnlicensedCount != 1 || len(a.Unlicensed) != 1 || a.Unlicensed[0] != "README.md" {
		t.Fatalf("unlicensed = %v", a.Unlicensed)
	}
	byID := map[string]LicenseUsage{}
	for _, u := range a.Licenses {
		byID[u.SPDX] = u
	}
	if byID["MIT"].Files != 2 || byID["LicenseRef-Proprietary"].Files != 2 || !byID["LicenseRef-Proprietary"].Proprietary ||
		byID["GPL-2.0"].Files != 1 || len(byID["LicenseRef-Proprietary"].Globs) != 2 {
		t.Fatalf("licenses = %+v", a.Licenses)
	}
	// vendor/open/x.c is readable by every tenant; vendor/blob.c by partner.
	if a.ExposedCount != 2 {
		t.Fatalf("exposed = %+v", a.Exposed)
	}
	found := map[string]string{}
	for _, e := range a.Exposed {
		found[e.Path] = e.Grantee + "/" + e.Grant
	}
	if found["vendor/open/x.c"] != "/read-only" || found["vendor/blob.c"] != "partner/modify" {
		t.Fatalf("exposed = %+v", a.Exposed)
	}
	if a.InvalidRules != 1 || len(a.Rules) != 6 {
		t.Fatalf("rules = %+v", a.Rules)
	}
	for _, r := range a.Rules {
		switch r.PathGlob {
		case "tools/**":
			if r.Valid || len(r.Problems) == 0 || r.Files != 1 {
				t.Errorf("tools rule = %+v", r)
			}
		case "docs/**":
			if !r.Expired || r.Files != 0 {
				t.Errorf("docs rule = %+v", r)
			}
		case "vendor/blob.c":
			if r.Grantee != "partner" || r.Files != 1 {
				t.Errorf("grantee rule = %+v", r)
			}
		}
	}
	if _, err := f.svc.AuditLicenses(f.ctx, f.tree, "missing"); err == nil {
		t.Fatal("unknown ref accepted")
	}
}
