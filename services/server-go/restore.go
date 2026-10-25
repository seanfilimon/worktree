package main

// `worktree-server restore` (docs/ENGINEERING_PLAN.md §15.2): offline
// restore of a .wtbak backup into a stopped server's data directory and
// object store.

import (
	"context"
	"errors"
	"flag"
	"fmt"
	"io"
	"os"
	"os/signal"
	"syscall"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/backup"
	"github.com/seanfilimon/worktree/services/server-go/internal/config"
	"github.com/seanfilimon/worktree/services/server-go/internal/server"
)

// runRestore implements the subcommand. Exit codes: 0 restored, 1 the
// restore failed (corrupt archive, non-empty data directory, store
// errors), 2 invalid usage.
func runRestore(args []string, stdout, stderr io.Writer) int {
	rc, err := config.LoadRestore(args)
	switch {
	case errors.Is(err, flag.ErrHelp):
		config.RestoreUsage(stdout)
		return 0
	case err != nil:
		fmt.Fprintf(stderr, "worktree-server restore: %v\n\n", err)
		config.RestoreUsage(stderr)
		return 2
	}
	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()

	if _, err := os.Stat(rc.File); err != nil {
		fmt.Fprintf(stderr, "worktree-server restore: %v\n", err)
		return 1
	}
	objects, err := server.OpenObjectStore(ctx, rc.ObjectStore, rc.DataDir, rc.S3, rc.CompressObjects)
	if err != nil {
		fmt.Fprintf(stderr, "worktree-server restore: open object store: %v\n", err)
		return 1
	}
	started := time.Now()
	fmt.Fprintf(stdout, "restoring %s into %s (%s object store)\n", rc.File, rc.DataDir, backup.DescribeStore(objects).Kind)
	rep, err := backup.Restore(ctx, backup.RestoreOptions{
		File: rc.File, DataDir: rc.DataDir, Objects: objects, Force: rc.Force,
	})
	if err != nil {
		fmt.Fprintf(stderr, "worktree-server restore: %v\n", err)
		return 1
	}
	m := rep.Manifest
	fmt.Fprintf(stdout, "backup %s (created %s by %s", m.ID, m.CreatedAt.UTC().Format(time.RFC3339), orDash(m.CreatedBy))
	if m.ServerVersion != "" {
		fmt.Fprintf(stdout, ", server %s", m.ServerVersion)
	}
	fmt.Fprintf(stdout, ")\n")
	fmt.Fprintf(stdout, "restored %d objects (%d bytes), all hashes verified\n", rep.Objects, rep.ObjectBytes)
	fmt.Fprintf(stdout, "restored %s (%d bytes, sha256 verified)\n", rep.MetaDBPath, m.MetaDB.Size)
	fmt.Fprintf(stdout, "done in %s; start the server with --data %s\n", time.Since(started).Round(time.Millisecond), rc.DataDir)
	return 0
}

func orDash(s string) string {
	if s == "" {
		return "-"
	}
	return s
}
