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
	"slices"
	"strings"
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

	// server-ops (§16.11): --list, and restoring a backup id from the
	// backup target (<data>/backups or the S3 bucket).
	if rc.List {
		return listTarget(ctx, rc, stdout, stderr)
	}
	source := rc.File
	var reader io.ReadCloser
	if _, err := os.Stat(rc.File); err != nil {
		if !backup.ValidID(rc.File) {
			fmt.Fprintf(stderr, "worktree-server restore: %v\n", err)
			return 1
		}
		target, err := server.OpenBackupTarget(ctx, rc.BackupTarget, rc.DataDir, rc.S3, rc.BackupS3Prefix)
		if err != nil {
			fmt.Fprintf(stderr, "worktree-server restore: %v\n", err)
			return 1
		}
		r, _, err := target.Open(ctx, rc.File)
		if err != nil {
			fmt.Fprintf(stderr, "worktree-server restore: backup %s in %s: %v\n", rc.File, target.Location(), err)
			return 1
		}
		defer r.Close()
		reader, source = r, target.Path(rc.File)
	}
	objects, err := server.OpenObjectStore(ctx, rc.ObjectStore, rc.DataDir, rc.S3, rc.CompressObjects)
	if err != nil {
		fmt.Fprintf(stderr, "worktree-server restore: open object store: %v\n", err)
		return 1
	}
	started := time.Now()
	fmt.Fprintf(stdout, "restoring %s into %s (%s object store)\n", source, rc.DataDir, backup.DescribeStore(objects).Kind)
	opts := backup.RestoreOptions{File: rc.File, DataDir: rc.DataDir, Objects: objects, Force: rc.Force}
	if reader != nil {
		opts.Source = reader
	}
	rep, err := backup.Restore(ctx, opts)
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

// listTarget prints the backups of the backup target, newest first
// (server-ops §16.11: `worktree-server restore --list`).
func listTarget(ctx context.Context, rc config.RestoreConfig, stdout, stderr io.Writer) int {
	target, err := server.OpenBackupTarget(ctx, rc.BackupTarget, rc.DataDir, rc.S3, rc.BackupS3Prefix)
	if err != nil {
		fmt.Fprintf(stderr, "worktree-server restore: %v\n", err)
		return 1
	}
	objs, err := target.List(ctx)
	if err != nil {
		fmt.Fprintf(stderr, "worktree-server restore: %v\n", err)
		return 1
	}
	slices.SortFunc(objs, func(a, b backup.TargetObject) int { return strings.Compare(b.ID, a.ID) })
	fmt.Fprintf(stdout, "backups in %s:\n", target.Location())
	if len(objs) == 0 {
		fmt.Fprintf(stdout, "  (none)\n")
		return 0
	}
	for _, o := range objs {
		created := o.Modified
		if t, ok := backup.IDTime(o.ID); ok {
			created = t
		}
		fmt.Fprintf(stdout, "  %s  %s  %d bytes\n", o.ID, created.UTC().Format(time.RFC3339), o.Size)
	}
	fmt.Fprintf(stdout, "restore one with: worktree-server restore --data DIR [same target flags] ID\n")
	return 0
}

func orDash(s string) string {
	if s == "" {
		return "-"
	}
	return s
}
