package observability

import (
	"strings"
	"testing"
)

func TestMetricsRenderIncludesRequestCounter(t *testing.T) {
	metrics := NewMetrics()
	metrics.RecordRequest("GET", "/health", 200)
	metrics.RecordRequest("GET", "/health", 200)

	output := metrics.Render()
	if !strings.Contains(output, `wt_http_requests_total{method="GET",path="/health",status="200"} 2`) {
		t.Fatalf("metrics output missing counter: %s", output)
	}
}
