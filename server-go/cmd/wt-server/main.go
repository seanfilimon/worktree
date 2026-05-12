package main

import (
	"context"
	"errors"
	"log/slog"
	"net/http"
	"os"
	"os/signal"
	"syscall"

	"github.com/ramizik/worktree/server-go/internal/auth"
	"github.com/ramizik/worktree/server-go/internal/config"
	"github.com/ramizik/worktree/server-go/internal/httpapi"
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
	stagedStore := staged.NewFileStore(cfg.StorageRoot)

	router := httpapi.NewRouter(httpapi.RouterConfig{
		Version:       "dev",
		Authenticator: auth.NewStaticAuthenticator(cfg.AuthToken),
		Staged:        httpapi.NewStagedService(objectStore, stagedStore),
	})

	srv := server.NewHTTPServer(cfg, router)

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
