package iam

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"net"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync"
	"testing"
	"time"

	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/credentials/insecure"
	"google.golang.org/grpc/health"
	healthpb "google.golang.org/grpc/health/grpc_health_v1"
	"google.golang.org/grpc/metadata"
	"google.golang.org/grpc/status"
	"google.golang.org/grpc/test/bufconn"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// failingAuth simulates a backend failure in Authenticate.
type failingAuth struct{ domain.Authenticator }

func (failingAuth) Authenticate(context.Context, string) (*domain.Principal, error) {
	return nil, errors.New("database on fire")
}

type grpcHarness struct {
	conn *grpc.ClientConn

	mu   sync.Mutex
	seen []*domain.Principal // principal observed by each handled call
	tok  []string            // TokenHashFrom observed by each handled call
}

func (h *grpcHarness) record(ctx context.Context) {
	h.mu.Lock()
	h.seen = append(h.seen, PrincipalFrom(ctx))
	h.tok = append(h.tok, TokenHashFrom(ctx))
	h.mu.Unlock()
}

func (h *grpcHarness) last() (*domain.Principal, string) {
	h.mu.Lock()
	defer h.mu.Unlock()
	if len(h.seen) == 0 {
		return nil, ""
	}
	return h.seen[len(h.seen)-1], h.tok[len(h.tok)-1]
}

func (h *grpcHarness) calls() int {
	h.mu.Lock()
	defer h.mu.Unlock()
	return len(h.seen)
}

func newGRPCHarness(t *testing.T, auth domain.Authenticator) *grpcHarness {
	t.Helper()
	h := &grpcHarness{}
	lis := bufconn.Listen(1 << 20)
	srv := grpc.NewServer(
		grpc.ChainUnaryInterceptor(UnaryInterceptor(auth),
			func(ctx context.Context, req any, _ *grpc.UnaryServerInfo, next grpc.UnaryHandler) (any, error) {
				h.record(ctx)
				return next(ctx, req)
			}),
		grpc.ChainStreamInterceptor(StreamInterceptor(auth),
			func(srv any, ss grpc.ServerStream, _ *grpc.StreamServerInfo, next grpc.StreamHandler) error {
				h.record(ss.Context())
				return next(srv, ss)
			}),
	)
	hs := health.NewServer()
	healthpb.RegisterHealthServer(srv, hs)
	go func() { _ = srv.Serve(lis) }()
	conn, err := grpc.NewClient("passthrough:///bufnet",
		grpc.WithContextDialer(func(ctx context.Context, _ string) (net.Conn, error) { return lis.DialContext(ctx) }),
		grpc.WithTransportCredentials(insecure.NewCredentials()))
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		_ = conn.Close()
		srv.Stop()
	})
	h.conn = conn
	return h
}

func TestGRPCInterceptors(t *testing.T) {
	e := newTestEnv(t, nil)
	alice := e.register(t, "alice")
	good, _, err := e.svc.CreateToken(context.Background(), alice.ID, "ci", 0)
	if err != nil {
		t.Fatal(err)
	}
	h := newGRPCHarness(t, e.svc)
	client := healthpb.NewHealthClient(h.conn)

	tests := []struct {
		name     string
		md       []string // key/value pairs
		wantCode codes.Code
		wantUser string // "" = anonymous
	}{
		{"no credentials", nil, codes.OK, ""},
		{"valid bearer", []string{"authorization", "Bearer " + good}, codes.OK, "alice"},
		{"lowercase scheme", []string{"authorization", "bearer " + good}, codes.OK, "alice"},
		{"empty header", []string{"authorization", ""}, codes.OK, ""},
		{"invalid token", []string{"authorization", "Bearer wt_nope"}, codes.Unauthenticated, ""},
		{"basic scheme", []string{"authorization", "Basic dXNlcjpwYXNz"}, codes.Unauthenticated, ""},
		{"bearer without token", []string{"authorization", "Bearer "}, codes.Unauthenticated, ""},
		{"duplicate headers", []string{"authorization", "Bearer " + good, "authorization", "Bearer " + good}, codes.Unauthenticated, ""},
	}
	for _, tt := range tests {
		for _, mode := range []string{"unary", "stream"} {
			t.Run(tt.name+"/"+mode, func(t *testing.T) {
				ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
				defer cancel()
				if tt.md != nil {
					ctx = metadata.AppendToOutgoingContext(ctx, tt.md...)
				}
				before := h.calls()
				var err error
				if mode == "unary" {
					_, err = client.Check(ctx, &healthpb.HealthCheckRequest{})
				} else {
					var stream healthpb.Health_WatchClient
					stream, err = client.Watch(ctx, &healthpb.HealthCheckRequest{})
					if err == nil {
						_, err = stream.Recv()
					}
				}
				if got := status.Code(err); got != tt.wantCode {
					t.Fatalf("code = %v (%v), want %v", got, err, tt.wantCode)
				}
				if tt.wantCode != codes.OK {
					if h.calls() != before {
						t.Fatal("handler ran despite invalid credentials")
					}
					return
				}
				p, th := h.last()
				if tt.wantUser == "" {
					if p != nil || th != "" {
						t.Fatalf("expected anonymous, got %+v %q", p, th)
					}
					return
				}
				if p == nil || p.Username != tt.wantUser || th != HashToken(good) {
					t.Fatalf("principal = %+v, token hash %q", p, th)
				}
			})
		}
	}
}

func TestGRPCInterceptorBackendError(t *testing.T) {
	h := newGRPCHarness(t, failingAuth{})
	ctx := metadata.AppendToOutgoingContext(context.Background(), "authorization", "Bearer wt_x")
	_, err := healthpb.NewHealthClient(h.conn).Check(ctx, &healthpb.HealthCheckRequest{})
	if status.Code(err) != codes.Internal {
		t.Fatalf("code = %v, want Internal", status.Code(err))
	}
	if strings.Contains(err.Error(), "fire") {
		t.Fatal("internal error details leaked to client")
	}
}

func TestHTTPMiddleware(t *testing.T) {
	e := newTestEnv(t, nil)
	alice := e.register(t, "alice")
	good, _, err := e.svc.CreateToken(context.Background(), alice.ID, "ci", 0)
	if err != nil {
		t.Fatal(err)
	}

	type observed struct {
		p          *domain.Principal
		tokenHash  string
		remoteAddr string
		rawQuery   string
		requestURI string
	}
	var mu sync.Mutex
	var last *observed
	next := http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		mu.Lock()
		last = &observed{PrincipalFrom(r.Context()), TokenHashFrom(r.Context()), RemoteAddrFrom(r.Context()), r.URL.RawQuery, r.RequestURI}
		mu.Unlock()
		w.WriteHeader(http.StatusTeapot)
	})
	srv := httptest.NewServer(Middleware(e.svc)(next))
	defer srv.Close()

	tests := []struct {
		name      string
		method    string
		path      string
		headers   []string
		wantCode  int
		wantUser  string
		wantQuery string
	}{
		{"anonymous", "GET", "/x", nil, http.StatusTeapot, "", ""},
		{"valid header", "GET", "/x", []string{"Authorization", "Bearer " + good}, http.StatusTeapot, "alice", ""},
		{"valid header POST", "POST", "/x", []string{"Authorization", "Bearer " + good}, http.StatusTeapot, "alice", ""},
		{"invalid header", "GET", "/x", []string{"Authorization", "Bearer wt_bad"}, http.StatusUnauthorized, "", ""},
		{"wrong scheme", "GET", "/x", []string{"Authorization", "Token " + good}, http.StatusUnauthorized, "", ""},
		{"empty header", "GET", "/x", []string{"Authorization", ""}, http.StatusTeapot, "", ""},
		{"duplicate headers", "GET", "/x", []string{"Authorization", "Bearer " + good, "Authorization", "Bearer " + good}, http.StatusUnauthorized, "", ""},
		{"query token GET", "GET", "/events?tenant=a&access_token=" + good + "&tree=b", nil, http.StatusTeapot, "alice", "tenant=a&tree=b"},
		{"query token invalid", "GET", "/events?access_token=wt_bad", nil, http.StatusUnauthorized, "", ""},
		{"query token empty", "GET", "/events?access_token=", nil, http.StatusUnauthorized, "", ""},
		{"query token ignored for POST", "POST", "/x?access_token=" + good, nil, http.StatusTeapot, "", "access_token=" + good},
		{"header wins over query", "GET", "/x?access_token=wt_bad", []string{"Authorization", "Bearer " + good}, http.StatusTeapot, "alice", "access_token=wt_bad"},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			mu.Lock()
			last = nil
			mu.Unlock()
			req, err := http.NewRequest(tt.method, srv.URL+tt.path, nil)
			if err != nil {
				t.Fatal(err)
			}
			for i := 0; i+1 < len(tt.headers); i += 2 {
				req.Header.Add(tt.headers[i], tt.headers[i+1])
			}
			resp, err := http.DefaultClient.Do(req)
			if err != nil {
				t.Fatal(err)
			}
			body, _ := io.ReadAll(resp.Body)
			_ = resp.Body.Close()
			if resp.StatusCode != tt.wantCode {
				t.Fatalf("status = %d (%s), want %d", resp.StatusCode, body, tt.wantCode)
			}
			mu.Lock()
			obs := last
			mu.Unlock()
			if tt.wantCode == http.StatusUnauthorized {
				if obs != nil {
					t.Fatal("handler ran despite invalid credentials")
				}
				var got struct {
					Error struct{ Code, Message string } `json:"error"`
				}
				if err := json.Unmarshal(body, &got); err != nil {
					t.Fatalf("body %q: %v", body, err)
				}
				if got.Error.Code != "unauthenticated" || got.Error.Message != "invalid or expired token" {
					t.Fatalf("body = %s", body)
				}
				if ct := resp.Header.Get("Content-Type"); !strings.HasPrefix(ct, "application/json") {
					t.Fatalf("Content-Type = %q", ct)
				}
				if resp.Header.Get("WWW-Authenticate") == "" {
					t.Fatal("missing WWW-Authenticate")
				}
				return
			}
			if obs == nil {
				t.Fatal("handler not called")
			}
			if obs.remoteAddr == "" {
				t.Fatal("remote addr not attached")
			}
			if obs.rawQuery != tt.wantQuery {
				t.Fatalf("downstream query = %q, want %q", obs.rawQuery, tt.wantQuery)
			}
			if strings.Contains(obs.requestURI, good) && tt.method == "GET" && tt.wantUser != "" && tt.headers == nil {
				t.Fatalf("token leaked in RequestURI %q", obs.requestURI)
			}
			if tt.wantUser == "" {
				if obs.p != nil || obs.tokenHash != "" {
					t.Fatalf("expected anonymous, got %+v", obs.p)
				}
				return
			}
			if obs.p == nil || obs.p.Username != tt.wantUser || obs.tokenHash != HashToken(good) {
				t.Fatalf("principal = %+v hash=%q", obs.p, obs.tokenHash)
			}
		})
	}
}

func TestHTTPMiddlewareBackendError(t *testing.T) {
	called := false
	h := Middleware(failingAuth{})(http.HandlerFunc(func(http.ResponseWriter, *http.Request) { called = true }))
	rec := httptest.NewRecorder()
	req := httptest.NewRequest(http.MethodGet, "/x", nil)
	req.Header.Set("Authorization", "Bearer wt_x")
	h.ServeHTTP(rec, req)
	if rec.Code != http.StatusInternalServerError || called {
		t.Fatalf("status = %d called=%v", rec.Code, called)
	}
	if !strings.Contains(rec.Body.String(), `"code":"internal"`) || strings.Contains(rec.Body.String(), "fire") {
		t.Fatalf("body = %s", rec.Body.String())
	}
}

func TestPrincipalContext(t *testing.T) {
	ctx := context.Background()
	if PrincipalFrom(ctx) != nil || RemoteAddrFrom(ctx) != "" || TokenHashFrom(ctx) != "" {
		t.Fatal("empty context has values")
	}
	p := &domain.Principal{AccountID: "a"}
	if got := PrincipalFrom(WithPrincipal(ctx, p)); got != p {
		t.Fatalf("PrincipalFrom = %+v", got)
	}
	if got := PrincipalFrom(WithPrincipal(ctx, nil)); got != nil {
		t.Fatalf("nil principal round trip = %+v", got)
	}
}

func TestParseBearer(t *testing.T) {
	tests := []struct {
		in, want string
		ok       bool
	}{
		{"Bearer abc", "abc", true},
		{"BEARER abc", "abc", true},
		{"  Bearer   abc  ", "abc", true},
		{"Bearer", "", false},
		{"Bearer ", "", false},
		{"Basic abc", "", false},
		{"abc", "", false},
	}
	for _, tt := range tests {
		got, err := parseBearer(tt.in)
		if (err == nil) != tt.ok || got != tt.want {
			t.Errorf("parseBearer(%q) = %q, %v", tt.in, got, err)
		}
	}
}
