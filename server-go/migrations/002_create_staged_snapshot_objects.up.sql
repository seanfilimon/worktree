CREATE TABLE staged_snapshot_objects (
    id          BIGSERIAL PRIMARY KEY,
    snapshot_id TEXT NOT NULL REFERENCES staged_snapshots(snapshot_id) ON DELETE CASCADE,
    object_hash TEXT NOT NULL,
    CONSTRAINT uq_snapshot_object UNIQUE (snapshot_id, object_hash)
);

CREATE INDEX idx_staged_objects_snapshot ON staged_snapshot_objects (snapshot_id);
