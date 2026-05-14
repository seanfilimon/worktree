CREATE TABLE tenant_policies (
    id SERIAL PRIMARY KEY,
    tenant TEXT NOT NULL,
    effect TEXT NOT NULL,
    account TEXT NOT NULL DEFAULT '',
    actions TEXT[] NOT NULL,
    resources TEXT[] NOT NULL,
    conditions JSONB
);

CREATE INDEX idx_tenant_policies_tenant ON tenant_policies(tenant);

-- Insert default global policies
INSERT INTO tenant_policies (tenant, effect, actions, resources, conditions)
VALUES 
    ('', 'allow', ARRAY['staged:create', 'staged:list'], ARRAY['tenant:${tenant}/*'], '{"scope": "staged:*"}'),
    ('', 'allow', ARRAY['branch:push', 'branch:pull', 'object:check', 'object:read', 'object:write', 'ref:list'], ARRAY['tenant:${tenant}/*', 'tenant:${tenant}'], '{}');
