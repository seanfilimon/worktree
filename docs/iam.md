# Identity and access management

W0rkTree servers ship a complete IAM system: accounts and tenants, built-in
and custom roles, teams, per-tree grants, declarative allow/deny policies with
conditions, service accounts with scoped keys, TOTP two-factor
authentication, session management, invitations, admin-issued password
resets and OpenID Connect single sign-on. Everything is available from the
admin panel (`/admin/`), the REST API (`/api/v1`) and the `wt` CLI.

## How a decision is made

For every request the server evaluates *(principal, action, resource)*:

1. **Disabled** accounts and service accounts are denied.
2. **Server admins** are allowed (policies do not apply to them; token
   scopes still do).
3. **Token scopes**: a token created with scopes may only perform matching
   actions (`tree:read`, `tree:*`, `*`).
4. **Role grants** are collected from the tenant membership, every team the
   account belongs to, direct per-tree grants, and a service account's own
   role. Roles are built-in or custom. Tree visibility adds read access
   (`public`: everyone, `internal`: any signed-in account).
5. **Policies** of the resource's tenant are applied: a matching **deny**
   statement wins over everything; a matching **allow** grants the action
   even without a role.
6. Otherwise the request is allowed only if some role grants the action.

`wt iam simulate` (or *Access simulator* in the admin panel) shows every step:

```bash
wt iam simulate --principal user:bob --action branch:push --tenant acme --tree acme/web --branch main
```

## Built-in roles

| Role | Grants |
|---|---|
| `viewer` | `tenant:read`, `tree:read` |
| `developer` | + `tree:write`, `branch:create`, `branch:push`, `mr:create` |
| `maintainer` | + `branch:delete`, `mr:approve`, `mr:merge`, `tree:create`, `iam:read` |
| `admin` | + `tree:admin`, `tenant:admin`, `iam:admin` |
| `owner` | everything in the tenant |

List all actions with `wt iam actions`.

## Teams and grants

Teams group tenant members. Grant a team (or a user, or a service account) a
role on one tree to raise its access there:

```bash
wt team create acme backend --name "Backend"
wt team add acme backend bob
wt grant set acme/api --team backend --role developer
wt grant list acme/api
```

## Custom roles

```bash
wt role create acme reviewer --permission tree:read --permission mr:create --permission mr:approve
wt tenant set-role acme carol reviewer
```

Custom roles are assigned exactly like built-in roles (tenant membership,
grants, service accounts, invitations).

## Policies

A policy is a list of statements:

```json
{
  "name": "protect-main",
  "enabled": true,
  "statements": [
    {
      "sid": "no-contractor-pushes-to-main",
      "effect": "deny",
      "actions": ["branch:push"],
      "resources": ["wt:branch/acme/*/main"],
      "principals": ["team:contractors"]
    },
    {
      "sid": "office-network-only",
      "effect": "deny",
      "actions": ["tree:*"],
      "resources": ["wt:tree/acme/secrets"],
      "principals": ["*"],
      "conditions": { "require_mfa": true, "ip_ranges": ["10.0.0.0/8"] }
    }
  ]
}
```

* **Resources**: `wt:tenant/<t>`, `wt:tree/<t>/<tree>`,
  `wt:branch/<t>/<tree>/<branch>`, `wt:path/<t>/<tree>/<path>`; `*` matches
  one segment, `**` any depth.
* **Principals**: `user:<name>`, `team:<slug>`, `role:<role>`,
  `sa:<name>`, `authenticated`, `anonymous`, `*`.
* **Conditions** (all must hold): `ip_ranges` (CIDR), `require_mfa`,
  `not_before` / `not_after`, `time_window` (`days`, `start`, `end`,
  `timezone`).

```bash
wt policy validate acme --file protect-main.json
wt policy create acme --file protect-main.json
wt policy disable acme <id>
```

## Service accounts

Non-human identities for CI and automation:

```bash
wt sa create acme ci --role developer --description "CI pipeline"
wt sa key-create acme ci --name github-actions --ttl-hours 720 --scope tree:read --scope tree:write
```

The key (`wt_…`) is shown once. Keys inherit the service account's role and
grants, narrowed by their scopes.

## Personal tokens and sessions

```bash
wt token create deploy --ttl-hours 24 --scope tree:read
wt session list
wt session revoke-others
```

## Two-factor authentication

```bash
wt mfa enable          # shows a QR code, asks for a code, prints recovery codes
wt login https://wt.example.com -u alice --mfa-code 123456
wt mfa recovery-codes  # regenerate (asks for a current code)
```

TOTP follows RFC 6238 (SHA-1, 6 digits, 30 s). Recovery codes are single use.
Policies can require MFA with `"require_mfa": true`.

## Invitations and password resets

```bash
wt invite create acme --user carol --role viewer --team backend   # prints a one-time code
wt invite accept wti_…                                            # run as carol
wt admin reset-code dave --ttl-hours 1                            # server admin
wt password-reset wtr_… --password-stdin                          # run as dave
```

Resetting a password revokes all of the account's sessions and tokens.

## Single sign-on (OpenID Connect)

```bash
wt sso set --enabled true --name Okta --issuer https://acme.okta.com \
  --client-id wt-server --client-secret-stdin \
  --redirect-url https://wt.example.com/api/v1/auth/sso/callback \
  --allowed-domain acme.com --auto-create true
```

The login page then shows **Sign in with Okta**. The flow uses PKCE, state
and nonce, and verifies ID-token signatures (RS256/ES256), issuer, audience
and expiry. `--link-by-email true` links a first-time identity to an existing
local account with the same verified email; it is **off by default** because
local account emails are not verified.

## Audit

Every IAM change and sign-in is recorded (`team.create`, `policy.update`,
`grant.set`, `sa.key.create`, `mfa.enable`, `session.revoke`,
`invitation.accept`, `password.reset`, `sso.login`, …):

```bash
wt audit --tenant acme --action policy.update
```
