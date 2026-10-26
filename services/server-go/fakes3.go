package main

// `worktree-server fake-s3` (docs/ENGINEERING_PLAN.md §16.11, testing
// only): serves the in-memory objstore.FakeS3 so end-to-end tests can run
// the S3 object store and S3 backup target without MinIO. Data lives in
// memory and is lost on exit; requests are not signature-checked.

import (
	"context"
	"errors"
	"flag"
	"fmt"
	"io"
	"net"
	"net/http"
	"os"
	"os/signal"
	"syscall"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/objstore"
)

// runFakeS3 implements the subcommand. Exit codes: 0 stopped by a signal,
// 1 listen failure, 2 invalid usage.
func runFakeS3(args []string, stdout, stderr io.Writer) int {
	fs := flag.NewFlagSet("worktree-server fake-s3", flag.ContinueOnError)
	fs.SetOutput(stderr)
	addr := fs.String("addr", "127.0.0.1:0", "`host:port` to listen on")
	bucket := fs.String("bucket", "", "create this `bucket` at startup")
	if err := fs.Parse(args); err != nil {
		if errors.Is(err, flag.ErrHelp) {
			return 0
		}
		return 2
	}
	if fs.NArg() > 0 {
		fmt.Fprintf(stderr, "worktree-server fake-s3: unexpected argument %q\n", fs.Arg(0))
		return 2
	}
	fake := objstore.NewFakeS3()
	if *bucket != "" {
		fake.CreateBucket(*bucket)
	}
	ln, err := net.Listen("tcp", *addr)
	if err != nil {
		fmt.Fprintf(stderr, "worktree-server fake-s3: %v\n", err)
		return 1
	}
	srv := &http.Server{Handler: fake, ReadHeaderTimeout: 10 * time.Second}
	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()
	go func() {
		<-ctx.Done()
		sctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		_ = srv.Shutdown(sctx)
	}()
	fmt.Fprintf(stdout, "fake-s3 (testing only, in memory) listening on http://%s\n", ln.Addr())
	if err := srv.Serve(ln); err != nil && !errors.Is(err, http.ErrServerClosed) {
		fmt.Fprintf(stderr, "worktree-server fake-s3: %v\n", err)
		return 1
	}
	return 0
}
