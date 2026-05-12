package httpapi

import (
	"encoding/json"
	"net/http"
	"time"

	"github.com/ramizik/worktree/server-go/internal/auth"
	"github.com/ramizik/worktree/server-go/internal/iam"
	"github.com/ramizik/worktree/server-go/internal/observability"
)

type RouterConfig struct {
	Version       string
	Authenticator auth.StaticAuthenticator
	Authorizer    iam.Authorizer
	Staged        *StagedService
	Metrics       *observability.Metrics
}

func NewRouter(cfg RouterConfig) http.Handler {
	if cfg.Metrics == nil {
		cfg.Metrics = observability.NewMetrics()
	}
	mux := http.NewServeMux()
	mux.HandleFunc("GET /health", healthHandler(cfg))
	mux.HandleFunc("GET /ready", readyHandler(cfg))
	mux.Handle("GET /metrics", cfg.Metrics)
	if cfg.Staged != nil {
		mux.Handle("POST /staged", authMiddleware(cfg.Authenticator, http.HandlerFunc(cfg.Staged.HandleUpload)))
		mux.Handle("GET /staged", authMiddleware(cfg.Authenticator, http.HandlerFunc(cfg.Staged.HandleList)))
	}
	return requestIDMiddleware(metricsMiddleware(cfg.Metrics, mux))
}

func healthHandler(cfg RouterConfig) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		writeJSON(w, http.StatusOK, map[string]any{
			"status":  "healthy",
			"version": cfg.Version,
			"time":    time.Now().UTC().Format(time.RFC3339),
		})
	}
}

func readyHandler(cfg RouterConfig) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		writeJSON(w, http.StatusOK, map[string]any{
			"status":  "ready",
			"version": cfg.Version,
		})
	}
}

func writeJSON(w http.ResponseWriter, status int, payload any) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(payload)
}
