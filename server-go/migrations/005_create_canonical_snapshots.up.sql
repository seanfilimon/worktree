CREATE TABLE canonical_snapshots (
    snapshot_id   TEXT        PRIMARY KEY,
    tenant        TEXT        NOT NULL,
    worktree      TEXT        NOT NULL,
    tree_id       TEXT        NOT NULL,
    branch        TEXT        NOT NULL,
    parents       JSONB       NOT NULL DEFAULT '[]'::jsonb,
    manifest_hash TEXT        NOT NULL,
    message       TEXT        NOT NULL DEFAULT '',
    author        TEXT        NOT NULL DEFAULT '',
    committed_at  TIMESTAMPTZ NOT NULL,
    payload       JSONB       NOT NULL
);

CREATE INDEX idx_canonical_snapshots_branch
    ON canonical_snapshots (tenant, worktree, tree_id, branch);
CREATE INDEX idx_canonical_snapshots_committed_at
    ON canonical_snapshots (committed_at);
