package httpapi

import (
	"context"
	"regexp"
	"strconv"
	"strings"
	"testing"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

var sampleRe = regexp.MustCompile(`^([a-zA-Z_:][a-zA-Z0-9_:]*)(\{(.*)\})? (\S+)$`)

// parseExposition validates the Prometheus text format and returns the
// samples keyed by their full "name{labels}" text.
func parseExposition(t *testing.T, body string) map[string]float64 {
	t.Helper()
	types := map[string]string{}
	samples := map[string]float64{}
	for i, line := range strings.Split(strings.TrimSuffix(body, "\n"), "\n") {
		switch {
		case strings.HasPrefix(line, "# HELP "):
			continue
		case strings.HasPrefix(line, "# TYPE "):
			f := strings.Fields(line)
			if len(f) != 4 {
				t.Fatalf("line %d: bad TYPE %q", i, line)
			}
			if _, dup := types[f[2]]; dup {
				t.Fatalf("line %d: duplicate TYPE for %s", i, f[2])
			}
			types[f[2]] = f[3]
			continue
		}
		m := sampleRe.FindStringSubmatch(line)
		if m == nil {
			t.Fatalf("line %d: malformed sample %q", i, line)
		}
		name := m[1]
		base := name
		if _, ok := types[base]; !ok {
			for _, suf := range []string{"_bucket", "_sum", "_count"} {
				if b := strings.TrimSuffix(name, suf); b != name && types[b] == "histogram" {
					base = b
				}
			}
		}
		if _, ok := types[base]; !ok {
			t.Fatalf("line %d: sample %s before its TYPE", i, name)
		}
		v, err := strconv.ParseFloat(m[4], 64)
		if err != nil {
			t.Fatalf("line %d: bad value %q", i, m[4])
		}
		key := name + m[2]
		if _, dup := samples[key]; dup {
			t.Fatalf("line %d: duplicate sample %s", i, key)
		}
		samples[key] = v
	}
	return samples
}

func TestMetricsExposition(t *testing.T) {
	f := newFixture(t)
	f.do(t, "GET", "/healthz", "", nil)
	f.do(t, "GET", "/healthz", "", nil)
	f.do(t, "GET", "/api/v1/tenants/acme", "", nil)      // 401
	f.do(t, "GET", "/api/v1/nope", "", nil)              // catch-all 404
	f.do(t, "GET", "/api/v1/trees/acme/web", "bob", nil) // 200

	rec := f.do(t, "GET", "/metrics", "", nil)
	if rec.Code != 200 {
		t.Fatalf("status = %d", rec.Code)
	}
	if ct := rec.Header().Get("Content-Type"); !strings.HasPrefix(ct, "text/plain; version=0.0.4") {
		t.Fatalf("Content-Type = %q", ct)
	}
	s := parseExposition(t, rec.Body.String())

	want := map[string]float64{
		`worktree_http_requests_total{route="GET /healthz",code="200"}`:                      2,
		`worktree_http_requests_total{route="GET /api/v1/tenants/{tenant}",code="401"}`:      1,
		`worktree_http_requests_total{route="/api/",code="404"}`:                             1,
		`worktree_http_requests_total{route="GET /api/v1/trees/{tenant}/{tree}",code="200"}`: 1,
		`worktree_http_request_duration_seconds_count{route="GET /healthz"}`:                 2,
		`worktree_http_request_duration_seconds_bucket{route="GET /healthz",le="+Inf"}`:      2,
		`worktree_tenants`:                 7,
		`worktree_accounts`:                6,
		`worktree_trees`:                   2,
		`worktree_http_requests_in_flight`: 1, // the /metrics request itself
	}
	for k, v := range want {
		if got, ok := s[k]; !ok || got != v {
			t.Errorf("%s = %v (present %v), want %v", k, got, ok, v)
		}
	}
	n, _, _ := f.objs.Stats(context.Background())
	if s["worktree_objects"] != float64(n) {
		t.Errorf("worktree_objects = %v, want %d", s["worktree_objects"], n)
	}
	for _, k := range []string{"go_goroutines", "worktree_uptime_seconds", `worktree_build_info{version="test-1.0",goversion=`} {
		found := false
		for sk := range s {
			if strings.HasPrefix(sk, k) {
				found = true
			}
		}
		if !found {
			t.Errorf("missing %s", k)
		}
	}

	// Buckets are cumulative and non-decreasing.
	prev := -1.0
	for _, le := range append(append([]string{}, bucketLabels()...), "+Inf") {
		v, ok := s[`worktree_http_request_duration_seconds_bucket{route="GET /healthz",le="`+le+`"}`]
		if !ok {
			t.Fatalf("missing bucket le=%s", le)
		}
		if v < prev {
			t.Fatalf("bucket le=%s decreased: %v < %v", le, v, prev)
		}
		prev = v
	}
}

func bucketLabels() []string {
	out := make([]string, len(latencyBuckets))
	for i, b := range latencyBuckets {
		out[i] = formatFloat(b)
	}
	return out
}

func TestMetricsObserveBuckets(t *testing.T) {
	m := newMetrics()
	m.observe("r", 200, 3*time.Millisecond)  // first bucket
	m.observe("r", 200, 10*time.Millisecond) // exactly on a bound: le is inclusive
	m.observe("r", 500, 20*time.Second)      // above all bounds: only +Inf
	h := m.latencies["r"]
	if h.count != 3 || h.buckets[0] != 1 || h.buckets[1] != 1 {
		t.Fatalf("hist = %+v", h)
	}
	var inBuckets uint64
	for _, c := range h.buckets {
		inBuckets += c
	}
	if inBuckets != 2 {
		t.Fatalf("bucketed = %d, want 2", inBuckets)
	}
	if m.requests[counterKey{"r", 500}] != 1 || m.requests[counterKey{"r", 200}] != 2 {
		t.Fatalf("counters = %v", m.requests)
	}
}

func TestEscapeLabel(t *testing.T) {
	if got := escapeLabel("a\"b\\c\nd"); got != `a\"b\\c\nd` {
		t.Fatalf("escapeLabel = %q", got)
	}
}

func TestStatsCache(t *testing.T) {
	calls := 0
	c := &statsCache{ttl: time.Hour, compute: func(context.Context) (s domain.ServerStats, err error) {
		calls++
		s.Trees = calls
		return s, nil
	}}
	a, _ := c.get(context.Background())
	b, _ := c.get(context.Background())
	if calls != 1 || a != b {
		t.Fatalf("calls = %d, a=%v b=%v", calls, a, b)
	}
}
