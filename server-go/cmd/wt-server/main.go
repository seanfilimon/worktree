package main

import (
	"context"
	"errors"
	"log/slog"
	"net"
	"net/http"
	"os"
	"os/signal"
	"syscall"

	"google.golang.org/grpc"

	"github.com/ramizik/worktree/server-go/internal/audit"
	"github.com/ramizik/worktree/server-go/internal/auth"
	"github.com/ramizik/worktree/server-go/internal/config"
	grpcserver "github.com/ramizik/worktree/server-go/internal/grpc"
	worktreepb "github.com/ramizik/worktree/server-go/internal/grpc/worktreepb/worktree/v1"
	"github.com/ramizik/worktree/server-go/internal/httpapi"
	"github.com/ramizik/worktree/server-go/internal/iam"
	"github.com/ramizik/worktree/server-go/internal/observability"
	"github.com/ramizik/worktree/server-go/internal/server"
	"github.com/ramizik/worktree/server-go/internal/staged"
	"github.com/ramizik/worktree/server-go/internal/storage"
)

func main() {
	cfg, err := config.Load()
	if err != nil {
		slog.Error("failed to load config", "error", err)
		os.Exit(1)
	}

	log := slog.New(slog.NewJSONHandler(os.Stdout, &slog.HandlerOptions{
		Level: cfg.LogLevel(),
	}))
	slog.SetDefault(log)

	objectStore := storage.NewLocalObjectStore(cfg.StorageRoot)

	var stagedStore staged.Store
	if cfg.DatabaseURL != "" {
		pgStore, err := staged.NewPostgresStore(context.Background(), cfg.DatabaseURL)
		if err != nil {
			slog.Error("failed to connect to postgres", "error", err)
			os.Exit(1)
		}
		defer pgStore.Close()
		stagedStore = pgStore
		slog.Info("using postgres staged store")
	} else {
		stagedStore = staged.NewFileStore(cfg.StorageRoot)
		slog.Info("using file staged store (dev mode)")
	}
	metrics := observability.NewMetrics()
	auditRecorder := audit.NewFileRecorder(cfg.AuditPath)

	router := httpapi.NewRouter(httpapi.RouterConfig{
		Version:       "dev",
		Authenticator: auth.NewStaticAuthenticator(cfg.AuthToken),
		Staged: httpapi.NewStagedService(objectStore, stagedStore, auditRecorder, iam.AllowAllAuthorizer{}, httpapi.StagedLimits{
			MaxObjectBytes: cfg.MaxStagedObjectBytes,
			MaxObjects:     cfg.MaxStagedObjects,
		}),
		Metrics: metrics,
	})

	srv := server.NewHTTPServer(cfg, router)

	syncSrv := grpcserver.NewSyncServer(objectStore, stagedStore, auditRecorder, iam.AllowAllAuthorizer{})
	grpcS := grpc.NewServer()
	worktreepb.RegisterSyncServiceServer(grpcS, syncSrv)

	grpcLis, err := net.Listen("tcp", cfg.GRPCAddr)
	if err != nil {
		slog.Error("failed to listen for gRPC", "addr", cfg.GRPCAddr, "error", err)
		os.Exit(1)
	}
	go func() {
		log.Info("starting gRPC server", "addr", cfg.GRPCAddr)
		if err := grpcS.Serve(grpcLis); err != nil {
			log.Error("gRPC server failed", "error", err)
		}
	}()

	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()

	errCh := make(chan error, 1)
	go func() {
		log.Info("starting wt-server", "addr", cfg.HTTPAddr, "tls", cfg.TLS.Enabled)
		if cfg.TLS.Enabled {
			errCh <- srv.ListenAndServeTLS(cfg.TLS.CertFile, cfg.TLS.KeyFile)
			return
		}
		errCh <- srv.ListenAndServe()
	}()

	select {
	case <-ctx.Done():
		grpcS.GracefulStop()
		shutdownCtx, cancel := context.WithTimeout(context.Background(), cfg.ShutdownTimeout)
		defer cancel()
		if err := srv.Shutdown(shutdownCtx); err != nil {
			log.Error("graceful shutdown failed", "error", err)
			os.Exit(1)
		}
		log.Info("server stopped")
	case err := <-errCh:
		if err != nil && !errors.Is(err, http.ErrServerClosed) {
			log.Error("server failed", "error", err)
			os.Exit(1)
		}
	}
}
