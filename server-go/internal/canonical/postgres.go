package canonical

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"
)

// PostgresStore implements Store backed by pgxpool.
//
// Push CAS is enforced by a single UPDATE ... WHERE tip = $expected statement
// inside a transaction that also inserts the snapshot rows. Snapshots and
// object refs are inserted before the CAS so an idempotent retry after a
// reconciled conflict reuses the same rows.
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

func (s *PostgresStore) GetBranch(ctx context.Context, key BranchKey) (Branch, error) {
	var branch Branch
	var tip *string
	err := s.pool.QueryRow(ctx, `
		SELECT tenant, worktree, tree_id, name, tip_snapshot_id, updated_at
		FROM canonical_branches
		WHERE tenant = $1 AND worktree = $2 AND tree_id = $3 AND name = $4
	`, key.Tenant, key.Worktree, key.TreeID, key.Name).Scan(
		&branch.Tenant, &branch.Worktree, &branch.TreeID, &branch.Name,
		&tip, &branch.UpdatedAt,
	)
	if err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			return Branch{}, ErrBranchNotFound
		}
		return Branch{}, fmt.Errorf("select canonical_branch: %w", err)
	}
	if tip != nil {
		branch.TipSnapshotID = *tip
	}
	return branch, nil
}

func (s *PostgresStore) ListBranches(ctx context.Context, tenant, worktree, treeID string) ([]Branch, error) {
	rows, err := s.pool.Query(ctx, `
		SELECT tenant, worktree, tree_id, name, tip_snapshot_id, updated_at
		FROM canonical_branches
		WHERE tenant = $1
		  AND ($2::text = '' OR worktree = $2)
		  AND ($3::text = '' OR tree_id  = $3)
		ORDER BY tenant, worktree, tree_id, name
	`, tenant, worktree, treeID)
	if err != nil {
		return nil, fmt.Errorf("list canonical_branches: %w", err)
	}
	defer rows.Close()

	var branches []Branch
	for rows.Next() {
		var b Branch
		var tip *string
		if err := rows.Scan(&b.Tenant, &b.Worktree, &b.TreeID, &b.Name, &tip, &b.UpdatedAt); err != nil {
			return nil, fmt.Errorf("scan canonical_branch row: %w", err)
		}
		if tip != nil {
			b.TipSnapshotID = *tip
		}
		branches = append(branches, b)
	}
	return branches, rows.Err()
}

func (s *PostgresStore) InsertSnapshots(ctx context.Context, snapshots []Snapshot) error {
	if len(snapshots) == 0 {
		return nil
	}
	tx, err := s.pool.Begin(ctx)
	if err != nil {
		return fmt.Errorf("begin tx: %w", err)
	}
	defer tx.Rollback(ctx) //nolint:errcheck

	for _, snap := range snapshots {
		if err := insertSnapshotTx(ctx, tx, snap); err != nil {
			return err
		}
	}
	return tx.Commit(ctx)
}

func insertSnapshotTx(ctx context.Context, tx pgx.Tx, snap Snapshot) error {
	parents := snap.Parents
	if parents == nil {
		parents = []string{}
	}
	parentsJSON, err := json.Marshal(parents)
	if err != nil {
		return fmt.Errorf("marshal parents: %w", err)
	}
	payload := snap.Payload
	if len(payload) == 0 {
		// Fall back to a minimal payload so the column is never NULL.
		payload = []byte("{}")
	}
	_, err = tx.Exec(ctx, `
		INSERT INTO canonical_snapshots
			(snapshot_id, tenant, worktree, tree_id, branch, parents, manifest_hash, message, author, committed_at, payload)
		VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
		ON CONFLICT (snapshot_id) DO NOTHING
	`,
		snap.SnapshotID, snap.Tenant, snap.Worktree, snap.TreeID, snap.Branch,
		parentsJSON, snap.ManifestHash, snap.Message, snap.Author, snap.CommittedAt, payload,
	)
	if err != nil {
		return fmt.Errorf("insert canonical_snapshot %s: %w", snap.SnapshotID, err)
	}
	for _, obj := range snap.Objects {
		_, err = tx.Exec(ctx, `
			INSERT INTO canonical_snapshot_objects (snapshot_id, object_hash, path, size)
			VALUES ($1, $2, $3, $4)
			ON CONFLICT (snapshot_id, object_hash, path) DO NOTHING
		`, snap.SnapshotID, obj.Hash, obj.Path, obj.Size)
		if err != nil {
			return fmt.Errorf("insert canonical_snapshot_object %s/%s: %w", snap.SnapshotID, obj.Hash, err)
		}
	}
	return nil
}

func (s *PostgresStore) GetSnapshot(ctx context.Context, snapshotID string) (Snapshot, error) {
	var snap Snapshot
	var parentsJSON []byte
	err := s.pool.QueryRow(ctx, `
		SELECT snapshot_id, tenant, worktree, tree_id, branch, parents,
		       manifest_hash, message, author, committed_at, payload
		FROM canonical_snapshots
		WHERE snapshot_id = $1
	`, snapshotID).Scan(
		&snap.SnapshotID, &snap.Tenant, &snap.Worktree, &snap.TreeID, &snap.Branch,
		&parentsJSON, &snap.ManifestHash, &snap.Message, &snap.Author, &snap.CommittedAt, &snap.Payload,
	)
	if err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			return Snapshot{}, fmt.Errorf("canonical snapshot %s not found", snapshotID)
		}
		return Snapshot{}, fmt.Errorf("select canonical_snapshot: %w", err)
	}
	if err := json.Unmarshal(parentsJSON, &snap.Parents); err != nil {
		return Snapshot{}, fmt.Errorf("unmarshal parents for %s: %w", snapshotID, err)
	}
	objects, err := s.listObjects(ctx, snap.SnapshotID)
	if err != nil {
		return Snapshot{}, err
	}
	snap.Objects = objects
	return snap, nil
}

func (s *PostgresStore) WalkSnapshots(ctx context.Context, key BranchKey, fromTip, untilTip string) ([]Snapshot, []string, error) {
	if fromTip == "" {
		return nil, nil, nil
	}
	visited := map[string]bool{}
	frontier := []string{fromTip}
	var ordered []Snapshot

	for len(frontier) > 0 {
		next := frontier[0]
		frontier = frontier[1:]
		if next == "" || next == untilTip || visited[next] {
			continue
		}
		visited[next] = true
		snap, err := s.GetSnapshot(ctx, next)
		if err != nil {
			return nil, nil, err
		}
		ordered = append(ordered, snap)
		for _, p := range snap.Parents {
			if p == "" || p == untilTip || visited[p] {
				continue
			}
			frontier = append(frontier, p)
		}
	}

	// Reverse so oldest is first — client must apply parents before children.
	for i, j := 0, len(ordered)-1; i < j; i, j = i+1, j-1 {
		ordered[i], ordered[j] = ordered[j], ordered[i]
	}

	objectSet := map[string]struct{}{}
	for _, snap := range ordered {
		for _, obj := range snap.Objects {
			objectSet[obj.Hash] = struct{}{}
		}
	}
	objects := make([]string, 0, len(objectSet))
	for h := range objectSet {
		objects = append(objects, h)
	}
	return ordered, objects, nil
}

func (s *PostgresStore) AdvanceTipCAS(ctx context.Context, key BranchKey, expectedTip, newTip string) error {
	tx, err := s.pool.Begin(ctx)
	if err != nil {
		return fmt.Errorf("begin tx: %w", err)
	}
	defer tx.Rollback(ctx) //nolint:errcheck

	// Upsert the branch row if it does not exist yet.
	_, err = tx.Exec(ctx, `
		INSERT INTO canonical_branches (tenant, worktree, tree_id, name, tip_snapshot_id, updated_at)
		VALUES ($1, $2, $3, $4, NULL, NOW())
		ON CONFLICT (tenant, worktree, tree_id, name) DO NOTHING
	`, key.Tenant, key.Worktree, key.TreeID, key.Name)
	if err != nil {
		return fmt.Errorf("upsert canonical_branch: %w", err)
	}

	var expectedArg interface{}
	if expectedTip == "" {
		expectedArg = nil
	} else {
		expectedArg = expectedTip
	}

	cmd, err := tx.Exec(ctx, `
		UPDATE canonical_branches
		SET tip_snapshot_id = $5, updated_at = NOW()
		WHERE tenant = $1 AND worktree = $2 AND tree_id = $3 AND name = $4
		  AND tip_snapshot_id IS NOT DISTINCT FROM $6
	`, key.Tenant, key.Worktree, key.TreeID, key.Name, newTip, expectedArg)
	if err != nil {
		return fmt.Errorf("cas canonical_branch tip: %w", err)
	}
	if cmd.RowsAffected() == 0 {
		return ErrConflict
	}
	return tx.Commit(ctx)
}

func (s *PostgresStore) listObjects(ctx context.Context, snapshotID string) ([]ObjectRef, error) {
	rows, err := s.pool.Query(ctx, `
		SELECT object_hash, path, size
		FROM canonical_snapshot_objects
		WHERE snapshot_id = $1
		ORDER BY path, object_hash
	`, snapshotID)
	if err != nil {
		return nil, fmt.Errorf("list canonical_snapshot_objects: %w", err)
	}
	defer rows.Close()

	objects := []ObjectRef{}
	for rows.Next() {
		var obj ObjectRef
		if err := rows.Scan(&obj.Hash, &obj.Path, &obj.Size); err != nil {
			return nil, fmt.Errorf("scan canonical_snapshot_object row: %w", err)
		}
		objects = append(objects, obj)
	}
	return objects, rows.Err()
}

var _ Store = (*PostgresStore)(nil)
