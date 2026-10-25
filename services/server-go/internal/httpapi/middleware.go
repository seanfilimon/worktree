package httpapi

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"errors"
	"log/slog"
	"net/http"
	"runtime/debug"
	"slices"
	"strconv"
	"strings"
	"sync"
	"time"
)

// RequestIDHeader carries the request id in requests and responses.
const RequestIDHeader = "X-Request-Id"

type ctxKey int

const (
	requestIDKey ctxKey = iota
	routeKey
)

// requestIDFrom returns the request id stored in ctx, or "".
func requestIDFrom(ctx context.Context) string {
	id, _ := ctx.Value(requestIDKey).(string)
	return id
}

// routeInfo is a per-request mutable slot the router fills with the matched
// pattern. It is shared by pointer so that it survives r.WithContext copies
// made by inner middleware.
type routeInfo struct {
	mu      sync.Mutex
	pattern string
}

func setRoute(ctx context.Context, pattern string) {
	if ri, ok := ctx.Value(routeKey).(*routeInfo); ok {
		ri.mu.Lock()
		ri.pattern = pattern
		ri.mu.Unlock()
	}
}

func (ri *routeInfo) get() string {
	ri.mu.Lock()
	defer ri.mu.Unlock()
	if ri.pattern == "" {
		return "unmatched"
	}
	return ri.pattern
}

// requestIDMiddleware propagates a well-formed incoming X-Request-Id or
// generates a new one, and echoes it in the response.
func requestIDMiddleware(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		id := r.Header.Get(RequestIDHeader)
		if !validRequestID(id) {
			id = newRequestID()
		}
		w.Header().Set(RequestIDHeader, id)
		next.ServeHTTP(w, r.WithContext(context.WithValue(r.Context(), requestIDKey, id)))
	})
}

func validRequestID(id string) bool {
	if id == "" || len(id) > 128 {
		return false
	}
	for i := 0; i < len(id); i++ {
		c := id[i]
		if !(c >= 'a' && c <= 'z' || c >= 'A' && c <= 'Z' || c >= '0' && c <= '9' || c == '-' || c == '_' || c == '.' || c == ':') {
			return false
		}
	}
	return true
}

func newRequestID() string {
	var b [12]byte
	_, _ = rand.Read(b[:])
	return hex.EncodeToString(b[:])
}

// statusRecorder captures the status code and body size of a response. It
// forwards Flush and exposes Unwrap so http.ResponseController keeps working
// through it (needed by the SSE handler).
type statusRecorder struct {
	http.ResponseWriter
	status      int
	bytes       int64
	wroteHeader bool
}

func (s *statusRecorder) WriteHeader(code int) {
	if !s.wroteHeader {
		s.status = code
		s.wroteHeader = true
	}
	s.ResponseWriter.WriteHeader(code)
}

func (s *statusRecorder) Write(p []byte) (int, error) {
	if !s.wroteHeader {
		s.status = http.StatusOK
		s.wroteHeader = true
	}
	n, err := s.ResponseWriter.Write(p)
	s.bytes += int64(n)
	return n, err
}

func (s *statusRecorder) Flush() {
	if !s.wroteHeader {
		s.status = http.StatusOK
		s.wroteHeader = true
	}
	_ = http.NewResponseController(s.ResponseWriter).Flush()
}

func (s *statusRecorder) Unwrap() http.ResponseWriter { return s.ResponseWriter }

// instrumentMiddleware logs every request and records metrics by route.
func (a *api) instrumentMiddleware(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		start := time.Now()
		ri := &routeInfo{}
		rec := &statusRecorder{ResponseWriter: w, status: http.StatusOK}
		a.metrics.inFlight.Add(1)
		defer func() {
			a.metrics.inFlight.Add(-1)
			elapsed := time.Since(start)
			route := ri.get()
			a.metrics.observe(route, rec.status, elapsed)

			level := slog.LevelInfo
			switch {
			case rec.status >= 500:
				level = slog.LevelError
			case route == "GET /healthz" || route == "GET /metrics":
				level = slog.LevelDebug
			}
			a.log.LogAttrs(r.Context(), level, "http request",
				slog.String("request_id", requestIDFrom(r.Context())),
				slog.String("method", r.Method),
				slog.String("path", r.URL.Path),
				slog.String("route", route),
				slog.Int("status", rec.status),
				slog.Int64("bytes", rec.bytes),
				slog.Duration("duration", elapsed),
				slog.String("remote", r.RemoteAddr),
			)
		}()
		next.ServeHTTP(rec, r.WithContext(context.WithValue(r.Context(), routeKey, ri)))
	})
}

// recoverMiddleware turns handler panics into 500 responses and logs the
// stack. http.ErrAbortHandler is re-panicked as net/http expects.
func (a *api) recoverMiddleware(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		defer func() {
			v := recover()
			if v == nil {
				return
			}
			if err, ok := v.(error); ok && errors.Is(err, http.ErrAbortHandler) {
				panic(v)
			}
			a.log.ErrorContext(r.Context(), "panic in http handler",
				slog.String("request_id", requestIDFrom(r.Context())),
				slog.String("method", r.Method),
				slog.String("path", r.URL.Path),
				slog.Any("panic", v),
				slog.String("stack", string(debug.Stack())))
			if rec, ok := w.(*statusRecorder); ok && rec.wroteHeader {
				// Too late for an error body; abort the connection so the
				// client sees a truncated response instead of a success.
				panic(http.ErrAbortHandler)
			}
			writeJSON(w, http.StatusInternalServerError,
				errorBody{Error: errorDetail{Code: codeInternal, Message: "internal server error"}})
		}()
		next.ServeHTTP(w, r)
	})
}

// corsMiddleware allows cross-origin calls from the given origins ("*" =
// any). The API authenticates with bearer tokens, not cookies, so
// credentials are never allowed.
func corsMiddleware(origins []string) func(http.Handler) http.Handler {
	allowAny := slices.Contains(origins, "*")
	allowed := make(map[string]bool, len(origins))
	for _, o := range origins {
		allowed[strings.TrimRight(strings.ToLower(strings.TrimSpace(o)), "/")] = true
	}
	return func(next http.Handler) http.Handler {
		return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			origin := r.Header.Get("Origin")
			if origin == "" {
				next.ServeHTTP(w, r)
				return
			}
			h := w.Header()
			h.Add("Vary", "Origin")
			if !allowAny && !allowed[strings.ToLower(origin)] {
				next.ServeHTTP(w, r)
				return
			}
			h.Set("Access-Control-Allow-Origin", origin)
			h.Set("Access-Control-Expose-Headers", RequestIDHeader+", WWW-Authenticate, Content-Disposition, Retry-After, "+headerRateLimit+", "+headerRateRemaining)
			if r.Method == http.MethodOptions && r.Header.Get("Access-Control-Request-Method") != "" {
				h.Add("Vary", "Access-Control-Request-Method")
				h.Add("Vary", "Access-Control-Request-Headers")
				h.Set("Access-Control-Allow-Methods", "GET, HEAD, POST, PUT, PATCH, DELETE, OPTIONS")
				h.Set("Access-Control-Allow-Headers", "Authorization, Content-Type, Last-Event-ID, "+RequestIDHeader)
				h.Set("Access-Control-Max-Age", strconv.Itoa(600))
				setRoute(r.Context(), "OPTIONS (preflight)")
				w.WriteHeader(http.StatusNoContent)
				return
			}
			next.ServeHTTP(w, r)
		})
	}
}
