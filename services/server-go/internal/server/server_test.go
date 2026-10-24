package server

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"io"
	"log/slog"
	"net"
	"net/http"
	"os"
	"strings"
	"sync"
	"testing"
	"time"

	"google.golang.org/grpc"
	"google.golang.org/grpc/credentials/insecure"
	healthpb "google.golang.org/grpc/health/grpc_health_v1"
	"google.golang.org/grpc/metadata"

	"github.com/seanfilimon/worktree/services/server-go/internal/config"
	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
)

var quietLog = slog.New(slog.NewTextHandler(io.Discard, nil))

// testSync is a minimal SyncService: Ping reports a version and
// DownloadObjects streams large messages or blocks until cancelled.
type testSync struct {
	syncpb.UnimplementedSyncServiceServer
	block   bool
	started chan struct{}
}

func (s *testSync) Ping(context.Context, *syncpb.PingRequest) (*syncpb.PingResponse, error) {
	return &syncpb.PingResponse{ServerVersion: "unit"}, nil
}

func (s *testSync) DownloadObjects(_ *syncpb.WantMessage, st grpc.ServerStreamingServer[syncpb.ObjectData]) error {
	if s.block {
		close(s.started)
		<-st.Context().Done()
		return st.Context().Err()
	}
	for i := 0; i < 3; i++ {
		if err := st.Send(&syncpb.ObjectData{Data: bytes.Repeat([]byte{byte(i)}, 10<<20)}); err != nil {
			return err
		}
	}
	return nil
}

// noAuth is an Authenticator that rejects every token.
type noAuth struct{ domain.Authenticator }

func (noAuth) Authenticate(context.Context, string) (*domain.Principal, error) {
	return nil, domain.ErrUnauthenticated
}

// startUnit serves a gRPC+HTTP mux like Run does, without the storage stack.
func startUnit(t *testing.T, svc *testSync) (addr string, stop func(timeout time.Duration) error) {
	t.Helper()
	g, hs := newGRPCServer(noAuth{}, svc, quietLog)
	api := http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("X-Proto", r.Proto)
		_, _ = io.WriteString(w, "hello "+r.URL.Path)
	})
	srv := newHTTPServer(mux(g, api), quietLog)
	ln, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	serveErr := make(chan error, 1)
	go func() { serveErr <- srv.Serve(ln) }()
	var once sync.Once
	var stopErr error
	stop = func(timeout time.Duration) error {
		once.Do(func() {
			hs.Shutdown()
			ctx, cancel := context.WithTimeout(context.Background(), timeout)
			defer cancel()
			stopErr = shutdown(ctx, quietLog, srv, g, serveErr)
		})
		return stopErr
	}
	t.Cleanup(func() { _ = stop(5 * time.Second) })
	return ln.Addr().String(), stop
}

func dialGRPC(t *testing.T, addr string) *grpc.ClientConn {
	t.Helper()
	conn, err := grpc.NewClient("passthrough:///"+addr,
		grpc.WithTransportCredentials(insecure.NewCredentials()),
		grpc.WithDefaultCallOptions(grpc.MaxCallRecvMsgSize(64<<20)))
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { conn.Close() })
	return conn
}

func TestSingleListenerServesGRPCAndHTTP(t *testing.T) {
	addr, stop := startUnit(t, &testSync{})
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()

	conn := dialGRPC(t, addr)
	pong, err := syncpb.NewSyncServiceClient(conn).Ping(ctx, &syncpb.PingRequest{})
	if err != nil || pong.GetServerVersion() != "unit" {
		t.Fatalf("Ping = %v, %v", pong, err)
	}
	hc, err := healthpb.NewHealthClient(conn).Check(ctx, &healthpb.HealthCheckRequest{Service: syncpb.SyncService_ServiceDesc.ServiceName})
	if err != nil || hc.GetStatus() != healthpb.HealthCheckResponse_SERVING {
		t.Fatalf("health = %v, %v", hc, err)
	}

	// Server streaming with messages above gRPC's 4 MiB default.
	stream, err := syncpb.NewSyncServiceClient(conn).DownloadObjects(ctx, &syncpb.WantMessage{})
	if err != nil {
		t.Fatal(err)
	}
	for i := 0; ; i++ {
		msg, err := stream.Recv()
		if errors.Is(err, io.EOF) {
			if i != 3 {
				t.Fatalf("got %d messages, want 3", i)
			}
			break
		}
		if err != nil {
			t.Fatalf("Recv: %v", err)
		}
		if len(msg.GetData()) != 10<<20 {
			t.Fatalf("message %d has %d bytes", i, len(msg.GetData()))
		}
	}

	// Plain HTTP/1.1 reaches the API handler.
	resp, err := http.Get("http://" + addr + "/healthz")
	if err != nil {
		t.Fatal(err)
	}
	body, _ := io.ReadAll(resp.Body)
	resp.Body.Close()
	if string(body) != "hello /healthz" || resp.Header.Get("X-Proto") != "HTTP/1.1" {
		t.Fatalf("http/1.1: %q %v", body, resp.Header)
	}

	// HTTP/2 prior knowledge without application/grpc also reaches the API.
	tr := &http.Transport{Protocols: new(http.Protocols)}
	tr.Protocols.SetUnencryptedHTTP2(true)
	defer tr.CloseIdleConnections()
	resp, err = (&http.Client{Transport: tr}).Get("http://" + addr + "/api/v1/x")
	if err != nil {
		t.Fatal(err)
	}
	body, _ = io.ReadAll(resp.Body)
	resp.Body.Close()
	if string(body) != "hello /api/v1/x" || resp.ProtoMajor != 2 {
		t.Fatalf("h2c: %q proto %s", body, resp.Proto)
	}

	if err := stop(5 * time.Second); err != nil {
		t.Fatalf("shutdown: %v", err)
	}
	if _, err := net.DialTimeout("tcp", addr, time.Second); err == nil {
		t.Fatal("listener still accepting after shutdown")
	}
}

// A stream that never finishes must not hang (or panic) shutdown: after the
// timeout the server closes it forcibly.
func TestShutdownForcesStuckStreams(t *testing.T) {
	svc := &testSync{block: true, started: make(chan struct{})}
	addr, stop := startUnit(t, svc)
	conn := dialGRPC(t, addr)
	stream, err := syncpb.NewSyncServiceClient(conn).DownloadObjects(context.Background(), &syncpb.WantMessage{})
	if err != nil {
		t.Fatal(err)
	}
	recvErr := make(chan error, 1)
	go func() {
		_, err := stream.Recv()
		recvErr <- err
	}()
	select {
	case <-svc.started:
	case <-time.After(5 * time.Second):
		t.Fatal("stream did not start")
	}

	start := time.Now()
	if err := stop(300 * time.Millisecond); err != nil {
		t.Fatalf("shutdown: %v", err)
	}
	if d := time.Since(start); d > 5*time.Second {
		t.Fatalf("shutdown took %v", d)
	}
	select {
	case err := <-recvErr:
		if err == nil {
			t.Fatal("stream ended without error")
		}
	case <-time.After(5 * time.Second):
		t.Fatal("client stream not terminated")
	}
}

func TestMuxRouting(t *testing.T) {
	tests := []struct {
		proto int
		ct    string
		grpc  bool
	}{
		{2, "application/grpc", true},
		{2, "application/grpc+proto", true},
		{1, "application/grpc", false}, // gRPC requires HTTP/2
		{2, "application/json", false},
		{2, "", false},
	}
	for _, tc := range tests {
		var hitAPI bool
		h := mux(grpc.NewServer(), http.HandlerFunc(func(http.ResponseWriter, *http.Request) { hitAPI = true }))
		r, _ := http.NewRequest("POST", "/x", nil)
		r.ProtoMajor = tc.proto
		r.Header.Set("Content-Type", tc.ct)
		rec := &discardWriter{h: http.Header{}}
		h.ServeHTTP(rec, r)
		if hitAPI == tc.grpc {
			t.Errorf("proto %d ct %q: routed to API = %v, want %v", tc.proto, tc.ct, hitAPI, !tc.grpc)
		}
	}
}

type discardWriter struct{ h http.Header }

func (d *discardWriter) Header() http.Header         { return d.h }
func (d *discardWriter) Write(p []byte) (int, error) { return len(p), nil }
func (d *discardWriter) WriteHeader(int)             {}

// syncBuffer is a goroutine-safe bytes.Buffer.
type syncBuffer struct {
	mu sync.Mutex
	b  bytes.Buffer
}

func (s *syncBuffer) Write(p []byte) (int, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.b.Write(p)
}

func (s *syncBuffer) String() string {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.b.String()
}

// TestRunIntegration boots the real server (bbolt, IAM, sync service...)
// on an ephemeral port. It needs the real implementations of every
// package, so it only runs with WT_INTEGRATION=1.
func TestRunIntegration(t *testing.T) {
	if os.Getenv("WT_INTEGRATION") == "" {
		t.Skip("set WT_INTEGRATION=1 to run the server integration test")
	}
	var stdout, logs syncBuffer
	addrCh := make(chan net.Addr, 1)
	cfg := config.Config{
		Addr:              "127.0.0.1:0",
		DataDir:           t.TempDir(),
		AdminUser:         "admin",
		AdminPassword:     "integration-password",
		AllowRegistration: true,
		LogLevel:          slog.LevelDebug,
		LogFormat:         "text",
		ShutdownTimeout:   5 * time.Second,
		Version:           "9.9.9-test",
		Stdout:            &stdout,
		LogOutput:         &logs,
		OnListen:          func(a net.Addr) { addrCh <- a },
	}
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	runErr := make(chan error, 1)
	go func() { runErr <- Run(ctx, cfg) }()

	var addr string
	select {
	case a := <-addrCh:
		addr = a.String()
	case err := <-runErr:
		t.Fatalf("Run exited early: %v\nlogs:\n%s", err, logs.String())
	case <-time.After(15 * time.Second):
		t.Fatalf("server did not start\nlogs:\n%s", logs.String())
	}
	if !strings.Contains(stdout.String(), "worktree-server listening on "+addr) {
		t.Fatalf("startup line missing from stdout: %q", stdout.String())
	}
	base := "http://" + addr

	// REST: health.
	resp, err := http.Get(base + "/healthz")
	if err != nil {
		t.Fatal(err)
	}
	var health struct {
		Status  string `json:"status"`
		Version string `json:"version"`
	}
	err = json.NewDecoder(resp.Body).Decode(&health)
	resp.Body.Close()
	if err != nil || resp.StatusCode != 200 || health.Status != "ok" || health.Version != cfg.Version {
		t.Fatalf("healthz: %d %+v %v", resp.StatusCode, health, err)
	}

	// REST: the ensured admin can log in.
	resp, err = http.Post(base+"/api/v1/auth/login", "application/json",
		strings.NewReader(`{"username":"admin","password":"integration-password"}`))
	if err != nil {
		t.Fatal(err)
	}
	var session struct {
		Token   string         `json:"token"`
		Account domain.Account `json:"account"`
	}
	err = json.NewDecoder(resp.Body).Decode(&session)
	resp.Body.Close()
	if err != nil || resp.StatusCode != 200 || session.Token == "" || !session.Account.IsAdmin {
		t.Fatalf("login: %d %+v %v", resp.StatusCode, session, err)
	}

	// gRPC over h2c on the same port: anonymous and authenticated Ping.
	conn := dialGRPC(t, addr)
	client := syncpb.NewSyncServiceClient(conn)
	rctx, rcancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer rcancel()
	pong, err := client.Ping(rctx, &syncpb.PingRequest{})
	if err != nil || pong.GetServerVersion() != cfg.Version {
		t.Fatalf("Ping = %v, %v", pong, err)
	}
	authed := metadata.AppendToOutgoingContext(rctx, "authorization", "Bearer "+session.Token)
	pong, err = client.Ping(authed, &syncpb.PingRequest{})
	if err != nil || pong.GetUsername() != "admin" {
		t.Fatalf("authenticated Ping = %v, %v", pong, err)
	}
	hc, err := healthpb.NewHealthClient(conn).Check(rctx, &healthpb.HealthCheckRequest{})
	if err != nil || hc.GetStatus() != healthpb.HealthCheckResponse_SERVING {
		t.Fatalf("health = %v, %v", hc, err)
	}

	// REST via the admin token.
	req, _ := http.NewRequest("GET", base+"/api/v1/admin/stats", nil)
	req.Header.Set("Authorization", "Bearer "+session.Token)
	resp, err = http.DefaultClient.Do(req)
	if err != nil {
		t.Fatal(err)
	}
	resp.Body.Close()
	if resp.StatusCode != 200 {
		t.Fatalf("admin stats: %d", resp.StatusCode)
	}

	// Graceful shutdown.
	cancel()
	select {
	case err := <-runErr:
		if err != nil {
			t.Fatalf("Run: %v\nlogs:\n%s", err, logs.String())
		}
	case <-time.After(15 * time.Second):
		t.Fatal("Run did not return after cancel")
	}
	if _, err := net.DialTimeout("tcp", addr, time.Second); err == nil {
		t.Fatal("listener still open after shutdown")
	}
}
