package server

// server-ops (docs/ENGINEERING_PLAN.md §16.11): backup target selection
// (--backup-target disk|s3, reusing the --s3-* configuration) and the
// background retention service.

import (
	"context"
	"fmt"
	"path/filepath"

	"github.com/seanfilimon/worktree/services/server-go/internal/backup"
	"github.com/seanfilimon/worktree/services/server-go/internal/config"
	"github.com/seanfilimon/worktree/services/server-go/internal/objstore"
)

// OpenBackupTarget returns the backup target selected by kind: a
// DiskTarget on <dataDir>/backups or an S3Target on the --s3-* bucket under
// the --backup-s3-prefix (default <s3-prefix>backups/). Used by the server
// and by `worktree-server restore`.
func OpenBackupTarget(ctx context.Context, kind, dataDir string, s3 config.S3Config, prefix string) (backup.Target, error) {
	switch kind {
	case "", config.BackupTargetDisk:
		return backup.NewDiskTarget(filepath.Join(dataDir, "backups")), nil
	case config.BackupTargetS3:
		cctx, cancel := context.WithTimeout(ctx, s3ConnectTimeout)
		defer cancel()
		b, err := objstore.NewS3Bucket(cctx, objstore.S3Options{
			Endpoint: s3.Endpoint, Bucket: s3.Bucket, Region: s3.Region,
			AccessKey: s3.AccessKey, SecretKey: s3.SecretKey, Insecure: s3.Insecure,
		})
		if err != nil {
			return nil, fmt.Errorf("backup target: %w", err)
		}
		return backup.NewS3Target(b, config.BackupS3KeyPrefix(s3, prefix)), nil
	default:
		return nil, fmt.Errorf("unknown backup target %q", kind)
	}
}
