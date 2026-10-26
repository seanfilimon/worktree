package meta

// config-as-code (docs/ENGINEERING_PLAN.md §16.3): atomic updates of branch
// metadata (Branch.Description, Branch.Owner). PutBranch writes the whole
// record and could revert a tip moved by a concurrent push between a read
// and the write; UpdateBranchMeta reads, changes and writes in one
// transaction and never touches the tip. It is not part of
// domain.MetaStore; callers type-assert BranchMetaUpdater.

import (
	"context"

	bolt "go.etcd.io/bbolt"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// BranchMetaUpdater is implemented by the Bolt and Memory stores.
type BranchMetaUpdater interface {
	// UpdateBranchMeta sets the description and owner of an existing branch
	// (ErrNotFound when it does not exist) and returns the stored record.
	UpdateBranchMeta(ctx context.Context, treeID, name, description, owner string) (*domain.Branch, error)
}

var (
	_ BranchMetaUpdater = (*Bolt)(nil)
	_ BranchMetaUpdater = (*Memory)(nil)
)

// UpdateBranchMeta implements BranchMetaUpdater.
func (b *Bolt) UpdateBranchMeta(ctx context.Context, treeID, name, description, owner string) (*domain.Branch, error) {
	var out domain.Branch
	err := b.update(ctx, func(tx *bolt.Tx) error {
		if err := checkKeys("tree id", treeID, "branch name", name); err != nil {
			return err
		}
		cur, exists, err := getBranchTx(tx, treeID, name)
		if err != nil {
			return err
		}
		if !exists {
			return notFound("branch", treeID, name)
		}
		cur.Description, cur.Owner = description, owner
		cur.UpdatedAt = now()
		out = normBranch(cur)
		return putJSON(tx.Bucket(bktBranches), []byte(key(treeID, name)), out)
	})
	if err != nil {
		return nil, err
	}
	return &out, nil
}

// UpdateBranchMeta implements BranchMetaUpdater.
func (m *Memory) UpdateBranchMeta(ctx context.Context, treeID, name, description, owner string) (*domain.Branch, error) {
	var out domain.Branch
	err := m.write(ctx, func() error {
		if err := checkKeys("tree id", treeID, "branch name", name); err != nil {
			return err
		}
		in := inner(m.branches, treeID)
		cur, ok := in[name]
		if !ok {
			return notFound("branch", treeID, name)
		}
		cur.Description, cur.Owner = description, owner
		cur.UpdatedAt = now()
		out = normBranch(cur)
		in[name] = out
		return nil
	})
	if err != nil {
		return nil, err
	}
	return &out, nil
}
