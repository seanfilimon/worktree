package server

import (
	"context"
	"crypto/ecdsa"
	"crypto/elliptic"
	"crypto/rand"
	"crypto/tls"
	"crypto/x509"
	"crypto/x509/pkix"
	"encoding/json"
	"encoding/pem"
	"math/big"
	"net"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"testing"
	"time"

	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/credentials"
	"google.golang.org/grpc/status"

	"github.com/seanfilimon/worktree/services/server-go/internal/authz"
	"github.com/seanfilimon/worktree/services/server-go/internal/config"
	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/iam"
	"github.com/seanfilimon/worktree/services/server-go/internal/limits"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
	"github.com/seanfilimon/worktree/services/server-go/internal/opsmetrics"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
)

// ---- TLS -------------------------------------------------------------------

type testPKI struct {
	caPEM                 []byte
	caPool                *x509.CertPool
	serverCert, serverKey string // files
	clientCA              string // file
	client                tls.Certificate
	caCert                *x509.Certificate
	caKey                 *ecdsa.PrivateKey
}

func newPKI(t *testing.T) *testPKI {
	t.Helper()
	dir := t.TempDir()
	caKey, err := ecdsa.GenerateKey(elliptic.P256(), rand.Reader)
	must(t, err)
	caTmpl := &x509.Certificate{
		SerialNumber: big.NewInt(1), Subject: pkix.Name{CommonName: "wt test CA"},
		NotBefore: time.Now().Add(-time.Hour), NotAfter: time.Now().Add(time.Hour),
		IsCA: true, BasicConstraintsValid: true, KeyUsage: x509.KeyUsageCertSign | x509.KeyUsageDigitalSignature,
	}
	caDER, err := x509.CreateCertificate(rand.Reader, caTmpl, caTmpl, &caKey.PublicKey, caKey)
	must(t, err)
	caCert, err := x509.ParseCertificate(caDER)
	must(t, err)
	p := &testPKI{caPEM: pem.EncodeToMemory(&pem.Block{Type: "CERTIFICATE", Bytes: caDER}), caCert: caCert, caKey: caKey}
	p.caPool = x509.NewCertPool()
	p.caPool.AddCert(caCert)

	serverPEM, serverKeyPEM := p.leaf(t, 2, "localhost", x509.ExtKeyUsageServerAuth)
	p.serverCert, p.serverKey = filepath.Join(dir, "server.pem"), filepath.Join(dir, "server.key")
	must(t, os.WriteFile(p.serverCert, serverPEM, 0o600))
	must(t, os.WriteFile(p.serverKey, serverKeyPEM, 0o600))
	p.clientCA = filepath.Join(dir, "ca.pem")
	must(t, os.WriteFile(p.clientCA, p.caPEM, 0o600))
	clientPEM, clientKeyPEM := p.leaf(t, 3, "client", x509.ExtKeyUsageClientAuth)
	p.client, err = tls.X509KeyPair(clientPEM, clientKeyPEM)
	must(t, err)
	return p
}

func (p *testPKI) leaf(t *testing.T, serial int64, cn string, usage x509.ExtKeyUsage) (certPEM, keyPEM []byte) {
	t.Helper()
	key, err := ecdsa.GenerateKey(elliptic.P256(), rand.Reader)
	must(t, err)
	tmpl := &x509.Certificate{
		SerialNumber: big.NewInt(serial), Subject: pkix.Name{CommonName: cn},
		NotBefore: time.Now().Add(-time.Hour), NotAfter: time.Now().Add(time.Hour),
		KeyUsage: x509.KeyUsageDigitalSignature, ExtKeyUsage: []x509.ExtKeyUsage{usage},
		DNSNames: []string{"localhost"}, IPAddresses: []net.IP{net.ParseIP("127.0.0.1")},
	}
	der, err := x509.CreateCertificate(rand.Reader, tmpl, p.caCert, &key.PublicKey, p.caKey)
	must(t, err)
	kder, err := x509.MarshalECPrivateKey(key)
	must(t, err)
	return pem.EncodeToMemory(&pem.Block{Type: "CERTIFICATE", Bytes: der}), pem.EncodeToMemory(&pem.Block{Type: "EC PRIVATE KEY", Bytes: kder})
}

func must(t *testing.T, err error) {
	t.Helper()
	if err != nil {
		t.Fatal(err)
	}
}

func TestTLSConfig(t *testing.T) {
	if c, err := tlsConfig(config.Config{}); c != nil || err != nil {
		t.Fatalf("cleartext: %v %v", c, err)
	}
	p := newPKI(t)
	c, err := tlsConfig(config.Config{TLSCert: p.serverCert, TLSKey: p.serverKey})
	must(t, err)
	if c.ClientAuth != tls.NoClientCert || len(c.NextProtos) != 2 || c.NextProtos[0] != "h2" {
		t.Fatalf("tls config = %+v", c)
	}
	c, err = tlsConfig(config.Config{TLSCert: p.serverCert, TLSKey: p.serverKey, TLSClientCA: p.clientCA})
	must(t, err)
	if c.ClientAuth != tls.RequireAndVerifyClientCert || c.ClientCAs == nil {
		t.Fatalf("mtls config = %+v", c)
	}
	bad := filepath.Join(t.TempDir(), "bad.pem")
	must(t, os.WriteFile(bad, []byte("not pem"), 0o600))
	if _, err := tlsConfig(config.Config{TLSCert: p.serverCert, TLSKey: p.serverKey, TLSClientCA: bad}); err == nil {
		t.Fatal("bad client CA accepted")
	}
	if _, err := tlsConfig(config.Config{TLSCert: bad, TLSKey: p.serverKey}); err == nil {
		t.Fatal("bad certificate accepted")
	}
}

// TestRunTLSAndAudit boots the real server over mTLS: HTTPS and gRPC share
// the port, clients without a certificate are refused, and server.start /
// server.stop are audited.
func TestRunTLSAndAudit(t *testing.T) {
	p := newPKI(t)
	var logs syncBuffer
	addrCh := make(chan net.Addr, 1)
	dataDir := t.TempDir()
	cfg := config.Config{
		Addr: "127.0.0.1:0", DataDir: dataDir, AdminUser: "admin", AllowRegistration: true,
		LogLevel: -4, LogFormat: "text", ShutdownTimeout: 5 * time.Second, Version: "tls-test",
		Stdout: &syncBuffer{}, LogOutput: &logs, OnListen: func(a net.Addr) { addrCh <- a },
		TLSCert: p.serverCert, TLSKey: p.serverKey, TLSClientCA: p.clientCA,
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
		t.Fatalf("Run exited early: %v\n%s", err, logs.String())
	case <-time.After(15 * time.Second):
		t.Fatalf("server did not start\n%s", logs.String())
	}
	if strings.Contains(logs.String(), "cleartext") {
		t.Fatal("cleartext warning logged for a TLS loopback server")
	}

	clientTLS := &tls.Config{RootCAs: p.caPool, Certificates: []tls.Certificate{p.client}, ServerName: "localhost"}
	hc := &http.Client{Transport: &http.Transport{TLSClientConfig: clientTLS, ForceAttemptHTTP2: true}, Timeout: 10 * time.Second}
	resp, err := hc.Get("https://" + addr + "/healthz")
	must(t, err)
	var health struct {
		Status string            `json:"status"`
		Checks map[string]string `json:"checks"`
	}
	must(t, json.NewDecoder(resp.Body).Decode(&health))
	resp.Body.Close()
	if resp.StatusCode != 200 || health.Status != "ok" || health.Checks["meta"] != "ok" || resp.ProtoMajor != 2 {
		t.Fatalf("healthz over TLS: %d %s %+v", resp.StatusCode, resp.Proto, health)
	}

	// No client certificate: the handshake is refused.
	noCert := &http.Client{Transport: &http.Transport{TLSClientConfig: &tls.Config{RootCAs: p.caPool, ServerName: "localhost"}}, Timeout: 10 * time.Second}
	if resp, err := noCert.Get("https://" + addr + "/healthz"); err == nil {
		resp.Body.Close()
		t.Fatal("request without a client certificate succeeded")
	}

	// gRPC over TLS (ALPN h2) on the same port.
	conn, err := grpc.NewClient("passthrough:///"+addr, grpc.WithTransportCredentials(credentials.NewTLS(clientTLS)))
	must(t, err)
	defer conn.Close()
	rctx, rcancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer rcancel()
	pong, err := syncpb.NewSyncServiceClient(conn).Ping(rctx, &syncpb.PingRequest{})
	if err != nil || pong.GetServerVersion() != "tls-test" {
		t.Fatalf("Ping over TLS = %v, %v", pong, err)
	}

	cancel()
	select {
	case err := <-runErr:
		must(t, err)
	case <-time.After(15 * time.Second):
		t.Fatal("Run did not return")
	}
	store, err := meta.NewBolt(filepath.Join(dataDir, "meta.db"))
	must(t, err)
	defer store.Close()
	evs, err := store.ListAudit(context.Background(), domain.AuditFilter{Action: "server.*"})
	must(t, err)
	if len(evs) != 2 || evs[0].Action != "server.stop" || evs[1].Action != "server.start" ||
		evs[1].Actor != "system" || evs[1].Details["tls"] != "true" {
		t.Fatalf("server lifecycle audit = %+v", evs)
	}
}

// ---- interceptors ------------------------------------------------------------

type recAudit struct {
	mu  sync.Mutex
	evs []domain.AuditEvent
}

func (a *recAudit) Record(_ context.Context, e domain.AuditEvent) {
	a.mu.Lock()
	a.evs = append(a.evs, e)
	a.mu.Unlock()
}

func (a *recAudit) Query(context.Context, domain.AuditFilter) ([]domain.AuditEvent, error) {
	return nil, nil
}

func TestObserveInterceptor(t *testing.T) {
	log := &recAudit{}
	obs := observeUnary(log)
	ctx := iam.WithPrincipal(context.Background(), &domain.Principal{AccountID: "a1", Username: "alice"})
	push := &grpc.UnaryServerInfo{FullMethod: syncpb.SyncService_Push_FullMethodName}

	acc, rej, errs := opsmetrics.SyncPushes.Get("accepted"), opsmetrics.SyncPushes.Get("rejected"), opsmetrics.SyncPushes.Get("error")
	_, _ = obs(ctx, nil, push, func(context.Context, any) (any, error) { return &syncpb.PushResponse{Accepted: true}, nil })
	_, _ = obs(ctx, nil, push, func(context.Context, any) (any, error) { return &syncpb.PushResponse{}, nil })
	_, _ = obs(ctx, nil, push, func(context.Context, any) (any, error) {
		return nil, status.Error(codes.PermissionDenied, "no write access")
	})
	if opsmetrics.SyncPushes.Get("accepted") != acc+1 || opsmetrics.SyncPushes.Get("rejected") != rej+1 || opsmetrics.SyncPushes.Get("error") != errs+1 {
		t.Fatal("push results not counted")
	}
	pulls := opsmetrics.SyncPulls.Get("")
	_, _ = obs(ctx, nil, &grpc.UnaryServerInfo{FullMethod: syncpb.SyncService_Pull_FullMethodName},
		func(context.Context, any) (any, error) { return &syncpb.PullResponse{}, nil })
	if opsmetrics.SyncPulls.Get("") != pulls+1 {
		t.Fatal("pull not counted")
	}
	if len(log.evs) != 1 {
		t.Fatalf("audit = %+v", log.evs)
	}
	e := log.evs[0]
	if e.Action != "access.denied" || e.Actor != "alice" || e.ResourceType != "rpc" || e.Details["method"] != syncpb.SyncService_Push_FullMethodName ||
		e.Details["message"] != "no write access" {
		t.Fatalf("access.denied = %+v", e)
	}
	// Other failures are not access denials.
	_, _ = obs(ctx, nil, push, func(context.Context, any) (any, error) { return nil, status.Error(codes.NotFound, "x") })
	if len(log.evs) != 1 {
		t.Fatal("NotFound audited as access.denied")
	}
}

func TestRateLimitInterceptor(t *testing.T) {
	store := meta.NewMemory()
	defer store.Close()
	l := limits.New(limits.Deps{Store: store, RateLimit: 2, Logger: quietLog})
	ic := rateLimitUnary(l)
	ok := func(context.Context, any) (any, error) { return "ok", nil }
	ctx := iam.WithPrincipal(context.Background(), &domain.Principal{AccountID: "a1", Username: "alice"})
	for i := 0; i < 2; i++ {
		if _, err := ic(ctx, nil, &grpc.UnaryServerInfo{}, ok); err != nil {
			t.Fatalf("call %d: %v", i, err)
		}
	}
	before := opsmetrics.RateLimited.Get("")
	_, err := ic(ctx, nil, &grpc.UnaryServerInfo{}, ok)
	if status.Code(err) != codes.ResourceExhausted || !strings.Contains(err.Error(), "2 requests per minute") {
		t.Fatalf("third call: %v", err)
	}
	if opsmetrics.RateLimited.Get("") != before+1 {
		t.Fatal("rate limited call not counted")
	}
	// Disabled limiter / nil limiter pass everything.
	for _, ic := range []grpc.UnaryServerInterceptor{rateLimitUnary(nil), rateLimitUnary(limits.New(limits.Deps{Store: store}))} {
		for i := 0; i < 10; i++ {
			if _, err := ic(ctx, nil, &grpc.UnaryServerInfo{}, ok); err != nil {
				t.Fatal(err)
			}
		}
	}
}

func TestCountingAuthz(t *testing.T) {
	store := meta.NewMemory()
	defer store.Close()
	a := countingAuthz{authz.New(store, authz.Options{})}
	var _ domain.AccessEvaluator = a // optional interfaces survive the wrapper
	allow, deny := opsmetrics.AuthzDecisions.Get("allow"), opsmetrics.AuthzDecisions.Get("deny")
	ctx := context.Background()
	must(t, store.CreateAccount(ctx, &domain.Account{ID: "root", Username: "root", Email: "r@x", IsAdmin: true}))
	if err := a.Authorize(ctx, &domain.Principal{AccountID: "x", Username: "x"}, domain.ActServerAdmin, "", nil); err == nil {
		t.Fatal("non-admin authorized as server admin")
	}
	if err := a.Authorize(ctx, &domain.Principal{AccountID: "root", Username: "root", IsAdmin: true}, domain.ActServerAdmin, "", nil); err != nil {
		t.Fatal(err)
	}
	if opsmetrics.AuthzDecisions.Get("allow") != allow+1 || opsmetrics.AuthzDecisions.Get("deny") != deny+1 {
		t.Fatalf("decisions allow %d->%d deny %d->%d", allow, opsmetrics.AuthzDecisions.Get("allow"), deny, opsmetrics.AuthzDecisions.Get("deny"))
	}
}
