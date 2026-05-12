CREATE TABLE canonical_branches (
    tenant          TEXT        NOT NULL,
    worktree        TEXT        NOT NULL,
    tree_id         TEXT        NOT NULL,
    name            TEXT        NOT NULL,
    tip_snapshot_id TEXT,
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (tenant, worktree, tree_id, name)
);

CREATE INDEX idx_canonical_branches_tenant   ON canonical_branches (tenant);
CREATE INDEX idx_canonical_branches_worktree ON canonical_branches (tenant, worktree);
CREATE INDEX idx_canonical_branches_tree     ON canonical_branches (tenant, worktree, tree_id);
