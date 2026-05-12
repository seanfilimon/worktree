CREATE TABLE canonical_snapshot_objects (
    snapshot_id TEXT   NOT NULL REFERENCES canonical_snapshots(snapshot_id) ON DELETE CASCADE,
    object_hash TEXT   NOT NULL,
    path        TEXT   NOT NULL,
    size        BIGINT NOT NULL,
    PRIMARY KEY (snapshot_id, object_hash, path)
);

CREATE INDEX idx_canonical_snapshot_objects_hash
    ON canonical_snapshot_objects (object_hash);
CREATE INDEX idx_canonical_snapshot_objects_snapshot
    ON canonical_snapshot_objects (snapshot_id);
