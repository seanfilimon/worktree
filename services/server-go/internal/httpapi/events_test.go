package httpapi

import (
	"bufio"
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// sseStream reads an event stream line by line in the background.
type sseStream struct {
	resp   *http.Response
	lines  chan string
	cancel context.CancelFunc
}

func openSSE(t *testing.T, srv *httptest.Server, query, as string) *sseStream {
	t.Helper()
	ctx, cancel := context.WithCancel(context.Background())
	req, err := http.NewRequestWithContext(ctx, "GET", srv.URL+"/api/v1/events"+query, nil)
	if err != nil {
		t.Fatal(err)
	}
	if as != "" {
		req.Header.Set("Authorization", "Bearer "+as)
	}
	resp, err := srv.Client().Do(req)
	if err != nil {
		cancel()
		t.Fatal(err)
	}
	s := &sseStream{resp: resp, lines: make(chan string, 64), cancel: cancel}
	go func() {
		defer close(s.lines)
		sc := bufio.NewScanner(resp.Body)
		for sc.Scan() {
			s.lines <- sc.Text()
		}
	}()
	t.Cleanup(func() {
		cancel()
		resp.Body.Close()
	})
	return s
}

// next returns the next line that satisfies match, failing after timeout.
func (s *sseStream) next(t *testing.T, timeout time.Duration, match func(string) bool) string {
	t.Helper()
	deadline := time.After(timeout)
	for {
		select {
		case l, ok := <-s.lines:
			if !ok {
				t.Fatal("stream closed")
			}
			if match(l) {
				return l
			}
		case <-deadline:
			t.Fatal("timed out waiting for SSE line")
		}
	}
}

func waitFor(t *testing.T, what string, cond func() bool) {
	t.Helper()
	deadline := time.Now().Add(3 * time.Second)
	for !cond() {
		if time.Now().After(deadline) {
			t.Fatalf("timed out waiting for %s", what)
		}
		time.Sleep(5 * time.Millisecond)
	}
}

func TestEventsAuthorization(t *testing.T) {
	runCases(t, []endpointCase{
		{name: "firehose non-admin", method: "GET", path: "/api/v1/events", as: "alice", status: 403, code: "forbidden"},
		{name: "firehose anonymous", method: "GET", path: "/api/v1/events", status: 401, code: "unauthenticated"},
		{name: "tree without tenant", method: "GET", path: "/api/v1/events?tree=web", as: "alice", status: 400, code: "bad_request"},
		{name: "private tree anonymous", method: "GET", path: "/api/v1/events?tenant=acme&tree=web", status: 401, code: "unauthenticated"},
		{name: "tenant outsider", method: "GET", path: "/api/v1/events?tenant=acme", as: "dave", status: 403, code: "forbidden"},
		{name: "unknown tree", method: "GET", path: "/api/v1/events?tenant=acme&tree=nope", as: "alice", status: 404, code: "not_found"},
		{name: "unknown tenant", method: "GET", path: "/api/v1/events?tenant=nope", as: "alice", status: 404, code: "not_found"},
	})
}

func TestEventsStream(t *testing.T) {
	old := heartbeatInterval
	heartbeatInterval = 50 * time.Millisecond
	t.Cleanup(func() { heartbeatInterval = old })

	f := newFixture(t)
	srv := httptest.NewServer(f.h)
	t.Cleanup(srv.Close)

	s := openSSE(t, srv, "?tenant=acme&tree=web", "carol")
	if s.resp.StatusCode != http.StatusOK {
		t.Fatalf("status = %d", s.resp.StatusCode)
	}
	if ct := s.resp.Header.Get("Content-Type"); ct != "text/event-stream" {
		t.Fatalf("Content-Type = %q", ct)
	}
	if cc := s.resp.Header.Get("Cache-Control"); cc != "no-cache" {
		t.Fatalf("Cache-Control = %q", cc)
	}
	s.next(t, 2*time.Second, func(l string) bool { return l == ": connected" })
	waitFor(t, "subscription", func() bool { return f.bus.subscribers() == 1 })

	// An event for another tree is filtered out; the web event arrives.
	f.bus.Publish(domain.Event{Type: domain.EventBranchPushed, TenantID: f.acme.ID, TreeID: f.docs.ID, At: t0})
	f.bus.Publish(domain.Event{Type: domain.EventBranchPushed, TenantID: f.acme.ID, TreeID: f.web.ID, Actor: "bob",
		Payload: map[string]string{"branch": "main"}, At: t0})
	ev := s.next(t, 2*time.Second, func(l string) bool { return strings.HasPrefix(l, "event: ") })
	if ev != "event: branch.pushed" {
		t.Fatalf("event line = %q", ev)
	}
	data := s.next(t, time.Second, func(string) bool { return true })
	if !strings.HasPrefix(data, "data: ") {
		t.Fatalf("data line = %q", data)
	}
	var got domain.Event
	if err := json.Unmarshal([]byte(strings.TrimPrefix(data, "data: ")), &got); err != nil {
		t.Fatal(err)
	}
	if got.TreeID != f.web.ID || got.Actor != "bob" || got.Type != domain.EventBranchPushed {
		t.Fatalf("event = %+v", got)
	}
	if blank := s.next(t, time.Second, func(string) bool { return true }); blank != "" {
		t.Fatalf("frame not terminated by a blank line: %q", blank)
	}

	s.next(t, 2*time.Second, func(l string) bool { return l == ": heartbeat" })

	// Client disconnect releases the subscription.
	s.cancel()
	waitFor(t, "unsubscribe after disconnect", func() bool { return f.bus.subscribers() == 0 })
}

func TestEventsStreamEndsOnShutdown(t *testing.T) {
	f := newFixture(t)
	srv := httptest.NewServer(f.h)
	t.Cleanup(srv.Close)

	s := openSSE(t, srv, "?tenant=acme", "carol")
	s.next(t, 2*time.Second, func(l string) bool { return l == ": connected" })
	waitFor(t, "subscription", func() bool { return f.bus.subscribers() == 1 })
	f.bus.Publish(domain.Event{Type: domain.EventTreeCreated, TenantID: f.acme.ID, At: t0})
	s.next(t, 2*time.Second, func(l string) bool { return l == "event: tree.created" })

	close(f.done)
	deadline := time.After(3 * time.Second)
	for {
		select {
		case _, ok := <-s.lines:
			if !ok {
				waitFor(t, "unsubscribe", func() bool { return f.bus.subscribers() == 0 })
				return
			}
		case <-deadline:
			t.Fatal("stream did not end on shutdown")
		}
	}
}

func TestEventsAdminFirehoseWithTypes(t *testing.T) {
	f := newFixture(t)
	srv := httptest.NewServer(f.h)
	t.Cleanup(srv.Close)

	s := openSSE(t, srv, "?types=mr.opened,%20mr.merged", "admin")
	s.next(t, 2*time.Second, func(l string) bool { return l == ": connected" })
	waitFor(t, "subscription", func() bool { return f.bus.subscribers() == 1 })
	f.bus.Publish(domain.Event{Type: domain.EventTreeCreated, TenantID: "x", At: t0})
	f.bus.Publish(domain.Event{Type: domain.EventMRMerged, TenantID: "y", At: t0})
	if ev := s.next(t, 2*time.Second, func(l string) bool { return strings.HasPrefix(l, "event: ") }); ev != "event: mr.merged" {
		t.Fatalf("event = %q", ev)
	}
}

func TestWriteSSE(t *testing.T) {
	var b strings.Builder
	if err := writeSSE(&b, domain.Event{Type: "evil\ntype", TenantID: "t", Payload: func() {}, At: t0}); err != nil {
		t.Fatal(err)
	}
	out := b.String()
	if !strings.HasPrefix(out, "event: eviltype\ndata: {") || !strings.HasSuffix(out, "}\n\n") || strings.Count(out, "\n") != 3 {
		t.Fatalf("frame = %q", out)
	}
	if strings.Contains(out, "payload") {
		t.Fatalf("unencodable payload not dropped: %q", out)
	}
}
