package server

// hardening (§15.10): grpc-encoding counters of the gRPC handler wrapper.

import (
	"context"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"

	"google.golang.org/grpc"
	"google.golang.org/grpc/encoding/gzip"

	"github.com/seanfilimon/worktree/services/server-go/internal/opsmetrics"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
)

// encodingCounts snapshots the four well-known label values.
func encodingCounts() map[string]uint64 {
	out := map[string]uint64{}
	for _, l := range []string{"request/gzip", "request/identity", "response/gzip", "response/identity", "request/other"} {
		out[l] = opsmetrics.GRPCEncoding.Get(l)
	}
	return out
}

func encodingDelta(before map[string]uint64) map[string]uint64 {
	out := map[string]uint64{}
	for l, v := range encodingCounts() {
		if d := v - before[l]; d != 0 {
			out[l] = d
		}
	}
	return out
}

func sameCounts(a, b map[string]uint64) bool {
	if len(a) != len(b) {
		return false
	}
	for k, v := range a {
		if b[k] != v {
			return false
		}
	}
	return true
}

func TestGRPCEncodingWrapper(t *testing.T) {
	cases := []struct {
		name     string
		reqEnc   string
		handler  http.HandlerFunc
		want     map[string]uint64
		wantBody string
		flushes  bool
	}{
		{
			name:   "gzip both ways, headers committed by WriteHeader",
			reqEnc: "gzip",
			handler: func(w http.ResponseWriter, r *http.Request) {
				if _, ok := w.(http.Flusher); !ok {
					t.Error("wrapped writer is not an http.Flusher")
				}
				w.Header().Set("Grpc-Encoding", "gzip")
				w.WriteHeader(200)
				w.(http.Flusher).Flush()
				_, _ = w.Write([]byte("abc"))
				w.(http.Flusher).Flush()
			},
			want:     map[string]uint64{"request/gzip": 1, "response/gzip": 1},
			wantBody: "abc",
			flushes:  true,
		},
		{
			name:   "identity request, headers committed by the first Write",
			reqEnc: "",
			handler: func(w http.ResponseWriter, r *http.Request) {
				_, _ = w.Write([]byte("x"))
				w.Header().Set("Grpc-Encoding", "gzip") // too late: not sent
				_, _ = w.Write([]byte("y"))
			},
			want:     map[string]uint64{"request/identity": 1, "response/identity": 1},
			wantBody: "xy",
		},
		{
			name:   "headers committed by Flush (status-only response)",
			reqEnc: "gzip",
			handler: func(w http.ResponseWriter, r *http.Request) {
				w.Header().Set("Grpc-Encoding", "gzip")
				w.(http.Flusher).Flush()
				w.Header().Set("Grpc-Status", "0")
			},
			want:    map[string]uint64{"request/gzip": 1, "response/gzip": 1},
			flushes: true,
		},
		{
			name:    "unknown encodings are bucketed",
			reqEnc:  "snappy",
			handler: func(w http.ResponseWriter, r *http.Request) { w.WriteHeader(200) },
			want:    map[string]uint64{"request/other": 1, "response/identity": 1},
		},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			before := encodingCounts()
			req := httptest.NewRequest("POST", "/worktree.sync.v1.SyncService/Ping", nil)
			if c.reqEnc != "" {
				req.Header.Set("Grpc-Encoding", c.reqEnc)
			}
			rec := httptest.NewRecorder()
			countGRPCEncoding(c.handler).ServeHTTP(rec, req)
			if got := encodingDelta(before); !sameCounts(got, c.want) {
				t.Fatalf("counter delta = %v, want %v", got, c.want)
			}
			if rec.Body.String() != c.wantBody {
				t.Fatalf("body = %q", rec.Body.String())
			}
			if c.flushes && !rec.Flushed {
				t.Fatal("Flush did not reach the underlying writer")
			}
		})
	}
	// http.ResponseController reaches the underlying writer.
	rec := httptest.NewRecorder()
	countGRPCEncoding(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if err := http.NewResponseController(w).Flush(); err != nil {
			t.Errorf("ResponseController.Flush: %v", err)
		}
	})).ServeHTTP(rec, httptest.NewRequest("POST", "/", nil))
	if !rec.Flushed {
		t.Fatal("ResponseController flush lost")
	}
	// A writer that is not an http.Flusher does not become one (grpc then
	// refuses the request exactly as without the wrapper).
	countGRPCEncoding(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if _, ok := w.(http.Flusher); ok {
			t.Error("non-flushing writer wrapped as an http.Flusher")
		}
	})).ServeHTTP(&discardWriter{h: http.Header{}}, httptest.NewRequest("POST", "/", nil))
}

// The real gRPC server behind mux: gzip-compressed calls are counted as
// request/gzip + response/gzip, plain ones as identity both ways.
func TestGRPCEncodingCountedThroughMux(t *testing.T) {
	addr, _ := startUnit(t, &testSync{})

	before := encodingCounts()
	downloadBytes(t, addr, grpc.UseCompressor(gzip.Name))
	if got := encodingDelta(before); got["request/gzip"] != 1 || got["response/gzip"] != 1 || got["request/identity"] != 0 {
		t.Fatalf("gzip download counted %v", got)
	}

	before = encodingCounts()
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	conn := dialGRPC(t, addr)
	if _, err := syncpb.NewSyncServiceClient(conn).Ping(ctx, &syncpb.PingRequest{}); err != nil {
		t.Fatal(err)
	}
	if got := encodingDelta(before); got["request/identity"] != 1 || got["response/identity"] != 1 || got["request/gzip"] != 0 {
		t.Fatalf("plain Ping counted %v", got)
	}
}
