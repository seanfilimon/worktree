package httpapi

// Tests of GET /debug/pprof[/{profile}] (docs/ENGINEERING_PLAN.md §15.10).

import (
	"bytes"
	"compress/gzip"
	"io"
	"strings"
	"testing"
)

func newPprofFixture(t *testing.T) *fixture {
	t.Helper()
	return newFixture(t, func(d *Deps) { d.Pprof = true })
}

func TestPprofDisabledIsNotFound(t *testing.T) {
	f := newFixture(t)
	for _, p := range []string{"/debug/pprof", "/debug/pprof/heap", "/debug/pprof/cmdline"} {
		for _, as := range []string{"admin", "alice", ""} {
			rec := f.do(t, "GET", p, as, nil)
			if rec.Code != 404 || errCode(rec) != "not_found" {
				t.Fatalf("%s as %q with pprof off: %d %s", p, as, rec.Code, rec.Body.String())
			}
		}
	}
}

func TestPprofRequiresServerAdmin(t *testing.T) {
	f := newPprofFixture(t)
	for _, p := range []string{"/debug/pprof", "/debug/pprof/heap", "/debug/pprof/goroutine"} {
		if rec := f.do(t, "GET", p, "", nil); rec.Code != 401 {
			t.Fatalf("%s anonymous: %d %s", p, rec.Code, rec.Body.String())
		}
		if rec := f.do(t, "GET", p, "alice", nil); rec.Code != 403 {
			t.Fatalf("%s non-admin: %d %s", p, rec.Code, rec.Body.String())
		}
	}
}

func TestPprofIndexListsProfilesWithAbsoluteLinks(t *testing.T) {
	f := newPprofFixture(t)
	rec := f.do(t, "GET", "/debug/pprof", "admin", nil)
	if rec.Code != 200 || !strings.HasPrefix(rec.Header().Get("Content-Type"), "text/html") {
		t.Fatalf("index: %d %q", rec.Code, rec.Header().Get("Content-Type"))
	}
	body := rec.Body.String()
	for _, p := range pprofProfiles {
		if !strings.Contains(body, `href="/debug/pprof/`+p.name) {
			t.Errorf("index lacks an absolute link to %s:\n%s", p.name, body)
		}
	}
}

func TestPprofProfilesServeData(t *testing.T) {
	f := newPprofFixture(t)

	// Sampled profiles are gzipped protobuf.
	for _, name := range []string{"heap", "allocs", "goroutine", "threadcreate", "block", "mutex"} {
		rec := f.do(t, "GET", "/debug/pprof/"+name, "admin", nil)
		if rec.Code != 200 || rec.Body.Len() == 0 {
			t.Fatalf("%s: %d (%d bytes)", name, rec.Code, rec.Body.Len())
		}
		zr, err := gzip.NewReader(bytes.NewReader(rec.Body.Bytes()))
		if err != nil {
			t.Fatalf("%s is not gzip: %v", name, err)
		}
		if raw, err := io.ReadAll(zr); err != nil || len(raw) == 0 {
			t.Fatalf("%s: gunzip %d bytes, %v", name, len(raw), err)
		}
	}

	// Text renderings.
	rec := f.do(t, "GET", "/debug/pprof/goroutine?debug=1", "admin", nil)
	if rec.Code != 200 || !strings.Contains(rec.Body.String(), "goroutine profile:") {
		t.Fatalf("goroutine?debug=1: %d %.200s", rec.Code, rec.Body.String())
	}
	if rec := f.do(t, "GET", "/debug/pprof/cmdline", "admin", nil); rec.Code != 200 || rec.Body.Len() == 0 {
		t.Fatalf("cmdline: %d", rec.Code)
	}
	if rec := f.do(t, "GET", "/debug/pprof/symbol", "admin", nil); rec.Code != 200 ||
		!strings.Contains(rec.Body.String(), "num_symbols") {
		t.Fatalf("symbol: %d %s", rec.Code, rec.Body.String())
	}
	// A one-second CPU profile.
	if rec := f.do(t, "GET", "/debug/pprof/profile?seconds=1", "admin", nil); rec.Code != 200 || rec.Body.Len() == 0 {
		t.Fatalf("profile: %d %s", rec.Code, rec.Body.String())
	}

	// Unknown profiles are not found.
	if rec := f.do(t, "GET", "/debug/pprof/nope", "admin", nil); rec.Code != 404 || errCode(rec) != "not_found" {
		t.Fatalf("unknown profile: %d %s", rec.Code, rec.Body.String())
	}
}
