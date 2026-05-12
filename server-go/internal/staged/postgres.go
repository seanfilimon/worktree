package staged

import (
	"context"
	"fmt"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"
)

// PostgresStore implements Store using a Postgres connection pool.
// Add is idempotent: duplicate snapshot_id inserts are silently ignored.
type PostgresStore struct {
	pool *pgxpool.Pool
}

func NewPostgresStore(ctx context.Context, dsn string) (*PostgresStore, error) {
	pool, err := pgxpool.New(ctx, dsn)
	if err != nil {
		return nil, fmt.Errorf("pgxpool.New: %w", err)
	}
	if err := pool.Ping(ctx); err != nil {
		pool.Close()
		return nil, fmt.Errorf("postgres ping: %w", err)
	}
	return &PostgresStore{pool: pool}, nil
}

func (s *PostgresStore) Close() {
	s.pool.Close()
}

func (s *PostgresStore) Add(ctx context.Context, snap Snapshot) error {
	if snap.CreatedAt.IsZero() {
		snap.CreatedAt = time.Now().UTC()
	}

	tx, err := s.pool.Begin(ctx)
	if err != nil {
		return fmt.Errorf("begin tx: %w", err)
	}
	defer tx.Rollback(ctx) //nolint:errcheck

	_, err = tx.Exec(ctx, `
		INSERT INTO staged_snapshots (snapshot_id, tenant, worktree, tree_id, branch, created_at)
		VALUES ($1, $2, $3, $4, $5, $6)
		ON CONFLICT (snapshot_id) DO NOTHING
	`, snap.SnapshotID, snap.Tenant, snap.Worktree, snap.TreeID, snap.Branch, snap.CreatedAt)
	if err != nil {
		return fmt.Errorf("insert staged_snapshot: %w", err)
	}

	for _, objHash := range snap.ObjectIDs {
		_, err = tx.Exec(ctx, `
			INSERT INTO staged_snapshot_objects (snapshot_id, object_hash)
			VALUES ($1, $2)
			ON CONFLICT (snapshot_id, object_hash) DO NOTHING
		`, snap.SnapshotID, objHash)
		if err != nil {
			return fmt.Errorf("insert staged_snapshot_object %s: %w", objHash, err)
		}
	}

	return tx.Commit(ctx)
}

func (s *PostgresStore) List(ctx context.Context, filter ListFilter) ([]Snapshot, error) {
	rows, err := s.pool.Query(ctx, `
		SELECT s.snapshot_id, s.tenant, s.worktree, s.tree_id, s.branch, s.created_at,
		       coalesce(array_agg(o.object_hash ORDER BY o.id) FILTER (WHERE o.object_hash IS NOT NULL), '{}') AS object_ids
		FROM staged_snapshots s
		LEFT JOIN staged_snapshot_objects o ON o.snapshot_id = s.snapshot_id
		WHERE ($1::text = '' OR s.tenant   = $1)
		  AND ($2::text = '' OR s.worktree = $2)
		  AND ($3::text = '' OR s.branch   = $3)
		GROUP BY s.id
		ORDER BY s.id DESC
	`, filter.Tenant, filter.Worktree, filter.Branch)
	if err != nil {
		return nil, fmt.Errorf("list staged_snapshots: %w", err)
	}
	defer rows.Close()

	var snapshots []Snapshot
	for rows.Next() {
		var snap Snapshot
		var objectIDs []string
		if err := rows.Scan(
			&snap.SnapshotID, &snap.Tenant, &snap.Worktree, &snap.TreeID,
			&snap.Branch, &snap.CreatedAt, &objectIDs,
		); err != nil {
			return nil, fmt.Errorf("scan staged_snapshot row: %w", err)
		}
		snap.ObjectIDs = objectIDs
		snapshots = append(snapshots, snap)
	}
	return snapshots, rows.Err()
}

// Ensure PostgresStore satisfies Store at compile time.
var _ Store = (*PostgresStore)(nil)
