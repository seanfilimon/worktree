CREATE TABLE tenant_permissions (
    id SERIAL PRIMARY KEY,
    tenant TEXT NOT NULL,
    effect TEXT NOT NULL,
    account TEXT NOT NULL DEFAULT '',
    action TEXT NOT NULL,
    resource_pattern TEXT NOT NULL,
    conditions JSONB
);

CREATE INDEX idx_tenant_permissions_tenant_account ON tenant_permissions(tenant, account);

-- Insert default global policies
INSERT INTO tenant_permissions (tenant, effect, action, resource_pattern, conditions)
VALUES
    ('', 'allow', 'staged:create', 'tenant:${tenant}/*', '{"scope": "staged:*"}'),
    ('', 'allow', 'staged:list', 'tenant:${tenant}/*', '{"scope": "staged:*"}'),
    ('', 'allow', 'branch:push', 'tenant:${tenant}/*', '{}'),
    ('', 'allow', 'branch:pull', 'tenant:${tenant}/*', '{}'),
    ('', 'allow', 'object:check', 'tenant:${tenant}/*', '{}'),
    ('', 'allow', 'object:read', 'tenant:${tenant}/*', '{}'),
    ('', 'allow', 'object:write', 'tenant:${tenant}/*', '{}'),
    ('', 'allow', 'ref:list', 'tenant:${tenant}/*', '{}'),
    ('', 'allow', 'branch:push', 'tenant:${tenant}', '{}'),
    ('', 'allow', 'branch:pull', 'tenant:${tenant}', '{}'),
    ('', 'allow', 'object:check', 'tenant:${tenant}', '{}'),
    ('', 'allow', 'object:read', 'tenant:${tenant}', '{}'),
    ('', 'allow', 'object:write', 'tenant:${tenant}', '{}'),
    ('', 'allow', 'ref:list', 'tenant:${tenant}', '{}');
