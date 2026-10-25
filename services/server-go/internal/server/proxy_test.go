package server

import (
	"net/http"
	"net/http/httptest"
	"testing"

	"github.com/seanfilimon/worktree/services/server-go/internal/config"
)

func TestForwardedFor(t *testing.T) {
	trusted, err := config.ParseProxies([]string{"10.0.0.0/8", "192.168.1.1"})
	if err != nil {
		t.Fatal(err)
	}
	var seen string
	h := forwardedFor(http.HandlerFunc(func(_ http.ResponseWriter, r *http.Request) { seen = r.RemoteAddr }), trusted)
	cases := []struct {
		remote, xff, want string
	}{
		{"10.1.2.3:5555", "203.0.113.7", "203.0.113.7:0"},
		{"10.1.2.3:5555", "1.1.1.1, 203.0.113.7, 10.9.9.9", "203.0.113.7:0"}, // rightmost untrusted
		{"192.168.1.1:80", "198.51.100.2", "198.51.100.2:0"},
		{"8.8.8.8:1234", "203.0.113.7", "8.8.8.8:1234"}, // untrusted peer: header ignored
		{"10.1.2.3:5555", "", "10.1.2.3:5555"},          // no header
		{"10.1.2.3:5555", "garbage", "10.1.2.3:5555"},   // malformed
		{"10.1.2.3:5555", "10.4.4.4", "10.1.2.3:5555"},  // only proxies
	}
	for _, c := range cases {
		req := httptest.NewRequest("GET", "/healthz", nil)
		req.RemoteAddr = c.remote
		if c.xff != "" {
			req.Header.Set("X-Forwarded-For", c.xff)
		}
		h.ServeHTTP(httptest.NewRecorder(), req)
		if seen != c.want {
			t.Errorf("remote %s xff %q: got %s, want %s", c.remote, c.xff, seen, c.want)
		}
	}
	if _, err := config.ParseProxies([]string{"not-an-ip"}); err == nil {
		t.Error("invalid proxy accepted")
	}
}
