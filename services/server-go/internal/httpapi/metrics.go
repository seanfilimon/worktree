package httpapi

import (
	"bytes"
	"context"
	"fmt"
	"io"
	"log/slog"
	"net/http"
	"runtime"
	"sort"
	"strconv"
	"strings"
	"sync"
	"sync/atomic"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/opsmetrics"
)

// latencyBuckets are the upper bounds (seconds) of the request duration
// histogram.
var latencyBuckets = []float64{0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1, 2.5, 5, 10}

// statsCacheTTL bounds how often /metrics recomputes store statistics
// (which may walk the object store).
const statsCacheTTL = 10 * time.Second

type counterKey struct {
	route string
	code  int
}

type histogram struct {
	buckets []uint64 // non-cumulative counts per latencyBuckets entry
	count   uint64
	sum     float64
}

// metrics is a minimal, dependency-free metrics registry rendered in the
// Prometheus text exposition format (version 0.0.4).
type metrics struct {
	inFlight atomic.Int64

	mu        sync.Mutex
	requests  map[counterKey]uint64
	latencies map[string]*histogram
}

func newMetrics() *metrics {
	return &metrics{
		requests:  make(map[counterKey]uint64),
		latencies: make(map[string]*histogram),
	}
}

func (m *metrics) observe(route string, code int, d time.Duration) {
	secs := d.Seconds()
	m.mu.Lock()
	defer m.mu.Unlock()
	m.requests[counterKey{route, code}]++
	h := m.latencies[route]
	if h == nil {
		h = &histogram{buckets: make([]uint64, len(latencyBuckets))}
		m.latencies[route] = h
	}
	h.count++
	h.sum += secs
	if i := sort.SearchFloat64s(latencyBuckets, secs); i < len(latencyBuckets) {
		h.buckets[i]++
	}
}

// statsCache memoizes ServerStats for the metrics endpoint.
type statsCache struct {
	ttl     time.Duration
	compute func(context.Context) (domain.ServerStats, error)

	mu    sync.Mutex
	at    time.Time
	value domain.ServerStats
	err   error
}

func (c *statsCache) get(ctx context.Context) (domain.ServerStats, error) {
	c.mu.Lock()
	defer c.mu.Unlock()
	if !c.at.IsZero() && time.Since(c.at) < c.ttl {
		return c.value, c.err
	}
	c.value, c.err = c.compute(ctx)
	c.at = time.Now()
	return c.value, c.err
}

// handleMetrics answers GET /metrics in the Prometheus text format.
func (a *api) handleMetrics(w http.ResponseWriter, r *http.Request) error {
	var buf bytes.Buffer
	a.writeMetrics(r.Context(), &buf)
	w.Header().Set("Content-Type", "text/plain; version=0.0.4; charset=utf-8")
	w.Header().Set("Cache-Control", "no-store")
	_, _ = buf.WriteTo(w)
	return nil
}

func (a *api) writeMetrics(ctx context.Context, w io.Writer) {
	m := a.metrics
	m.mu.Lock()
	keys := make([]counterKey, 0, len(m.requests))
	for k := range m.requests {
		keys = append(keys, k)
	}
	counts := make(map[counterKey]uint64, len(keys))
	for k, v := range m.requests {
		counts[k] = v
	}
	routes := make([]string, 0, len(m.latencies))
	hists := make(map[string]histogram, len(m.latencies))
	for route, h := range m.latencies {
		routes = append(routes, route)
		hists[route] = histogram{buckets: append([]uint64(nil), h.buckets...), count: h.count, sum: h.sum}
	}
	m.mu.Unlock()

	sort.Slice(keys, func(i, j int) bool {
		if keys[i].route != keys[j].route {
			return keys[i].route < keys[j].route
		}
		return keys[i].code < keys[j].code
	})
	sort.Strings(routes)

	header(w, "worktree_http_requests_total", "counter", "Total HTTP requests by route and status code.")
	for _, k := range keys {
		fmt.Fprintf(w, "worktree_http_requests_total{route=\"%s\",code=\"%d\"} %d\n", escapeLabel(k.route), k.code, counts[k])
	}

	header(w, "worktree_http_request_duration_seconds", "histogram", "HTTP request latency by route.")
	for _, route := range routes {
		h := hists[route]
		lr := escapeLabel(route)
		var cum uint64
		for i, le := range latencyBuckets {
			cum += h.buckets[i]
			fmt.Fprintf(w, "worktree_http_request_duration_seconds_bucket{route=\"%s\",le=\"%s\"} %d\n", lr, formatFloat(le), cum)
		}
		fmt.Fprintf(w, "worktree_http_request_duration_seconds_bucket{route=\"%s\",le=\"+Inf\"} %d\n", lr, h.count)
		fmt.Fprintf(w, "worktree_http_request_duration_seconds_sum{route=\"%s\"} %s\n", lr, formatFloat(h.sum))
		fmt.Fprintf(w, "worktree_http_request_duration_seconds_count{route=\"%s\"} %d\n", lr, h.count)
	}

	gauge(w, "worktree_http_requests_in_flight", "HTTP requests currently being served.", float64(m.inFlight.Load()))

	if st, err := a.stats.get(ctx); err != nil {
		a.log.WarnContext(ctx, "metrics: computing server stats failed", slog.Any("error", err))
	} else {
		gauge(w, "worktree_tenants", "Number of tenants.", float64(st.Tenants))
		gauge(w, "worktree_accounts", "Number of accounts.", float64(st.Accounts))
		gauge(w, "worktree_trees", "Number of trees.", float64(st.Trees))
		gauge(w, "worktree_branches", "Number of branches.", float64(st.Branches))
		gauge(w, "worktree_snapshots", "Number of snapshots.", float64(st.Snapshots))
		gauge(w, "worktree_staged_snapshots", "Number of staged snapshots.", float64(st.Staged))
		gauge(w, "worktree_objects", "Number of stored objects.", float64(st.Objects))
		gauge(w, "worktree_storage_bytes", "Bytes used by stored objects.", float64(st.StorageBytes))
	}

	writeOpsCounters(w)

	header(w, "worktree_build_info", "gauge", "Build information; the value is always 1.")
	fmt.Fprintf(w, "worktree_build_info{version=\"%s\",goversion=\"%s\"} 1\n", escapeLabel(a.d.Version), escapeLabel(runtime.Version()))
	gauge(w, "worktree_uptime_seconds", "Seconds since the server started.", time.Since(a.d.StartedAt).Seconds())
	gauge(w, "go_goroutines", "Number of goroutines that currently exist.", float64(runtime.NumGoroutine()))
	var ms runtime.MemStats
	runtime.ReadMemStats(&ms)
	gauge(w, "go_memstats_heap_alloc_bytes", "Number of heap bytes allocated and still in use.", float64(ms.HeapAlloc))
	gauge(w, "go_memstats_sys_bytes", "Number of bytes obtained from the system.", float64(ms.Sys))
}

// writeOpsCounters renders the process-wide §13.3 counters. Labelled
// counters always list their well-known label values (as 0 until used) so
// dashboards see the series from the start.
func writeOpsCounters(w io.Writer) {
	known := map[string][]string{
		opsmetrics.SyncPushes.Name:     {"accepted", "rejected", "error"},
		opsmetrics.AuthzDecisions.Name: {"allow", "deny"},
	}
	for _, c := range opsmetrics.All() {
		header(w, c.Name, "counter", c.Help)
		if c.Label == "" {
			fmt.Fprintf(w, "%s %d\n", c.Name, c.Get(""))
			continue
		}
		seen := map[string]bool{}
		samples := c.Samples()
		for _, s := range samples {
			seen[s.Label] = true
		}
		for _, l := range known[c.Name] {
			if !seen[l] {
				samples = append(samples, opsmetrics.Sample{Label: l})
			}
		}
		sort.Slice(samples, func(i, j int) bool { return samples[i].Label < samples[j].Label })
		for _, s := range samples {
			fmt.Fprintf(w, "%s{%s=\"%s\"} %d\n", c.Name, c.Label, escapeLabel(s.Label), s.Value)
		}
	}
}

func header(w io.Writer, name, typ, help string) {
	fmt.Fprintf(w, "# HELP %s %s\n# TYPE %s %s\n", name, help, name, typ)
}

func gauge(w io.Writer, name, help string, v float64) {
	header(w, name, "gauge", help)
	fmt.Fprintf(w, "%s %s\n", name, formatFloat(v))
}

func formatFloat(v float64) string { return strconv.FormatFloat(v, 'g', -1, 64) }

var labelEscaper = strings.NewReplacer(`\`, `\\`, `"`, `\"`, "\n", `\n`)

// escapeLabel escapes a label value per the exposition format.
func escapeLabel(s string) string { return labelEscaper.Replace(s) }
