/**
 * v0.4 IAM endpoints (docs/ENGINEERING_PLAN.md §12.3): teams, roles,
 * policies, tree grants, service accounts, invitations, the action catalog,
 * the access simulator and effective permissions.
 *
 * Field names mirror services/server-go/internal/domain/iam.go. Go encodes
 * empty slices as `null`; every function here normalises them to `[]`.
 */
import { keepPreviousData, useQuery } from "@tanstack/react-query";
import { useMemo } from "react";
import type { ApiClient } from "./client";
import { useApi } from "./context";
import type { Account, APIToken, ISODateString, Tenant } from "./types";

// ---- wire types ----

export interface IamAction {
  action: string;
  description: string;
  area: string;
}

export const TEAM_ROLES = ["maintainer", "member"] as const;
export type TeamRole = (typeof TEAM_ROLES)[number];

export interface Team {
  id: string;
  tenant_id: string;
  slug: string;
  name: string;
  description: string;
  created_at: ISODateString;
  updated_at: ISODateString;
}

export interface TeamMember {
  account: Account;
  role: TeamRole;
}

export interface TeamDetail {
  team: Team;
  members: TeamMember[];
  grants: Grant[];
}

export interface CreateTeamRequest {
  slug: string;
  name: string;
  description: string;
}

export interface UpdateTeamRequest {
  name?: string;
  description?: string;
}

export interface RoleDef {
  name: string;
  description: string;
  permissions: string[];
  builtin: boolean;
}

export interface CreateRoleRequest {
  name: string;
  description: string;
  permissions: string[];
}

export interface UpdateRoleRequest {
  description?: string;
  permissions?: string[];
}

export type Effect = "allow" | "deny";

export interface TimeWindow {
  /** "mon".."sun"; empty = every day. */
  days: string[];
  /** "09:00" */
  start: string;
  /** "18:00", exclusive; may wrap midnight. */
  end: string;
  /** IANA name, default "UTC". */
  timezone: string;
}

export interface PolicyConditions {
  ip_ranges?: string[];
  require_mfa?: boolean;
  not_before?: ISODateString;
  not_after?: ISODateString;
  time_window?: TimeWindow;
}

export interface PolicyStatement {
  sid?: string;
  effect: Effect;
  actions: string[];
  resources: string[];
  principals: string[];
  conditions: PolicyConditions;
}

export interface Policy {
  id: string;
  tenant_id: string;
  name: string;
  description: string;
  enabled: boolean;
  statements: PolicyStatement[];
  version: number;
  created_by: string;
  created_at: ISODateString;
  updated_at: ISODateString;
}

export interface PolicyRequest {
  name: string;
  description: string;
  enabled: boolean;
  statements: PolicyStatement[];
}

export interface PolicyValidationError {
  /** Zero-based statement index. */
  statement: number;
  /** e.g. "actions", "resources", "conditions.ip_ranges". */
  field: string;
  message: string;
}

export interface PolicyValidation {
  valid: boolean;
  errors: PolicyValidationError[];
}

export const SUBJECT_TYPES = ["user", "team", "service_account"] as const;
export type SubjectType = (typeof SUBJECT_TYPES)[number];

export interface Grant {
  tree_id: string;
  subject_type: SubjectType;
  subject_id: string;
  /** Username / team slug / service-account name. */
  subject: string;
  role: string;
  created_by: string;
  created_at: ISODateString;
}

export interface PutGrantRequest {
  subject_type: SubjectType;
  subject: string;
  role: string;
}

export interface ServiceAccount {
  id: string;
  tenant_id: string;
  name: string;
  description: string;
  role: string;
  disabled: boolean;
  created_by: string;
  created_at: ISODateString;
  updated_at: ISODateString;
}

/** APIToken with the v0.4 fields. */
export interface IamToken extends APIToken {
  kind?: "session" | "api" | "service";
  service_account_id?: string;
  scopes?: string[];
  mfa?: boolean;
  last_ip?: string;
  user_agent?: string;
}

export interface ServiceAccountDetail {
  service_account: ServiceAccount;
  keys: IamToken[];
}

export interface CreateServiceAccountRequest {
  name: string;
  description: string;
  role: string;
}

export interface UpdateServiceAccountRequest {
  description?: string;
  role?: string;
  disabled?: boolean;
}

export interface CreateKeyRequest {
  name: string;
  ttl_hours: number;
  scopes: string[];
}

export interface CreatedKey {
  /** Raw bearer token, only ever returned once. */
  token: string;
  api_token: IamToken;
}

export interface Invitation {
  id: string;
  tenant_id: string;
  email?: string;
  username?: string;
  role: string;
  team_id?: string;
  invited_by: string;
  created_at: ISODateString;
  expires_at: ISODateString;
  accepted_at?: ISODateString;
  accepted_by?: string;
  revoked: boolean;
}

export interface CreateInvitationRequest {
  email?: string;
  username?: string;
  role: string;
  /** Team slug to join on acceptance. */
  team?: string;
}

export interface CreatedInvitation {
  invitation: Invitation;
  code: string;
}

export interface InvitationPreview {
  invitation: Invitation;
  tenant: Tenant;
}

export type PrincipalType = "user" | "service_account";

export interface SimulateRequest {
  principal: { type: PrincipalType; name: string; tenant?: string };
  action: string;
  tenant: string;
  tree?: string;
  branch?: string;
  path?: string;
  context: { ip?: string; mfa?: boolean };
}

export type ReasonSource =
  | "server_admin"
  | "disabled"
  | "tenant_role"
  | "team_role"
  | "tree_grant"
  | "custom_role"
  | "service_account"
  | "visibility"
  | "token_scope"
  | "policy"
  | "default"
  | (string & {});

export interface DecisionReason {
  source: ReasonSource;
  detail: string;
  effect: "allow" | "deny" | "info";
}

export interface Decision {
  allowed: boolean;
  effect: "allow" | "deny" | "implicit_deny";
  reasons: DecisionReason[];
}

export interface EffectiveQuery {
  tenant: string;
  tree?: string;
  /** `user:<name>` or `sa:<tenant>/<name>`; omitted = the caller. */
  principal?: string;
}

export interface EffectivePermissions {
  role: string;
  team_roles: { team: string; role: string }[];
  grants: Grant[];
  permissions: string[];
}

// ---- derived helpers ----

export type InvitationStatus = "pending" | "accepted" | "expired" | "revoked";

export function invitationStatus(inv: Invitation, now: number = Date.now()): InvitationStatus {
  if (inv.revoked) return "revoked";
  if (inv.accepted_at) return "accepted";
  if (Date.parse(inv.expires_at) <= now) return "expired";
  return "pending";
}

/** The area of an action ("tree:read" → "tree"). */
export function actionArea(action: string): string {
  const i = action.indexOf(":");
  return i > 0 ? action.slice(0, i) : action;
}

/** Groups actions by area, keeping first-seen area order. */
export function groupByArea<T>(items: readonly T[], area: (item: T) => string): [string, T[]][] {
  const groups = new Map<string, T[]>();
  for (const it of items) {
    const a = area(it);
    const g = groups.get(a);
    if (g) g.push(it);
    else groups.set(a, [it]);
  }
  return [...groups];
}

/** Whether a permission list (concrete, `area:*` or `*`) covers `action`. */
export function permits(permissions: readonly string[], action: string): boolean {
  return permissions.some((p) => p === "*" || p === action || (p.endsWith(":*") && actionArea(action) === p.slice(0, -2)));
}

// ---- endpoints ----

const V1 = "/api/v1";
const seg = encodeURIComponent;
const arr = <T>(v: T[] | null | undefined): T[] => v ?? [];

function normStatement(s: PolicyStatement): PolicyStatement {
  return {
    ...s,
    actions: arr(s.actions),
    resources: arr(s.resources),
    principals: arr(s.principals),
    conditions: s.conditions ?? {},
  };
}

function normPolicy(p: Policy): Policy {
  return { ...p, statements: arr(p.statements).map(normStatement) };
}

function normRole(r: RoleDef): RoleDef {
  return { ...r, permissions: arr(r.permissions) };
}

/** IAM endpoints bound to a client. */
export function createIamApi(client: ApiClient) {
  const t = (tenant: string) => `${V1}/tenants/${seg(tenant)}`;
  const tree = (tenant: string, name: string) => `${V1}/trees/${seg(tenant)}/${seg(name)}`;

  return {
    actions: async () => arr(await client.request<IamAction[] | null>(`${V1}/iam/actions`)),

    teams: {
      list: async (tenant: string) => arr(await client.request<Team[] | null>(`${t(tenant)}/teams`)),
      create: (tenant: string, body: CreateTeamRequest) =>
        client.request<Team>(`${t(tenant)}/teams`, { method: "POST", body }),
      get: async (tenant: string, team: string): Promise<TeamDetail> => {
        const d = await client.request<TeamDetail>(`${t(tenant)}/teams/${seg(team)}`);
        return { team: d.team, members: arr(d.members), grants: arr(d.grants) };
      },
      update: (tenant: string, team: string, body: UpdateTeamRequest) =>
        client.request<Team>(`${t(tenant)}/teams/${seg(team)}`, { method: "PATCH", body }),
      remove: (tenant: string, team: string) =>
        client.request<void>(`${t(tenant)}/teams/${seg(team)}`, { method: "DELETE" }),
      setMember: (tenant: string, team: string, username: string, role: TeamRole) =>
        client.request<TeamMember>(`${t(tenant)}/teams/${seg(team)}/members/${seg(username)}`, {
          method: "PUT",
          body: { role },
        }),
      removeMember: (tenant: string, team: string, username: string) =>
        client.request<void>(`${t(tenant)}/teams/${seg(team)}/members/${seg(username)}`, { method: "DELETE" }),
    },

    roles: {
      list: async (tenant: string) => arr(await client.request<RoleDef[] | null>(`${t(tenant)}/roles`)).map(normRole),
      get: async (tenant: string, name: string) => normRole(await client.request<RoleDef>(`${t(tenant)}/roles/${seg(name)}`)),
      create: (tenant: string, body: CreateRoleRequest) =>
        client.request<RoleDef>(`${t(tenant)}/roles`, { method: "POST", body }),
      update: (tenant: string, name: string, body: UpdateRoleRequest) =>
        client.request<RoleDef>(`${t(tenant)}/roles/${seg(name)}`, { method: "PATCH", body }),
      remove: (tenant: string, name: string) =>
        client.request<void>(`${t(tenant)}/roles/${seg(name)}`, { method: "DELETE" }),
    },

    policies: {
      list: async (tenant: string) => arr(await client.request<Policy[] | null>(`${t(tenant)}/policies`)).map(normPolicy),
      get: async (tenant: string, id: string) => normPolicy(await client.request<Policy>(`${t(tenant)}/policies/${seg(id)}`)),
      create: (tenant: string, body: PolicyRequest) =>
        client.request<Policy>(`${t(tenant)}/policies`, { method: "POST", body }),
      replace: (tenant: string, id: string, body: PolicyRequest) =>
        client.request<Policy>(`${t(tenant)}/policies/${seg(id)}`, { method: "PUT", body }),
      remove: (tenant: string, id: string) =>
        client.request<void>(`${t(tenant)}/policies/${seg(id)}`, { method: "DELETE" }),
      validate: async (tenant: string, statements: unknown, signal?: AbortSignal): Promise<PolicyValidation> => {
        const r = await client.request<PolicyValidation>(`${t(tenant)}/policies/validate`, {
          method: "POST",
          body: { statements },
          signal,
        });
        return { valid: r.valid, errors: arr(r.errors) };
      },
    },

    grants: {
      list: async (tenant: string, name: string) => arr(await client.request<Grant[] | null>(`${tree(tenant, name)}/grants`)),
      put: (tenant: string, name: string, body: PutGrantRequest) =>
        client.request<Grant>(`${tree(tenant, name)}/grants`, { method: "PUT", body }),
      remove: (tenant: string, name: string, subjectType: SubjectType, subject: string) =>
        client.request<void>(`${tree(tenant, name)}/grants`, {
          method: "DELETE",
          query: { subject_type: subjectType, subject },
        }),
    },

    serviceAccounts: {
      list: async (tenant: string) =>
        arr(await client.request<ServiceAccount[] | null>(`${t(tenant)}/service-accounts`)),
      create: (tenant: string, body: CreateServiceAccountRequest) =>
        client.request<ServiceAccount>(`${t(tenant)}/service-accounts`, { method: "POST", body }),
      get: async (tenant: string, name: string): Promise<ServiceAccountDetail> => {
        const d = await client.request<ServiceAccountDetail>(`${t(tenant)}/service-accounts/${seg(name)}`);
        return { service_account: d.service_account, keys: arr(d.keys) };
      },
      update: (tenant: string, name: string, body: UpdateServiceAccountRequest) =>
        client.request<ServiceAccount>(`${t(tenant)}/service-accounts/${seg(name)}`, { method: "PATCH", body }),
      remove: (tenant: string, name: string) =>
        client.request<void>(`${t(tenant)}/service-accounts/${seg(name)}`, { method: "DELETE" }),
      createKey: (tenant: string, name: string, body: CreateKeyRequest) =>
        client.request<CreatedKey>(`${t(tenant)}/service-accounts/${seg(name)}/keys`, { method: "POST", body }),
      revokeKey: (tenant: string, name: string, id: string) =>
        client.request<void>(`${t(tenant)}/service-accounts/${seg(name)}/keys/${seg(id)}`, { method: "DELETE" }),
    },

    invitations: {
      list: async (tenant: string) => arr(await client.request<Invitation[] | null>(`${t(tenant)}/invitations`)),
      create: (tenant: string, body: CreateInvitationRequest) =>
        client.request<CreatedInvitation>(`${t(tenant)}/invitations`, { method: "POST", body }),
      revoke: (tenant: string, id: string) =>
        client.request<void>(`${t(tenant)}/invitations/${seg(id)}`, { method: "DELETE" }),
    },

    simulate: async (body: SimulateRequest): Promise<Decision> => {
      const d = await client.request<Decision>(`${V1}/iam/simulate`, { method: "POST", body });
      return { ...d, reasons: arr(d.reasons) };
    },

    effective: async (q: EffectiveQuery): Promise<EffectivePermissions> => {
      const e = await client.request<EffectivePermissions>(`${V1}/iam/effective`, { query: { ...q } });
      return {
        role: e.role ?? "",
        team_roles: arr(e.team_roles),
        grants: arr(e.grants),
        permissions: arr(e.permissions),
      };
    },
  };
}

export type IamApi = ReturnType<typeof createIamApi>;

// ---- query keys + hooks ----

export const iamKeys = {
  actions: ["iam", "actions"] as const,
  tenantIam: (t: string) => ["tenant", t, "iam"] as const,
  teams: (t: string) => ["tenant", t, "iam", "teams"] as const,
  team: (t: string, slug: string) => ["tenant", t, "iam", "teams", slug] as const,
  roles: (t: string) => ["tenant", t, "iam", "roles"] as const,
  policies: (t: string) => ["tenant", t, "iam", "policies"] as const,
  validate: (t: string, statements: string) => ["tenant", t, "iam", "validate", statements] as const,
  serviceAccounts: (t: string) => ["tenant", t, "iam", "service-accounts"] as const,
  serviceAccount: (t: string, name: string) => ["tenant", t, "iam", "service-accounts", name] as const,
  invitations: (t: string) => ["tenant", t, "iam", "invitations"] as const,
  grants: (t: string, n: string) => ["tree", t, n, "grants"] as const,
  effective: (q: EffectiveQuery) => ["iam", "effective", q] as const,
};

/** The IAM API bound to the app's client. */
export function useIamApi(): IamApi {
  const { client } = useApi();
  return useMemo(() => createIamApi(client), [client]);
}

export function useIamActions() {
  const iam = useIamApi();
  return useQuery({ queryKey: iamKeys.actions, queryFn: iam.actions, staleTime: Infinity });
}

export function useTeams(tenant: string) {
  const iam = useIamApi();
  return useQuery({ queryKey: iamKeys.teams(tenant), queryFn: () => iam.teams.list(tenant) });
}

export function useTeam(tenant: string, slug: string) {
  const iam = useIamApi();
  return useQuery({ queryKey: iamKeys.team(tenant, slug), queryFn: () => iam.teams.get(tenant, slug), enabled: slug !== "" });
}

export function useRoles(tenant: string, enabled = true) {
  const iam = useIamApi();
  return useQuery({ queryKey: iamKeys.roles(tenant), queryFn: () => iam.roles.list(tenant), enabled: enabled && tenant !== "" });
}

export function usePolicies(tenant: string) {
  const iam = useIamApi();
  return useQuery({ queryKey: iamKeys.policies(tenant), queryFn: () => iam.policies.list(tenant) });
}

/** Server-side validation of `statements` (pass the JSON text so the key is stable). */
export function usePolicyValidation(tenant: string, statementsJson: string, enabled: boolean) {
  const iam = useIamApi();
  return useQuery({
    queryKey: iamKeys.validate(tenant, statementsJson),
    queryFn: ({ signal }) => iam.policies.validate(tenant, JSON.parse(statementsJson) as unknown, signal),
    enabled,
    placeholderData: keepPreviousData,
    staleTime: Infinity,
  });
}

export function useServiceAccounts(tenant: string, enabled = true) {
  const iam = useIamApi();
  return useQuery({
    queryKey: iamKeys.serviceAccounts(tenant),
    queryFn: () => iam.serviceAccounts.list(tenant),
    enabled: enabled && tenant !== "",
  });
}

export function useServiceAccount(tenant: string, name: string) {
  const iam = useIamApi();
  return useQuery({
    queryKey: iamKeys.serviceAccount(tenant, name),
    queryFn: () => iam.serviceAccounts.get(tenant, name),
    enabled: name !== "",
  });
}

export function useInvitations(tenant: string) {
  const iam = useIamApi();
  return useQuery({ queryKey: iamKeys.invitations(tenant), queryFn: () => iam.invitations.list(tenant) });
}

export function useTreeGrants(tenant: string, tree: string) {
  const iam = useIamApi();
  return useQuery({ queryKey: iamKeys.grants(tenant, tree), queryFn: () => iam.grants.list(tenant, tree) });
}

export function useEffectivePermissions(q: EffectiveQuery | null) {
  const iam = useIamApi();
  return useQuery({
    queryKey: iamKeys.effective(q ?? { tenant: "" }),
    queryFn: () => iam.effective(q ?? { tenant: "" }),
    enabled: q !== null && q.tenant !== "",
  });
}
