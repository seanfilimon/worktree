package httpapi

import (
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/ramizik/worktree/server-go/internal/observability"
)

func TestRouterExposesMetrics(t *testing.T) {
	metrics := observability.NewMetrics()
	router := NewRouter(RouterConfig{Version: "test", Metrics: metrics})

	healthReq := httptest.NewRequest(http.MethodGet, "/health", nil)
	router.ServeHTTP(httptest.NewRecorder(), healthReq)

	req := httptest.NewRequest(http.MethodGet, "/metrics", nil)
	rec := httptest.NewRecorder()
	router.ServeHTTP(rec, req)

	if rec.Code != http.StatusOK {
		t.Fatalf("status = %d", rec.Code)
	}
	if !strings.Contains(rec.Body.String(), `wt_http_requests_total{method="GET",path="/health",status="200"} 1`) {
		t.Fatalf("metrics body = %s", rec.Body.String())
	}
}
