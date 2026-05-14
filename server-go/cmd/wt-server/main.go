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
	"github.com/ramizik/worktree/server-go/internal/canonical"
	"github.com/ramizik/worktree/server-go/internal/config"
	grpcserver "github.com/ramizik/worktree/server-go/internal/grpc"
	worktreepb "github.com/ramizik/worktree/server-go/internal/grpc/worktreepb/worktree/v1"
	"github.com/ramizik/worktree/server-go/internal/httpapi"
	"github.com/ramizik/worktree/server-go/internal/iam"
	"github.com/ramizik/worktree/server-go/internal/migrate"
	"github.com/ramizik/worktree/server-go/internal/observability"
	"github.com/ramizik/worktree/server-go/internal/server"
	"github.com/ramizik/worktree/server-go/internal/staged"
	"github.com/ramizik/worktree/server-go/internal/storage"

	"github.com/jackc/pgx/v5/pgxpool"
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

	if os.Getenv("WT_SERVER_FORCE_FILE_STORE") == "true" {
		cfg.DatabaseURL = ""
		slog.Info("forcing file store locally (database URL ignored)")
	}

	if cfg.RunMigrations && cfg.DatabaseURL != "" {
		if err := migrate.Run(context.Background(), cfg.DatabaseURL); err != nil {
			slog.Error("failed to run migrations", "error", err)
			os.Exit(1)
		}
	}

	objectStore := storage.NewLocalObjectStore(cfg.StorageRoot)

	var stagedStore staged.Store
	var canonicalStore canonical.Store

	if cfg.DatabaseURL != "" {
		pool, err := pgxpool.New(context.Background(), cfg.DatabaseURL)
		if err != nil {
			slog.Error("failed to connect to postgres pool", "error", err)
			os.Exit(1)
		}
		defer pool.Close()

		stagedStore = staged.NewPostgresStoreWithPool(pool)
		canonicalStore = canonical.NewPostgresStoreWithPool(pool)
		slog.Info("using postgres storage")
	} else {
		stagedStore = staged.NewFileStore(cfg.StorageRoot)
		slog.Info("using file staged store (dev mode)")
	}

	metrics := observability.NewMetrics()
	auditRecorder := audit.NewFileRecorder(cfg.AuditPath)
	authenticator, err := buildAuthenticator(cfg)
	if err != nil {
		slog.Error("failed to configure authentication", "error", err)
		os.Exit(1)
	}
	jwtSecret := os.Getenv("WT_SERVER_JWT_SECRET")
	if jwtSecret == "" {
		jwtSecret = "default-insecure-dev-secret"
	}
	jwtAuth := auth.NewJWTAuthenticator(jwtSecret)

	authorizer, err := buildAuthorizer(cfg)
	if err != nil {
		slog.Error("failed to configure IAM", "error", err)
		os.Exit(1)
	}

	var canService *httpapi.CanonicalService
	if canonicalStore != nil {
		var updater canonical.PolicyUpdater
		if polAuth, ok := authorizer.(*iam.PolicyAuthorizer); ok {
			updater = polAuth
		}
		coreService := canonical.NewService(canonicalStore, objectStore, updater)
		canService = httpapi.NewCanonicalService(coreService, auditRecorder, authorizer)
	}

	router := httpapi.NewRouter(httpapi.RouterConfig{
		Version:       "dev",
		Authenticator: jwtAuth,
		Authorizer:    authorizer,
		Staged: httpapi.NewStagedService(objectStore, stagedStore, auditRecorder, authorizer, httpapi.StagedLimits{
			MaxObjectBytes: cfg.MaxStagedObjectBytes,
			MaxObjects:     cfg.MaxStagedObjects,
		}),
		Canonical:    canService,
		Metrics:      metrics,
		LoginHandler: httpapi.HandleLogin(authenticator, jwtAuth),
	})

	srv := server.NewHTTPServer(cfg, router)

	syncSrv := grpcserver.NewSyncServer(objectStore, stagedStore, auditRecorder, authorizer)
	grpcS := grpc.NewServer(grpc.UnaryInterceptor(grpcserver.AuthUnaryInterceptor(jwtAuth)))
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

func buildAuthenticator(cfg config.Config) (auth.Authenticator, error) {
	if cfg.AuthMode == "static-dev" {
		return auth.NewStaticAuthenticator(cfg.AuthToken), nil
	}
	credentials := []auth.TokenCredential{}
	if cfg.AuthCredentialsPath != "" {
		loaded, err := auth.LoadTokenCredentialsFile(cfg.AuthCredentialsPath)
		if err != nil {
			return nil, err
		}
		credentials = append(credentials, loaded...)
	}
	if cfg.AuthToken != "" {
		credentials = append(credentials, auth.TokenCredential{
			TokenID: "env-token",
			Secret:  cfg.AuthToken,
			Tenant:  cfg.AuthTenant,
			Account: cfg.AuthAccount,
			Scopes:  cfg.AuthScopes,
		})
	}
	return auth.NewBearerTokenAuthenticator(credentials), nil
}

func buildAuthorizer(cfg config.Config) (iam.Authorizer, error) {
	if cfg.IAMMode == "allow-all-dev" {
		return iam.AllowAllAuthorizer{}, nil
	}
	if cfg.IAMPolicyPath != "" {
		data, err := os.ReadFile(cfg.IAMPolicyPath)
		if err != nil {
			return nil, err
		}
		rules, err := iam.ParsePolicies(data)
		if err != nil {
			return nil, err
		}
		// Put them all under the default/global tenant for now
		return iam.NewPolicyAuthorizer(map[string][]iam.PolicyRule{"": rules}), nil
	}
	return iam.NewDefaultPolicyAuthorizer(), nil
}
