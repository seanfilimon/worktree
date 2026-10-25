package server

// gRPC message compression (docs/ENGINEERING_PLAN.md §14.6): the server
// has gzip registered, accepts gzip-compressed requests and compresses its
// responses to such clients.

import (
	"context"
	"errors"
	"io"
	"net"
	"sync/atomic"
	"testing"
	"time"

	"google.golang.org/grpc"
	"google.golang.org/grpc/credentials/insecure"
	"google.golang.org/grpc/encoding/gzip"

	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
)

// countingConn counts the bytes read from the server.
type countingConn struct {
	net.Conn
	read *atomic.Int64
}

func (c countingConn) Read(p []byte) (int, error) {
	n, err := c.Conn.Read(p)
	c.read.Add(int64(n))
	return n, err
}

// downloadBytes runs DownloadObjects (3 x 10 MiB of repeated bytes) and
// returns the bytes received on the wire.
func downloadBytes(t *testing.T, addr string, opts ...grpc.CallOption) int64 {
	t.Helper()
	var read atomic.Int64
	conn, err := grpc.NewClient("passthrough:///"+addr,
		grpc.WithTransportCredentials(insecure.NewCredentials()),
		grpc.WithContextDialer(func(ctx context.Context, a string) (net.Conn, error) {
			c, err := (&net.Dialer{}).DialContext(ctx, "tcp", a)
			if err != nil {
				return nil, err
			}
			return countingConn{Conn: c, read: &read}, nil
		}),
		grpc.WithDefaultCallOptions(grpc.MaxCallRecvMsgSize(64<<20)))
	if err != nil {
		t.Fatal(err)
	}
	defer conn.Close()
	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Second)
	defer cancel()
	stream, err := syncpb.NewSyncServiceClient(conn).DownloadObjects(ctx, &syncpb.WantMessage{}, opts...)
	if err != nil {
		t.Fatal(err)
	}
	n := 0
	for {
		msg, err := stream.Recv()
		if errors.Is(err, io.EOF) {
			break
		}
		if err != nil {
			t.Fatalf("Recv: %v", err)
		}
		if len(msg.GetData()) != 10<<20 {
			t.Fatalf("message %d has %d bytes", n, len(msg.GetData()))
		}
		n++
	}
	if n != 3 {
		t.Fatalf("got %d messages", n)
	}
	return read.Load()
}

func TestGRPCGzipNegotiation(t *testing.T) {
	addr, _ := startUnit(t, &testSync{})

	// A gzip-compressed request is accepted (Unimplemented otherwise) and
	// the responses come back gzip-compressed: far fewer wire bytes.
	compressed := downloadBytes(t, addr, grpc.UseCompressor(gzip.Name))
	plain := downloadBytes(t, addr)
	if plain < 30<<20 {
		t.Fatalf("uncompressed download read only %d bytes", plain)
	}
	if compressed*10 > plain {
		t.Fatalf("gzip download read %d bytes, uncompressed %d", compressed, plain)
	}

	// Unary RPCs negotiate the same way.
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	conn := dialGRPC(t, addr)
	pong, err := syncpb.NewSyncServiceClient(conn).Ping(ctx, &syncpb.PingRequest{}, grpc.UseCompressor(gzip.Name))
	if err != nil || pong.GetServerVersion() != "unit" {
		t.Fatalf("gzip Ping = %v, %v", pong, err)
	}
}
