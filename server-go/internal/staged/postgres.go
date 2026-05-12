package staged

import (
	"context"
	"errors"
	"fmt"

	"github.com/jackc/pgx/v5"
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

func NewPostgresStoreWithPool(pool *pgxpool.Pool) *PostgresStore {
	return &PostgresStore{pool: pool}
}

func (s *PostgresStore) Close() {
	s.pool.Close()
}

func (s *PostgresStore) Add(ctx context.Context, snap Snapshot) (AddResult, error) {
	snap = NormalizeSnapshot(snap)

	tx, err := s.pool.Begin(ctx)
	if err != nil {
		return AddResult{}, fmt.Errorf("begin tx: %w", err)
	}
	defer tx.Rollback(ctx) //nolint:errcheck

	var insertedID int64
	err = tx.QueryRow(ctx, `
		INSERT INTO staged_snapshots (snapshot_id, tenant, worktree, tree_id, branch, payload_hash, created_at)
		VALUES ($1, $2, $3, $4, $5, $6, $7)
		ON CONFLICT (tenant, worktree, tree_id, branch, snapshot_id) DO NOTHING
		RETURNING id
	`, snap.SnapshotID, snap.Tenant, snap.Worktree, snap.TreeID, snap.Branch, snap.PayloadHash, snap.CreatedAt).Scan(&insertedID)
	if err != nil && !errors.Is(err, pgx.ErrNoRows) {
		return AddResult{}, fmt.Errorf("insert staged_snapshot: %w", err)
	}

	if insertedID == 0 {
		var existing Snapshot
		var existingID int64
		err = tx.QueryRow(ctx, `
			SELECT id, snapshot_id, tenant, worktree, tree_id, branch, payload_hash, created_at
			FROM staged_snapshots
			WHERE tenant = $1 AND worktree = $2 AND tree_id = $3 AND branch = $4 AND snapshot_id = $5
		`, snap.Tenant, snap.Worktree, snap.TreeID, snap.Branch, snap.SnapshotID).Scan(
			&existingID, &existing.SnapshotID, &existing.Tenant, &existing.Worktree,
			&existing.TreeID, &existing.Branch, &existing.PayloadHash, &existing.CreatedAt,
		)
		if err != nil {
			return AddResult{}, fmt.Errorf("select existing staged_snapshot: %w", err)
		}
		if existing.PayloadHash != snap.PayloadHash {
			return AddResult{}, ErrConflict
		}
		existingObjects, err := s.listObjects(ctx, tx, existingID)
		if err != nil {
			return AddResult{}, err
		}
		existing.Objects = existingObjects
		existing = NormalizeSnapshot(existing)
		if err := tx.Commit(ctx); err != nil {
			return AddResult{}, err
		}
		return AddResult{Status: AddStatusAlreadyExists, Snapshot: existing}, nil
	}

	for _, obj := range snap.Objects {
		_, err = tx.Exec(ctx, `
			INSERT INTO staged_snapshot_objects (staged_snapshot_id, snapshot_id, object_hash, path, size)
			VALUES ($1, $2, $3, $4, $5)
			ON CONFLICT (staged_snapshot_id, path, object_hash) DO NOTHING
		`, insertedID, snap.SnapshotID, obj.Hash, obj.Path, obj.Size)
		if err != nil {
			return AddResult{}, fmt.Errorf("insert staged_snapshot_object %s: %w", obj.Hash, err)
		}
	}

	if err := tx.Commit(ctx); err != nil {
		return AddResult{}, err
	}
	return AddResult{Status: AddStatusCreated, Snapshot: snap}, nil
}

func (s *PostgresStore) List(ctx context.Context, filter ListFilter) ([]Snapshot, error) {
	rows, err := s.pool.Query(ctx, `
		SELECT s.id, s.snapshot_id, s.tenant, s.worktree, s.tree_id, s.branch, s.payload_hash, s.created_at
		FROM staged_snapshots s
		WHERE ($1::text = '' OR s.tenant   = $1)
		  AND ($2::text = '' OR s.worktree = $2)
		  AND ($3::text = '' OR s.branch   = $3)
		ORDER BY s.id DESC
	`, filter.Tenant, filter.Worktree, filter.Branch)
	if err != nil {
		return nil, fmt.Errorf("list staged_snapshots: %w", err)
	}
	defer rows.Close()

	var snapshots []Snapshot
	for rows.Next() {
		var snap Snapshot
		var snapshotID int64
		if err := rows.Scan(
			&snapshotID, &snap.SnapshotID, &snap.Tenant, &snap.Worktree, &snap.TreeID,
			&snap.Branch, &snap.PayloadHash, &snap.CreatedAt,
		); err != nil {
			return nil, fmt.Errorf("scan staged_snapshot row: %w", err)
		}
		objects, err := s.listObjects(ctx, s.pool, snapshotID)
		if err != nil {
			return nil, err
		}
		snap.Objects = objects
		snapshots = append(snapshots, NormalizeSnapshot(snap))
	}
	return snapshots, rows.Err()
}

type objectQuerier interface {
	Query(ctx context.Context, sql string, args ...any) (pgx.Rows, error)
}

func (s *PostgresStore) listObjects(ctx context.Context, q objectQuerier, stagedSnapshotID int64) ([]ObjectRef, error) {
	rows, err := q.Query(ctx, `
		SELECT path, object_hash, size
		FROM staged_snapshot_objects
		WHERE staged_snapshot_id = $1
		ORDER BY path, object_hash
	`, stagedSnapshotID)
	if err != nil {
		return nil, fmt.Errorf("list staged_snapshot_objects: %w", err)
	}
	defer rows.Close()

	objects := []ObjectRef{}
	for rows.Next() {
		var obj ObjectRef
		if err := rows.Scan(&obj.Path, &obj.Hash, &obj.Size); err != nil {
			return nil, fmt.Errorf("scan staged_snapshot_object row: %w", err)
		}
		objects = append(objects, obj)
	}
	return objects, rows.Err()
}

// Ensure PostgresStore satisfies Store at compile time.
var _ Store = (*PostgresStore)(nil)
