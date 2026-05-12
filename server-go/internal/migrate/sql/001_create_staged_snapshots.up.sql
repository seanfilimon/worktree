CREATE TABLE staged_snapshots (
    id           BIGSERIAL PRIMARY KEY,
    snapshot_id  TEXT        NOT NULL,
    tenant       TEXT        NOT NULL,
    worktree     TEXT        NOT NULL,
    tree_id      TEXT        NOT NULL,
    branch       TEXT        NOT NULL,
    payload_hash TEXT        NOT NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT uq_staged_snapshot_identity UNIQUE (tenant, worktree, tree_id, branch, snapshot_id)
);

CREATE INDEX idx_staged_snapshots_tenant   ON staged_snapshots (tenant);
CREATE INDEX idx_staged_snapshots_worktree ON staged_snapshots (tenant, worktree);
CREATE INDEX idx_staged_snapshots_branch   ON staged_snapshots (tenant, worktree, branch);
