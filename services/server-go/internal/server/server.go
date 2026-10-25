// Package server wires the worktree-server components together and runs
// them on a single listener: gRPC (HTTP/2 cleartext, "h2c") for the sync
// protocol and HTTP/1.1 + HTTP/2 for the REST API, SSE and admin panel.
package server

import (
	"context"
	"errors"
	"fmt"
	"io"
	"log/slog"
	"net"
	"net/http"
	"os"
	"os/signal"
	"path/filepath"
	"runtime/debug"
	"strconv"
	"strings"
	"syscall"
	"time"

	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/health"
	healthpb "google.golang.org/grpc/health/grpc_health_v1"
	"google.golang.org/grpc/status"

	"github.com/seanfilimon/worktree/services/server-go/internal/accesscfg"
	"github.com/seanfilimon/worktree/services/server-go/internal/audit"
	"github.com/seanfilimon/worktree/services/server-go/internal/authz"
	"github.com/seanfilimon/worktree/services/server-go/internal/ci"
	"github.com/seanfilimon/worktree/services/server-go/internal/codesearch"
	"github.com/seanfilimon/worktree/services/server-go/internal/config"
	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/events"
	"github.com/seanfilimon/worktree/services/server-go/internal/gc"
	"github.com/seanfilimon/worktree/services/server-go/internal/httpapi"
	"github.com/seanfilimon/worktree/services/server-go/internal/iam"
	"github.com/seanfilimon/worktree/services/server-go/internal/license"
	"github.com/seanfilimon/worktree/services/server-go/internal/limits"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
	"github.com/seanfilimon/worktree/services/server-go/internal/objstore"
	"github.com/seanfilimon/worktree/services/server-go/internal/opsmetrics"
	"github.com/seanfilimon/worktree/services/server-go/internal/pathacl"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
	"github.com/seanfilimon/worktree/services/server-go/internal/release"
	"github.com/seanfilimon/worktree/services/server-go/internal/review"
	"github.com/seanfilimon/worktree/services/server-go/internal/signing"
	"github.com/seanfilimon/worktree/services/server-go/internal/sso"
	"github.com/seanfilimon/worktree/services/server-go/internal/staged"
	"github.com/seanfilimon/worktree/services/server-go/internal/syncsvc"
	"github.com/seanfilimon/worktree/services/server-go/internal/todo"
	"github.com/seanfilimon/worktree/services/server-go/internal/webhooks"
)

// maxGRPCMessage is the gRPC message size limit in both directions.
const maxGRPCMessage = 64 << 20

// Run starts the server described by cfg and blocks until ctx is cancelled
// or SIGINT/SIGTERM is received, then shuts down gracefully within
// cfg.ShutdownTimeout. It returns nil after a clean shutdown.
func Run(ctx context.Context, cfg config.Config) (err error) {
	ctx, stopSignals := signal.NotifyContext(ctx, os.Interrupt, syscall.SIGTERM)
	defer stopSignals()

	stdout := cfg.Stdout
	if stdout == nil {
		stdout = os.Stdout
	}
	log := newLogger(cfg)
	if cfg.ShutdownTimeout <= 0 {
		cfg.ShutdownTimeout = config.DefaultShutdownTimeout
	}
	fmt.Fprintf(stdout, "worktree-server version %s\n", cfg.Version)

	// ---- storage ----
	if err := os.MkdirAll(cfg.DataDir, 0o750); err != nil {
		return fmt.Errorf("create data directory: %w", err)
	}
	store, err := meta.NewBolt(filepath.Join(cfg.DataDir, "meta.db"))
	if err != nil {
		return fmt.Errorf("open metadata store: %w", err)
	}
	defer func() {
		if cerr := store.Close(); cerr != nil {
			err = errors.Join(err, fmt.Errorf("close metadata store: %w", cerr))
		}
	}()
	// DiskStore lays objects out as <root>/objects/<2-hex>/<62-hex>, so the
	// data directory itself is the root.
	objects := objstore.New(cfg.DataDir)
	history := meta.NewHistory(store)

	// ---- services ----
	iamSvc := iam.New(store, iam.Options{AllowRegistration: cfg.AllowRegistration})
	if cfg.AdminPassword != "" {
		if err := iam.EnsureAdmin(ctx, store, iamSvc, cfg.AdminUser, cfg.AdminPassword); err != nil {
			return fmt.Errorf("ensure admin account %q: %w", cfg.AdminUser, err)
		}
		log.Info("admin account ensured", slog.String("username", cfg.AdminUser))
	}
	// Authorization (roles, teams, grants, service accounts, policies) is
	// decided by the v0.4 engine; iamSvc handles authentication only.
	// countingAuthz feeds worktree_authz_decisions_total (§13.3).
	authorizer := countingAuthz{authz.New(store, authz.Options{})}
	bus := events.NewBus()
	auditLog := audit.New(store, log)
	lic := license.New(store)
	rev := review.New(store, history, authorizer, bus, auditLog)
	stg := staged.New(store, authorizer, bus)

	// v0.5 components (docs/ENGINEERING_PLAN.md §13).
	v5 := func() (d struct {
		Store   domain.MetaStore
		Objects objstore.Store
		History domain.History
		Authz   domain.Authorizer
		Bus     domain.EventBus
		Audit   domain.AuditLog
		Logger  *slog.Logger
	}) {
		d.Store, d.Objects, d.History, d.Authz, d.Bus, d.Audit, d.Logger = store, objects, history, authorizer, bus, auditLog, log
		return d
	}()
	todos := todo.New(todo.Deps(v5))
	releases := release.New(release.Deps(v5))
	ciSvc := ci.New(ci.Deps(v5))
	signer := signing.New(signing.Deps(v5))
	collector := gc.New(gc.Deps{
		Store: store, Objects: objects, History: history, Authz: authorizer, Bus: bus, Audit: auditLog, Logger: log,
		Interval: cfg.GCInterval, Grace: cfg.GCGrace, StagedRetention: cfg.StagedRetention, AuditRetention: cfg.AuditRetention,
	})
	limiter := limits.New(limits.Deps{
		Store: store, Objects: objects, History: history, Authz: authorizer, Bus: bus, Audit: auditLog, Logger: log,
		Defaults:  domain.TenantLimits{MaxTrees: cfg.DefaultMaxTrees, MaxStorageBytes: cfg.DefaultMaxStorage},
		RateLimit: cfg.RateLimit,
	})
	accessCfg := accesscfg.New(accesscfg.Deps(v5))
	// v0.6 components (docs/ENGINEERING_PLAN.md §14).
	hooks := webhooks.New(webhooks.Deps(v5))
	codeIndex := codesearch.New(codesearch.Deps(v5))
	go accessCfg.Run(ctx) // access workstream: drops access docs of deleted trees (§13.14)
	pathACL := pathacl.New(pathacl.Deps(v5))
	// review-ci (§13.4): tree.deleted cleanup of tree-scoped documents.
	rev.SetLogger(log)
	rev.Start(ctx)
	ciSvc.Start(ctx)
	// deps-releases (§13.5): bus subscribers creating TODOs from pushed
	// dependencies and cleaning up TODOs/releases of deleted trees.
	todos.Start(ctx)
	releases.Start(ctx)
	// webhooks (§14.2): bus-driven deliveries, retries and log retention.
	hooks.Configure(webhooks.Options{AllowPrivate: cfg.WebhookAllowPrivate, Version: cfg.Version})
	hooks.Start(ctx)

	syncSvc := syncsvc.New(syncsvc.Deps{
		Store:   store,
		Objects: objects,
		History: history,
		Authz:   authorizer,
		Review:  rev,
		License: lic,
		Staged:  stg,
		Bus:     bus,
		Audit:   auditLog,
		Version: cfg.Version,

		PushChecks: []domain.PushChecker{
			pathACL, // sec-paths (§13.1): path ACLs run first
			rev,     // review-ci (§13.4): RequireLinear on direct pushes
			limiter, // ops (§13.3): branch and storage quotas
			signer,  // signing (§13.6): verifies signatures, sets Verified; keep last
		},
		AccessCfg: accessCfg,
		// ops (§13.3): MaxTrees also applies to trees created by a push.
		TreeQuota: limiter,

		// daemon (§13.11): ends WatchBranches streams on shutdown.
		Done: ctx.Done(),
	})

	// ---- gRPC ----
	grpcServer, healthSrv := newGRPCServer(iamSvc, syncSvc, log, grpcOps{limiter: limiter, audit: auditLog})

	// ---- HTTP ----
	api := httpapi.New(httpapi.Deps{
		Store:       store,
		Objects:     objects,
		History:     history,
		Auth:        iamSvc,
		Authz:       authorizer,
		Review:      rev,
		License:     lic,
		Staged:      stg,
		Bus:         bus,
		Audit:       auditLog,
		Version:     cfg.Version,
		AdminDir:    cfg.AdminDir,
		StartedAt:   time.Now(),
		Logger:      log,
		CORSOrigins: cfg.CORSOrigins,
		Done:        ctx.Done(),

		AllowRegistration: cfg.AllowRegistration,
		DataDir:           cfg.DataDir,

		Security: iamSvc,
		Access:   authorizer,
		SSO:      sso.New(store, iamSvc, iamSvc, sso.Options{PublicURL: cfg.PublicURL, Logger: log}),

		Todos:     todos,
		Releases:  releases,
		CI:        ciSvc,
		Signing:   signer,
		GC:        collector,
		Limits:    limiter,
		AccessCfg: accessCfg,

		// v0.6 (§14).
		Webhooks:   hooks,
		CodeSearch: codeIndex,
	})

	proxies, _ := config.ParseProxies(cfg.TrustedProxies) // validated by Load
	srv := newHTTPServer(forwardedFor(mux(grpcServer, api), proxies), log)
	tlsCfg, err := tlsConfig(cfg)
	if err != nil {
		return err
	}
	if tlsCfg != nil {
		srv.TLSConfig = tlsCfg
		srv.Protocols.SetHTTP2(true)
	}

	ln, err := net.Listen("tcp", cfg.Addr)
	if err != nil {
		return fmt.Errorf("listen on %s: %w", cfg.Addr, err)
	}
	addr := ln.Addr()

	serveErr := make(chan error, 1)
	if tlsCfg != nil {
		go func() { serveErr <- srv.ServeTLS(ln, "", "") }()
	} else {
		go func() { serveErr <- srv.Serve(ln) }()
	}

	// ---- background operations (§13.3) ----
	stopGC := collector.Start(ctx)
	stopUsage := limiter.Start(ctx)
	stopMerges := opsmetrics.CountMerges(bus)
	defer func() { stopMerges(); stopUsage(); stopGC() }()

	scheme := "http"
	if tlsCfg != nil {
		scheme = "https"
	}
	log.Info("server started",
		slog.String("addr", addr.String()),
		slog.String("scheme", scheme),
		slog.Bool("mtls", cfg.TLSClientCA != ""),
		slog.String("version", cfg.Version),
		slog.String("data", cfg.DataDir),
		slog.Bool("admin_panel", cfg.AdminDir != ""),
		slog.Bool("allow_registration", cfg.AllowRegistration))
	if cfg.CleartextExposed() {
		log.Warn("serving cleartext HTTP and gRPC on a non-loopback address; credentials travel unencrypted — use --tls-cert/--tls-key or a TLS-terminating proxy",
			slog.String("addr", cfg.Addr))
	}
	auditLog.Record(ctx, audit.System(audit.ActionServerStart, "server", "", map[string]string{
		"version": cfg.Version, "addr": addr.String(), "tls": strconv.FormatBool(tlsCfg != nil)}))
	fmt.Fprintf(stdout, "worktree-server listening on %s\n", addr)
	if cfg.OnListen != nil {
		cfg.OnListen(addr)
	}

	select {
	case err := <-serveErr:
		// Serve failed before shutdown was requested.
		grpcServer.Stop()
		return fmt.Errorf("serve: %w", err)
	case <-ctx.Done():
	}

	auditLog.Record(context.WithoutCancel(ctx), audit.System(audit.ActionServerStop, "server", "", map[string]string{"version": cfg.Version}))
	log.Info("shutting down", slog.Duration("timeout", cfg.ShutdownTimeout))
	healthSrv.Shutdown() // report NOT_SERVING to health checkers
	shutdownCtx, cancel := context.WithTimeout(context.WithoutCancel(ctx), cfg.ShutdownTimeout)
	defer cancel()
	return shutdown(shutdownCtx, log, srv, grpcServer, serveErr)
}

// shutdown stops the HTTP server (which carries all gRPC traffic through
// ServeHTTP) and then the gRPC server.
//
// Ordering matters: grpc.Server.GracefulStop must not run while ServeHTTP
// streams are active (their transport does not support draining), so it
// only runs after http.Server.Shutdown reports that every connection has
// finished. On timeout everything is closed forcibly.
func shutdown(ctx context.Context, log *slog.Logger, srv *http.Server, g *grpc.Server, serveErr <-chan error) error {
	httpErr := srv.Shutdown(ctx)
	if httpErr != nil {
		log.Warn("graceful HTTP shutdown timed out; closing remaining connections", slog.Any("error", httpErr))
		_ = srv.Close()
		g.Stop()
	} else {
		done := make(chan struct{})
		go func() { g.GracefulStop(); close(done) }()
		select {
		case <-done:
		case <-ctx.Done():
			g.Stop()
			<-done
		}
	}
	if err := <-serveErr; err != nil && !errors.Is(err, http.ErrServerClosed) {
		return fmt.Errorf("serve: %w", err)
	}
	log.Info("server stopped")
	if httpErr != nil && !errors.Is(httpErr, context.DeadlineExceeded) {
		return fmt.Errorf("shutdown: %w", httpErr)
	}
	return nil
}

// newGRPCServer builds the gRPC server with the auth and panic-recovery
// interceptors, 64 MiB message limits, the sync service and the standard
// grpc.health.v1 service (reporting SERVING).
func newGRPCServer(auth domain.Authenticator, sync syncpb.SyncServiceServer, log *slog.Logger, ops grpcOps) (*grpc.Server, *health.Server) {
	// After authentication: sync metrics + access.denied auditing, then
	// rate limiting (§13.3).
	g := grpc.NewServer(
		grpc.MaxRecvMsgSize(maxGRPCMessage),
		grpc.MaxSendMsgSize(maxGRPCMessage),
		grpc.ChainUnaryInterceptor(recoverUnary(log), iam.UnaryInterceptor(auth), observeUnary(ops.audit), rateLimitUnary(ops.limiter)),
		grpc.ChainStreamInterceptor(recoverStream(log), iam.StreamInterceptor(auth), observeStream(ops.audit), rateLimitStream(ops.limiter)),
	)
	syncpb.RegisterSyncServiceServer(g, sync)
	hs := health.NewServer()
	healthpb.RegisterHealthServer(g, hs)
	hs.SetServingStatus("", healthpb.HealthCheckResponse_SERVING)
	hs.SetServingStatus(syncpb.SyncService_ServiceDesc.ServiceName, healthpb.HealthCheckResponse_SERVING)
	return g, hs
}

// newHTTPServer returns the single-port server: HTTP/1.1 plus HTTP/2 over
// cleartext with prior knowledge (h2c), which is what gRPC clients speak to
// an http:// endpoint. The standard library's unencrypted HTTP/2 support
// (Go 1.24+) is used rather than golang.org/x/net/http2/h2c because it
// keeps HTTP/2 connections under http.Server's control, so Shutdown
// drains them (GOAWAY) instead of losing track of hijacked connections.
func newHTTPServer(h http.Handler, log *slog.Logger) *http.Server {
	srv := &http.Server{
		Handler:           h,
		ReadHeaderTimeout: 10 * time.Second,
		IdleTimeout:       2 * time.Minute,
		MaxHeaderBytes:    1 << 20,
		ErrorLog:          slog.NewLogLogger(log.Handler(), slog.LevelWarn),
		// No ReadTimeout/WriteTimeout: object uploads, downloads and the
		// SSE stream are long-lived.
	}
	srv.Protocols = new(http.Protocols)
	srv.Protocols.SetHTTP1(true)
	srv.Protocols.SetUnencryptedHTTP2(true)
	srv.HTTP2 = &http.HTTP2Config{
		MaxConcurrentStreams:          250,
		MaxReceiveBufferPerConnection: 16 << 20,
		MaxReceiveBufferPerStream:     8 << 20,
	}
	return srv
}

// mux routes gRPC requests (HTTP/2 + application/grpc*) to the gRPC server
// and everything else to the HTTP API.
func mux(g *grpc.Server, api http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.ProtoMajor == 2 && strings.HasPrefix(r.Header.Get("Content-Type"), "application/grpc") {
			g.ServeHTTP(w, r)
			return
		}
		api.ServeHTTP(w, r)
	})
}

func newLogger(cfg config.Config) *slog.Logger {
	out := cfg.LogOutput
	if out == nil {
		out = os.Stderr
	}
	return slog.New(newHandler(out, cfg.LogFormat, cfg.LogLevel))
}

func newHandler(w io.Writer, format string, level slog.Level) slog.Handler {
	opts := &slog.HandlerOptions{Level: level}
	if format == "json" {
		return slog.NewJSONHandler(w, opts)
	}
	return slog.NewTextHandler(w, opts)
}

// recoverUnary converts handler panics into codes.Internal errors.
func recoverUnary(log *slog.Logger) grpc.UnaryServerInterceptor {
	return func(ctx context.Context, req any, info *grpc.UnaryServerInfo, h grpc.UnaryHandler) (resp any, err error) {
		defer func() {
			if v := recover(); v != nil {
				log.ErrorContext(ctx, "panic in grpc handler", slog.String("method", info.FullMethod),
					slog.Any("panic", v), slog.String("stack", string(debug.Stack())))
				err = status.Error(codes.Internal, "internal server error")
			}
		}()
		return h(ctx, req)
	}
}

// recoverStream is the streaming counterpart of recoverUnary.
func recoverStream(log *slog.Logger) grpc.StreamServerInterceptor {
	return func(srv any, ss grpc.ServerStream, info *grpc.StreamServerInfo, h grpc.StreamHandler) (err error) {
		defer func() {
			if v := recover(); v != nil {
				log.ErrorContext(ss.Context(), "panic in grpc stream handler", slog.String("method", info.FullMethod),
					slog.Any("panic", v), slog.String("stack", string(debug.Stack())))
				err = status.Error(codes.Internal, "internal server error")
			}
		}()
		return h(srv, ss)
	}
}
