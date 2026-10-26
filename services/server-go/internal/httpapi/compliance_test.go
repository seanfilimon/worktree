package httpapi

import (
	"context"
	"io"
	"log/slog"
	"strings"
	"testing"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
	"github.com/seanfilimon/worktree/services/server-go/internal/scan"
)

// compliance (§16.7): register the component schemas of the compliance
// routes with the OpenAPI schema checks (kept here so that the shared
// openAPITypes table needs no edit).
func init() {
	for name, v := range map[string]any{
		"ComplianceSettings":        scan.Settings{},
		"ComplianceSettingsRequest": scan.SettingsPatch{},
		"SecretFinding":             scan.Finding{},
		"SecretFindingList":         scan.FindingList{},
		"ResolveFindingRequest":     scan.ResolveRequest{},
		"AllowEntry":                scan.AllowEntry{},
		"AllowList":                 scan.AllowList{},
		"AllowEntryRequest":         scan.AllowRequest{},
		"ComplianceScanRequest":     scan.ScanRequest{},
		"ComplianceScanResult":      scan.ScanResult{},
		"ScanMatch":                 scan.ScanMatch{},
		"LicenseAudit":              scan.LicenseAudit{},
		"LicenseUsage":              scan.LicenseUsage{},
		"ExposedPath":               scan.ExposedPath{},
		"LicenseRuleCheck":          scan.RuleCheck{},
		"ComplianceSummary":         scan.Summary{},
		"ComplianceModeCounts":      scan.ModeCounts{},
		"TreeCompliance":            scan.TreeCompliance{},
		"RecentSecretFinding":       scan.RecentFinding{},
	} {
		openAPITypes[name] = v
	}
	openAPICommentDirs = append(openAPICommentDirs, "../scan")
}

// newComplianceFixture wires a scan.Service over depsStore.
func newComplianceFixture(t *testing.T) (*fixture, *scan.Service) {
	t.Helper()
	var svc *scan.Service
	f := newFixture(t, func(d *Deps) {
		ds := &depsStore{manageStore: &manageStore{fakeStore: d.Store.(*fakeStore)}, DocStore: meta.NewMemory()}
		d.Store = ds
		svc = scan.New(scan.Deps{Store: ds, Objects: d.Objects, Authz: d.Authz, Bus: d.Bus, Audit: d.Audit,
			Logger: slog.New(slog.NewTextHandler(io.Discard, nil))})
		d.Scan = svc
	})
	return f, svc
}

func TestComplianceEndpoints(t *testing.T) {
	f, svc := newComplianceFixture(t)
	do := func(method, path, as string, body any, want int) []byte {
		t.Helper()
		rec := f.do(t, method, path, as, body)
		if rec.Code != want {
			t.Fatalf("%s %s as %q: %d, want %d; %s", method, path, as, rec.Code, want, rec.Body.String())
		}
		return rec.Body.Bytes()
	}
	const base = "/api/v1/trees/acme/web/compliance"

	// Settings: defaults, validation, permissions.
	st := decode[scan.Settings](t, f.do(t, "GET", base+"/settings", "carol", nil))
	if st.Mode != scan.ModeWarn || st.TreeID != f.web.ID {
		t.Fatalf("defaults = %+v", st)
	}
	do("PUT", base+"/settings", "bob", map[string]any{"mode": "block"}, 403)
	do("PUT", base+"/settings", "alice", map[string]any{"mode": "loud"}, 400)
	do("PUT", base+"/settings", "", map[string]any{"mode": "block"}, 401)
	st = decode[scan.Settings](t, f.do(t, "PUT", base+"/settings", "alice", map[string]any{"mode": "block", "exclude": []string{"fixtures/**"}}))
	if st.Mode != scan.ModeBlock || len(st.Exclude) != 1 || st.UpdatedBy != "alice" {
		t.Fatalf("updated = %+v", st)
	}

	// A blocked push records a finding.
	key := "AK" + "IA" + "Z7Q4W2E8R5T1Y3U6"
	mh := manifest(t, f.objs, map[string]string{"README.md": "# web v2\n", "config/aws.env": "AWS_ACCESS_KEY_ID=" + key + "\n"})
	snap := &domain.Snapshot{ID: "snap-leak", TreeID: f.web.ID, BranchName: "main", Parents: []string{f.s2.ID}, ManifestHash: mh}
	err := svc.CheckPushPlan(context.Background(), f.users["bob"], &f.web,
		&domain.PushPlan{Branch: "main", OldTip: f.s2.ID, NewTip: snap.ID, NewSnapshots: []*domain.Snapshot{snap}})
	if err == nil || !strings.Contains(err.Error(), "config/aws.env:1") {
		t.Fatalf("push not blocked: %v", err)
	}

	do("GET", base+"/findings", "carol", nil, 403)
	do("GET", base+"/findings?status=weird", "bob", nil, 400)
	list := decode[scan.FindingList](t, f.do(t, "GET", base+"/findings", "bob", nil))
	if len(list.Findings) != 1 || list.Open != 1 || list.Findings[0].Action != scan.ActionBlocked {
		t.Fatalf("findings = %+v", list)
	}
	fd := list.Findings[0]
	if strings.Contains(fd.Excerpt, key) {
		t.Fatal("excerpt leaks the secret")
	}

	// Resolve and reopen.
	do("POST", base+"/findings/"+fd.ID+"/resolve", "bob", map[string]any{"resolution": "revoked"}, 403)
	do("POST", base+"/findings/"+fd.ID+"/resolve", "alice", map[string]any{"resolution": "nope"}, 400)
	do("POST", base+"/findings/missing/resolve", "alice", map[string]any{"resolution": "revoked"}, 404)
	res := decode[scan.Finding](t, f.do(t, "POST", base+"/findings/"+fd.ID+"/resolve", "alice", map[string]any{"resolution": "revoked", "note": "rotated"}))
	if res.Status != scan.StatusResolved || res.ResolvedBy != "alice" {
		t.Fatalf("resolved = %+v", res)
	}
	if l := decode[scan.FindingList](t, f.do(t, "GET", base+"/findings?status=resolved", "bob", nil)); len(l.Findings) != 1 {
		t.Fatalf("resolved list = %+v", l)
	}
	if l := decode[scan.FindingList](t, f.do(t, "GET", base+"/findings?status=all&limit=5", "bob", nil)); len(l.Findings) != 1 || l.Resolved != 1 {
		t.Fatalf("all list = %+v", l)
	}
	decode[scan.Finding](t, f.do(t, "POST", base+"/findings/"+fd.ID+"/resolve", "alice", map[string]any{"reopen": true}))

	// Allowlist.
	do("POST", base+"/allowlist", "bob", map[string]any{"fingerprint": fd.Fingerprint, "reason": "fixture"}, 403)
	do("POST", base+"/allowlist", "alice", map[string]any{"fingerprint": "zz", "reason": "fixture"}, 400)
	e := decode[scan.AllowEntry](t, f.do(t, "POST", base+"/allowlist", "alice", map[string]any{"fingerprint": fd.Fingerprint, "reason": "fixture"}))
	if e.Fingerprint != fd.Fingerprint || e.CreatedBy != "alice" {
		t.Fatalf("entry = %+v", e)
	}
	al := decode[scan.AllowList](t, f.do(t, "GET", base+"/allowlist", "bob", nil))
	if len(al.Entries) != 1 {
		t.Fatalf("allowlist = %+v", al)
	}
	if err := svc.CheckPushPlan(context.Background(), f.users["bob"], &f.web,
		&domain.PushPlan{Branch: "main", OldTip: f.s2.ID, NewTip: snap.ID, NewSnapshots: []*domain.Snapshot{snap}}); err != nil {
		t.Fatalf("allowlisted push blocked: %v", err)
	}
	do("DELETE", base+"/allowlist", "alice", nil, 400)
	do("DELETE", base+"/allowlist?fingerprint="+fd.Fingerprint, "bob", nil, 403)
	do("DELETE", base+"/allowlist?fingerprint="+fd.Fingerprint, "alice", nil, 204)
	do("DELETE", base+"/allowlist?fingerprint="+fd.Fingerprint, "alice", nil, 404)

	// On-demand scan and license audit of the default branch.
	scanRes := decode[scan.ScanResult](t, f.do(t, "POST", base+"/scan", "alice", map[string]any{"ref": "v1"}))
	if scanRes.SnapshotID != f.s1.ID || len(scanRes.Matches) != 0 {
		t.Fatalf("scan = %+v", scanRes)
	}
	if r := decode[scan.ScanResult](t, f.do(t, "POST", base+"/scan", "alice", nil)); r.SnapshotID != f.s2.ID || r.Files == 0 {
		t.Fatalf("scan default = %+v", r)
	}
	do("POST", base+"/scan", "bob", nil, 403)
	do("POST", base+"/scan", "alice", map[string]any{"ref": "nope"}, 404)
	audit := decode[scan.LicenseAudit](t, f.do(t, "GET", base+"/licenses", "alice", nil))
	if audit.SnapshotID != f.s2.ID || audit.ExposedCount != 1 || audit.Exposed[0].Path != "secret/key.txt" || audit.UnlicensedCount != 5 {
		t.Fatalf("audit = %+v", audit)
	}
	do("GET", base+"/licenses", "bob", nil, 403)

	// Server-wide summary.
	do("GET", "/api/v1/admin/compliance", "alice", nil, 403)
	sum := decode[scan.Summary](t, f.do(t, "GET", "/api/v1/admin/compliance", "admin", nil))
	if sum.Modes.Block != 1 || sum.ResolvedFindings != 1 || sum.OpenFindings != 0 || sum.Trees < 2 {
		t.Fatalf("summary = %+v", sum)
	}

	// Audit trail.
	actions := map[string]bool{}
	for _, ev := range f.audit.actions() {
		actions[ev] = true
	}
	for _, want := range []string{"compliance.settings", "secret.detected", "compliance.finding.resolve", "compliance.finding.reopen", "compliance.allow.add", "compliance.allow.remove"} {
		if !actions[want] {
			t.Errorf("audit lacks %s (have %v)", want, actions)
		}
	}
}

func TestComplianceNotImplemented(t *testing.T) {
	f := newFixture(t)
	for _, c := range []struct{ method, path string }{
		{"GET", "/api/v1/trees/acme/web/compliance/settings"},
		{"GET", "/api/v1/trees/acme/web/compliance/licenses"},
		{"GET", "/api/v1/admin/compliance"},
	} {
		if rec := f.do(t, c.method, c.path, "admin", nil); rec.Code != 501 {
			t.Errorf("%s %s = %d, want 501", c.method, c.path, rec.Code)
		}
	}
}
