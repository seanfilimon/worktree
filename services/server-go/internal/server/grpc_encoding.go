package server

// hardening (§15.10): gRPC message-compression negotiation made observable.
//
// grpc-go does not expose the reserved grpc-encoding headers to
// interceptors, so mux wraps the http.ResponseWriter of every gRPC request
// and counts the request's grpc-encoding and the grpc-encoding the server
// answers with into opsmetrics.GRPCEncoding (rendered by GET /metrics as
// worktree_grpc_encoding_total{encoding="request/gzip"|"response/identity"|...}).

import (
	"net/http"
	"strings"
	"sync"

	"github.com/seanfilimon/worktree/services/server-go/internal/opsmetrics"
)

// grpcEncodingLabel is the counter label for a grpc-encoding header value:
// "<dir>/<encoding>", identity when the header is absent and "other" for
// encodings the server does not register (bounding the label set).
func grpcEncodingLabel(dir, enc string) string {
	switch enc = strings.ToLower(strings.TrimSpace(enc)); enc {
	case "":
		enc = "identity"
	case "identity", "gzip":
	default:
		enc = "other"
	}
	return dir + "/" + enc
}

// countGRPCEncoding wraps a gRPC handler (grpc.Server.ServeHTTP) with the
// grpc-encoding counters.
func countGRPCEncoding(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		opsmetrics.GRPCEncoding.Inc(grpcEncodingLabel("request", r.Header.Get("Grpc-Encoding")))
		ew := &encodingWriter{ResponseWriter: w}
		if _, ok := w.(http.Flusher); ok {
			next.ServeHTTP(flushEncodingWriter{ew}, r)
			return
		}
		// Not a Flusher: grpc refuses the request, as it would unwrapped.
		next.ServeHTTP(ew, r)
	})
}

// encodingWriter counts the response grpc-encoding once, when the headers
// are committed (first WriteHeader, Write or Flush).
type encodingWriter struct {
	http.ResponseWriter
	once sync.Once
}

func (w *encodingWriter) observe() {
	w.once.Do(func() {
		opsmetrics.GRPCEncoding.Inc(grpcEncodingLabel("response", w.ResponseWriter.Header().Get("Grpc-Encoding")))
	})
}

func (w *encodingWriter) WriteHeader(code int) {
	w.observe()
	w.ResponseWriter.WriteHeader(code)
}

func (w *encodingWriter) Write(p []byte) (int, error) {
	w.observe()
	return w.ResponseWriter.Write(p)
}

// Unwrap lets http.ResponseController reach the underlying writer.
func (w *encodingWriter) Unwrap() http.ResponseWriter { return w.ResponseWriter }

// flushEncodingWriter is an encodingWriter over an http.Flusher; it keeps
// http.Flusher working, which grpc's ServeHTTP requires.
type flushEncodingWriter struct{ *encodingWriter }

func (w flushEncodingWriter) Flush() {
	w.observe()
	w.ResponseWriter.(http.Flusher).Flush()
}
