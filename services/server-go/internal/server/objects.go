package server

// storage-backends (docs/ENGINEERING_PLAN.md §15.2): object store
// selection (--object-store disk|s3).

import (
	"context"
	"fmt"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/config"
	"github.com/seanfilimon/worktree/services/server-go/internal/objstore"
)

// s3ConnectTimeout bounds the startup bucket check.
const s3ConnectTimeout = 30 * time.Second

// OpenObjectStore returns the object store selected by kind: a DiskStore
// rooted at dataDir (objects/<2-hex>/<62-hex>) or an S3Store, which is
// checked (and its bucket created) before returning. Used by the server and
// by `worktree-server restore`.
func OpenObjectStore(ctx context.Context, kind, dataDir string, s3 config.S3Config, compress bool) (objstore.Store, error) {
	switch kind {
	case "", config.ObjectStoreDisk:
		return objstore.NewWithOptions(dataDir, objstore.Options{Compress: compress}), nil
	case config.ObjectStoreS3:
		cctx, cancel := context.WithTimeout(ctx, s3ConnectTimeout)
		defer cancel()
		s, err := objstore.NewS3(cctx, objstore.S3Options{
			Endpoint: s3.Endpoint, Bucket: s3.Bucket, Prefix: s3.Prefix, Region: s3.Region,
			AccessKey: s3.AccessKey, SecretKey: s3.SecretKey, Insecure: s3.Insecure, Compress: compress,
		})
		if err != nil {
			return nil, err
		}
		return s, nil
	default:
		return nil, fmt.Errorf("unknown object store %q", kind)
	}
}
