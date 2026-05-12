CREATE TABLE audit_events (
    id          BIGSERIAL    PRIMARY KEY,
    recorded_at TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    event       TEXT         NOT NULL,
    action      TEXT         NOT NULL,
    decision    TEXT         NOT NULL,
    reason      TEXT,
    tenant      TEXT,
    account     TEXT,
    resource    TEXT,
    request_id  TEXT,
    http_method TEXT,
    http_path   TEXT
);

CREATE INDEX idx_audit_events_tenant  ON audit_events (tenant);
CREATE INDEX idx_audit_events_action  ON audit_events (action, decision);
