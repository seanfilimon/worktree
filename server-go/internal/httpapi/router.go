package httpapi

import (
	"encoding/json"
	"net/http"
	"time"

	"github.com/ramizik/worktree/server-go/internal/auth"
)

type RouterConfig struct {
	Version       string
	Authenticator auth.StaticAuthenticator
	Staged        *StagedService
}

func NewRouter(cfg RouterConfig) http.Handler {
	mux := http.NewServeMux()
	mux.HandleFunc("GET /health", healthHandler(cfg))
	mux.HandleFunc("GET /ready", readyHandler(cfg))
	if cfg.Staged != nil {
		mux.Handle("POST /staged", authMiddleware(cfg.Authenticator, http.HandlerFunc(cfg.Staged.HandleUpload)))
	}
	return requestIDMiddleware(mux)
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
