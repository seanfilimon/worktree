import type {
  Decision,
  EffectivePermissions,
  Grant,
  IamAction,
  IamToken,
  Invitation,
  Policy,
  RoleDef,
  ServiceAccount,
  Team,
  TeamDetail,
} from "@/api/iam";
import * as fx from "./fixtures";
import * as px from "./fixtures.people";

const T0 = "2026-10-20T12:00:00Z";
const inDays = (d: number) => new Date(Date.now() + d * 86_400_000).toISOString();

export const actions: IamAction[] = [
  { action: "tree:read", description: "Read trees and history", area: "tree" },
  { action: "tree:write", description: "Push and stage", area: "tree" },
  { action: "tree:admin", description: "Tree settings", area: "tree" },
  { action: "branch:push", description: "Push to a branch", area: "branch" },
  { action: "mr:merge", description: "Merge merge requests", area: "mr" },
  { action: "iam:read", description: "View IAM", area: "iam" },
];

export const roles: RoleDef[] = [
  { name: "owner", description: "", permissions: ["*"], builtin: true },
  { name: "admin", description: "", permissions: ["tree:*", "branch:*", "mr:*", "iam:*"], builtin: true },
  { name: "maintainer", description: "", permissions: ["tree:read", "tree:write", "branch:push", "mr:merge", "iam:read"], builtin: true },
  { name: "developer", description: "", permissions: ["tree:read", "tree:write", "branch:push"], builtin: true },
  { name: "viewer", description: "", permissions: ["tree:read"], builtin: true },
  { name: "release-manager", description: "Merges release branches", permissions: ["tree:read", "mr:merge"], builtin: false },
];

export const team: Team = {
  id: "team-1",
  tenant_id: fx.tenant.id,
  slug: "platform",
  name: "Platform",
  description: "Infra people",
  created_at: T0,
  updated_at: T0,
};

export const grant: Grant = {
  tree_id: fx.summary.tree.id,
  subject_type: "team",
  subject_id: team.id,
  subject: "platform",
  role: "maintainer",
  created_by: "alice",
  created_at: T0,
};

export const teamDetail: TeamDetail = {
  team,
  members: [{ account: fx.bob, role: "maintainer" }],
  grants: [grant],
};

export const policy: Policy = {
  id: "pol-1",
  tenant_id: fx.tenant.id,
  name: "no-friday-deploys",
  description: "Deny pushes to main on Fridays",
  enabled: true,
  statements: [
    {
      effect: "deny",
      actions: ["branch:push"],
      resources: ["wt:branch/acme/*/main"],
      principals: ["*"],
      conditions: { time_window: { days: ["fri"], start: "00:00", end: "23:59", timezone: "UTC" } },
    },
  ],
  version: 3,
  created_by: "alice",
  created_at: T0,
  updated_at: T0,
};

export const serviceAccount: ServiceAccount = {
  id: "sa-1",
  tenant_id: fx.tenant.id,
  name: "ci-bot",
  description: "CI pipeline",
  role: "developer",
  disabled: false,
  created_by: "alice",
  created_at: T0,
  updated_at: T0,
};

export const key: IamToken = {
  id: "key-1",
  account_id: "",
  name: "github-actions",
  created_at: T0,
  kind: "service",
  service_account_id: "sa-1",
  scopes: ["tree:read"],
};

const invitation = (over: Partial<Invitation>): Invitation => ({
  id: "inv-1",
  tenant_id: fx.tenant.id,
  role: "developer",
  invited_by: "alice",
  created_at: T0,
  expires_at: inDays(5),
  revoked: false,
  ...over,
});

export const invitations: Invitation[] = [
  invitation({ id: "inv-1", email: "dana@example.com", team_id: team.id }),
  invitation({ id: "inv-2", username: "erin", accepted_at: T0, accepted_by: "erin" }),
  invitation({ id: "inv-3", email: "old@example.com", expires_at: "2026-01-01T00:00:00Z" }),
  invitation({ id: "inv-4", email: "nope@example.com", revoked: true }),
];

export const decision: Decision = {
  allowed: false,
  effect: "deny",
  reasons: [
    { source: "tenant_role", detail: "bob is developer in acme", effect: "allow" },
    { source: "policy", detail: "no-friday-deploys statement 1 denies branch:push", effect: "deny" },
  ],
};

export const effective: EffectivePermissions = {
  role: "developer",
  team_roles: [{ team: "platform", role: "maintainer" }],
  grants: [grant],
  permissions: ["tree:read", "tree:write", "mr:merge"],
};

/** Routes for /tenants/acme with every IAM list. */
export function tenantIamRoutes() {
  const t = "/api/v1/tenants/acme";
  return {
    [`GET ${t}`]: fx.tenant,
    [`GET ${t}/trees`]: px.acmeTrees,
    [`GET ${t}/members`]: px.acmeMembers,
    "GET /api/v1/iam/actions": actions,
    [`GET ${t}/teams`]: [team],
    [`GET ${t}/teams/platform`]: teamDetail,
    [`GET ${t}/roles`]: roles,
    [`GET ${t}/policies`]: [policy],
    [`POST ${t}/policies/validate`]: { valid: true, errors: null },
    [`GET ${t}/service-accounts`]: [serviceAccount],
    [`GET ${t}/service-accounts/ci-bot`]: { service_account: serviceAccount, keys: [key] },
    [`GET ${t}/invitations`]: invitations,
  };
}
