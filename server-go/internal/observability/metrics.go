package observability

import (
	"fmt"
	"net/http"
	"sort"
	"strings"
	"sync"
)

type Metrics struct {
	mu       sync.Mutex
	requests map[requestKey]uint64
}

type requestKey struct {
	Method string
	Path   string
	Status int
}

func NewMetrics() *Metrics {
	return &Metrics{
		requests: make(map[requestKey]uint64),
	}
}

func (m *Metrics) RecordRequest(method string, path string, status int) {
	m.mu.Lock()
	defer m.mu.Unlock()
	m.requests[requestKey{Method: method, Path: path, Status: status}]++
}

func (m *Metrics) ServeHTTP(w http.ResponseWriter, r *http.Request) {
	w.Header().Set("Content-Type", "text/plain; version=0.0.4")
	w.WriteHeader(http.StatusOK)
	_, _ = w.Write([]byte(m.Render()))
}

func (m *Metrics) Render() string {
	m.mu.Lock()
	defer m.mu.Unlock()

	var keys []requestKey
	for key := range m.requests {
		keys = append(keys, key)
	}
	sort.Slice(keys, func(i, j int) bool {
		if keys[i].Path != keys[j].Path {
			return keys[i].Path < keys[j].Path
		}
		if keys[i].Method != keys[j].Method {
			return keys[i].Method < keys[j].Method
		}
		return keys[i].Status < keys[j].Status
	})

	var b strings.Builder
	b.WriteString("# HELP wt_http_requests_total Total HTTP requests.\n")
	b.WriteString("# TYPE wt_http_requests_total counter\n")
	for _, key := range keys {
		fmt.Fprintf(
			&b,
			"wt_http_requests_total{method=%q,path=%q,status=%q} %d\n",
			key.Method,
			key.Path,
			fmt.Sprintf("%d", key.Status),
			m.requests[key],
		)
	}
	return b.String()
}
