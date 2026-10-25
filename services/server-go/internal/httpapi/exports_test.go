package httpapi

import (
	"archive/tar"
	"archive/zip"
	"bytes"
	"compress/gzip"
	"context"
	"io"
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// pathDenyAccess denies tree:read on the listed paths and allows the rest.
type pathDenyAccess struct{ deny map[string]bool }

func (e pathDenyAccess) Evaluate(_ context.Context, _ *domain.Principal, act domain.Action, res domain.Resource, _ domain.RequestEnv) (domain.Decision, error) {
	if act == domain.ActTreeRead && e.deny[res.Path] {
		return domain.Decision{Allowed: false, Effect: "deny"}, nil
	}
	return domain.Decision{Allowed: true, Effect: "allow"}, nil
}

func (pathDenyAccess) EffectivePermissions(context.Context, *domain.Principal, string, *domain.Tree) ([]domain.Action, error) {
	return nil, nil
}

type archived struct {
	mode int64
	data string
}

func untar(t *testing.T, body []byte) map[string]archived {
	t.Helper()
	gz, err := gzip.NewReader(bytes.NewReader(body))
	if err != nil {
		t.Fatal(err)
	}
	tr := tar.NewReader(gz)
	out := map[string]archived{}
	for {
		h, err := tr.Next()
		if err == io.EOF {
			break
		}
		if err != nil {
			t.Fatal(err)
		}
		data, err := io.ReadAll(tr)
		if err != nil {
			t.Fatal(err)
		}
		out[h.Name] = archived{mode: h.Mode, data: string(data)}
	}
	return out
}

func unzip(t *testing.T, body []byte) map[string]archived {
	t.Helper()
	zr, err := zip.NewReader(bytes.NewReader(body), int64(len(body)))
	if err != nil {
		t.Fatal(err)
	}
	out := map[string]archived{}
	for _, f := range zr.File {
		rc, err := f.Open()
		if err != nil {
			t.Fatal(err)
		}
		data, err := io.ReadAll(rc)
		rc.Close()
		if err != nil {
			t.Fatal(err)
		}
		out[f.Name] = archived{mode: int64(f.Mode().Perm()), data: string(data)}
	}
	return out
}

func names(files map[string]archived) []string {
	out := make([]string, 0, len(files))
	for n := range files {
		out = append(out, n)
	}
	slices.Sort(out)
	return out
}

func addMITRule(t *testing.T, f *fixture) {
	t.Helper()
	must(t, f.store.PutLicenseRule(context.Background(), &domain.LicenseRule{TreeID: f.web.ID, PathGlob: "src/**", SPDX: "MIT", Grant: "redistribute"}))
	// Ignored for the notice: specific to one grantee / expired.
	past := t0.Add(-time.Hour)
	must(t, f.store.PutLicenseRule(context.Background(), &domain.LicenseRule{TreeID: f.web.ID, PathGlob: "README.md", SPDX: "GPL-3.0-only", Grant: "redistribute", Grantee: "other"}))
	must(t, f.store.PutLicenseRule(context.Background(), &domain.LicenseRule{TreeID: f.web.ID, PathGlob: "run.sh", SPDX: "BSD-3-Clause", ExpiresAt: &past}))
}

func TestArchiveTarGz(t *testing.T) {
	f := newFixture(t)
	addMITRule(t, f)
	rec := f.do(t, "GET", "/api/v1/trees/acme/web/archive/snap-2", "carol", nil)
	if rec.Code != 200 {
		t.Fatalf("status = %d: %s", rec.Code, rec.Body.String())
	}
	h := rec.Header()
	if h.Get("Content-Type") != "application/gzip" || !strings.Contains(h.Get("Content-Disposition"), "web-snap-2.tar.gz") || h.Get("X-Archive-Omitted") != "1" {
		t.Fatalf("headers = %v", h)
	}
	files := untar(t, rec.Body.Bytes())
	want := []string{"LICENSE", "LICENSES/NOTICE", "README.md", "index.html", "run.sh", "src/main.go"}
	if got := names(files); !slices.Equal(got, want) {
		t.Fatalf("files = %v, want %v", got, want)
	}
	if strings.Contains(rec.Body.String(), "hunter2") {
		t.Fatal("restricted content leaked")
	}
	if files["run.sh"].mode != 0o755 || files["README.md"].mode != 0o644 {
		t.Fatalf("modes: run.sh %o README.md %o", files["run.sh"].mode, files["README.md"].mode)
	}
	if files["src/main.go"].data != "package main\n\nfunc main() {}\n" {
		t.Fatalf("content = %q", files["src/main.go"].data)
	}
	notice := files["LICENSES/NOTICE"].data
	for _, part := range []string{"W0rkTree license notice\n", "Tree: acme/web\n", "Snapshot: snap-2\n", "\nMIT\n    src/main.go\n", "1 path was omitted"} {
		if !strings.Contains(notice, part) {
			t.Fatalf("notice lacks %q:\n%s", part, notice)
		}
	}
	for _, absent := range []string{"secret", "GPL", "BSD"} {
		if strings.Contains(notice, absent) {
			t.Fatalf("notice mentions %q:\n%s", absent, notice)
		}
	}
	hasAudit(t, f, "tree.export")
	for _, e := range f.audit.events {
		if e.Action == "tree.export" && (e.Details["omitted"] != "1" || e.Details["format"] != "tar.gz" || e.Details["snapshot"] != "snap-2") {
			t.Fatalf("audit details = %v", e.Details)
		}
	}
}

func TestArchiveZipAndBranchRef(t *testing.T) {
	f := newFixture(t)
	rec := f.do(t, "GET", "/api/v1/trees/acme/web/archive/main?format=zip", "carol", nil)
	if rec.Code != 200 || rec.Header().Get("Content-Type") != "application/zip" {
		t.Fatalf("status = %d, headers %v: %s", rec.Code, rec.Header(), rec.Body.String())
	}
	files := unzip(t, rec.Body.Bytes())
	if _, ok := files["secret/key.txt"]; ok {
		t.Fatal("restricted file included")
	}
	if files["run.sh"].mode != 0o755 || files["README.md"].data != "# web v2\n" {
		t.Fatalf("files = %v", files)
	}
	// Only the omission is noted: no included path is covered by a rule.
	if n := files["LICENSES/NOTICE"].data; !strings.Contains(n, "1 path was omitted") {
		t.Fatalf("notice = %q", n)
	}
}

func TestArchiveOmitsPolicyDeniedPaths(t *testing.T) {
	f := newFixture(t, func(d *Deps) {
		d.Access = pathDenyAccess{deny: map[string]bool{"index.html": true}}
	})
	rec := f.do(t, "GET", "/api/v1/trees/acme/web/archive/snap-2", "alice", nil)
	if rec.Code != 200 {
		t.Fatalf("status = %d: %s", rec.Code, rec.Body.String())
	}
	files := untar(t, rec.Body.Bytes())
	if _, ok := files["index.html"]; ok {
		t.Fatal("policy-denied path included")
	}
	if rec.Header().Get("X-Archive-Omitted") != "2" || !strings.Contains(files["LICENSES/NOTICE"].data, "2 paths were omitted") {
		t.Fatalf("omitted = %s, notice %q", rec.Header().Get("X-Archive-Omitted"), files["LICENSES/NOTICE"].data)
	}
}

func TestArchiveWithoutRestrictionsHasNoNotice(t *testing.T) {
	f := newFixture(t)
	rec := f.do(t, "GET", "/api/v1/trees/acme/web/archive/snap-1", "carol", nil)
	if rec.Code != 200 {
		t.Fatalf("status = %d", rec.Code)
	}
	if got := names(untar(t, rec.Body.Bytes())); !slices.Equal(got, []string{"README.md"}) {
		t.Fatalf("files = %v", got)
	}
}

func TestArchiveErrors(t *testing.T) {
	runCases(t, []endpointCase{
		{name: "bad format", method: "GET", path: "/api/v1/trees/acme/web/archive/snap-2?format=rar", as: "carol", status: 400, code: "bad_request"},
		{name: "unknown snapshot", method: "GET", path: "/api/v1/trees/acme/web/archive/nope", as: "carol", status: 404, code: "not_found"},
		{name: "foreign snapshot", method: "GET", path: "/api/v1/trees/acme/web/archive/snap-foreign", as: "carol", status: 404, code: "not_found"},
		{name: "outsider", method: "GET", path: "/api/v1/trees/acme/web/archive/snap-2", as: "dave", status: 403, code: "forbidden"},
		{name: "anonymous private", method: "GET", path: "/api/v1/trees/acme/web/archive/snap-2", status: 401, code: "unauthenticated"},
		{name: "missing manifest", method: "GET", path: "/api/v1/trees/acme/web/archive/snap-orphan", as: "carol", status: 500, code: "internal"},
	})
}

func TestBuildNoticeIsNilWithoutCoverage(t *testing.T) {
	if n := buildNotice(nil, nil, 0, nil); n != nil {
		t.Fatalf("notice = %q", n)
	}
}
