// Command worktree-server is the W0rkTree remote server: gRPC sync protocol,
// REST + SSE API and the admin panel on a single port.
//
// See README.md for configuration. Exit codes: 0 success, 1 runtime error,
// 2 invalid usage or configuration.
package main

import (
	"context"
	"errors"
	"flag"
	"fmt"
	"io"
	"os"
	"os/signal"
	"syscall"

	"github.com/seanfilimon/worktree/services/server-go/internal/config"
	"github.com/seanfilimon/worktree/services/server-go/internal/server"
)

// version is overridden at build time:
//
//	go build -ldflags "-X main.version=1.2.3"
var version = "0.8.0"

func main() {
	os.Exit(run(os.Args[1:], os.Stdout, os.Stderr))
}

func run(args []string, stdout, stderr io.Writer) int {
	// storage-backends (§15.2): offline restore subcommand.
	if len(args) > 0 && args[0] == "restore" {
		return runRestore(args[1:], stdout, stderr)
	}
	// server-ops (§16.11): in-memory S3 API for end-to-end tests.
	if len(args) > 0 && args[0] == "fake-s3" {
		return runFakeS3(args[1:], stdout, stderr)
	}
	cfg, err := config.Load(args)
	switch {
	case errors.Is(err, flag.ErrHelp):
		config.Usage(stdout)
		return 0
	case err != nil:
		fmt.Fprintf(stderr, "worktree-server: %v\n\n", err)
		config.Usage(stderr)
		return 2
	}
	if cfg.ShowVersion {
		fmt.Fprintf(stdout, "worktree-server %s\n", version)
		return 0
	}
	cfg.Version = version
	cfg.Stdout = stdout
	cfg.LogOutput = stderr

	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()
	if err := server.Run(ctx, cfg); err != nil {
		fmt.Fprintf(stderr, "worktree-server: %v\n", err)
		return 1
	}
	return 0
}
