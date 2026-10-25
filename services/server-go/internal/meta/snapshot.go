package meta

// storage-backends (docs/ENGINEERING_PLAN.md §15.2): consistent snapshots
// of the bbolt database for server backups.

import (
	"context"
	"io"

	bolt "go.etcd.io/bbolt"
)

// Snapshot runs fn inside one read transaction with the exact size of a
// consistent copy of the database and a WriterTo producing it (bbolt's
// tx.WriteTo). Writers are not blocked while fn runs. fn must write the
// copy before returning.
func (b *Bolt) Snapshot(ctx context.Context, fn func(size int64, db io.WriterTo) error) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	return b.db.View(func(tx *bolt.Tx) error {
		return fn(tx.Size(), tx)
	})
}
