CREATE TABLE staged_snapshot_objects (
    id                 BIGSERIAL PRIMARY KEY,
    staged_snapshot_id BIGINT NOT NULL REFERENCES staged_snapshots(id) ON DELETE CASCADE,
    snapshot_id        TEXT NOT NULL,
    object_hash        TEXT NOT NULL,
    path               TEXT NOT NULL,
    size               BIGINT NOT NULL,
    CONSTRAINT uq_snapshot_object UNIQUE (staged_snapshot_id, path, object_hash)
);

CREATE INDEX idx_staged_objects_snapshot_id ON staged_snapshot_objects (staged_snapshot_id);
CREATE INDEX idx_staged_objects_snapshot    ON staged_snapshot_objects (snapshot_id);
