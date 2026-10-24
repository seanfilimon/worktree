import { encodePath, type ApiClient } from "./client";
import type {
  Account,
  APIToken,
  AuditEvent,
  AuditQuery,
  AuthResponse,
  Branch,
  BranchProtection,
  ChangePasswordRequest,
  CompareResult,
  CreateAccountRequest,
  CreateBranchRequest,
  CreateTagRequest,
  FileHistoryEntry,
  SearchResults,
  ServerEvent,
  StagedBranchActivity,
  SystemInfo,
  TreeStats,
  UpdateTenantRequest,
  CreatedToken,
  CreateMergeRequestRequest,
  CreateTenantRequest,
  CreateTokenRequest,
  CreateTreeRequest,
  Health,
  LicenseRule,
  LoginRequest,
  MeResponse,
  Member,
  MergeRequest,
  MergeRequestState,
  RegisterRequest,
  Role,
  ServerStats,
  Snapshot,
  SnapshotDetail,
  StagedSnapshot,
  Tag,
  Tenant,
  Tree,
  TreeSummary,
  UpdateAccountRequest,
  UpdateTreeRequest,
} from "./types";

const V1 = "/api/v1";
const seg = encodeURIComponent;

/** Null-tolerant list helper: Go encodes empty slices as `null`. */
async function list<T>(p: Promise<T[] | null>): Promise<T[]> {
  return (await p) ?? [];
}

/** Every REST endpoint of §7, bound to a client. */
export function createApi(client: ApiClient) {
  const tree = (tenant: string, name: string) => `${V1}/trees/${seg(tenant)}/${seg(name)}`;

  return {
    client,

    system: {
      health: () => client.request<Health>("/healthz"),
      metrics: async () => (await client.raw("/metrics", { skipAuthRedirect: true })).text(),
    },

    auth: {
      register: (body: RegisterRequest) =>
        client.request<AuthResponse>(`${V1}/auth/register`, { method: "POST", body, skipAuthRedirect: true }),
      login: (body: LoginRequest) =>
        client.request<AuthResponse>(`${V1}/auth/login`, { method: "POST", body, skipAuthRedirect: true }),
      logout: () => client.request<void>(`${V1}/auth/logout`, { method: "POST", skipAuthRedirect: true }),
      me: () => client.request<MeResponse>(`${V1}/auth/me`),
      listTokens: () => list(client.request<APIToken[] | null>(`${V1}/auth/tokens`)),
      createToken: (body: CreateTokenRequest) =>
        client.request<CreatedToken>(`${V1}/auth/tokens`, { method: "POST", body }),
      revokeToken: (id: string) => client.request<void>(`${V1}/auth/tokens/${seg(id)}`, { method: "DELETE" }),
      changePassword: (body: ChangePasswordRequest) =>
        client.request<void>(`${V1}/auth/password`, { method: "POST", body }),
    },

    tenants: {
      list: () => list(client.request<Tenant[] | null>(`${V1}/tenants`)),
      create: (body: CreateTenantRequest) => client.request<Tenant>(`${V1}/tenants`, { method: "POST", body }),
      get: (tenant: string) => client.request<Tenant>(`${V1}/tenants/${seg(tenant)}`),
      update: (tenant: string, body: UpdateTenantRequest) =>
        client.request<Tenant>(`${V1}/tenants/${seg(tenant)}`, { method: "PATCH", body }),
      members: (tenant: string) => list(client.request<Member[] | null>(`${V1}/tenants/${seg(tenant)}/members`)),
      setMember: (tenant: string, username: string, role: Role) =>
        client.request<Member>(`${V1}/tenants/${seg(tenant)}/members/${seg(username)}`, {
          method: "PUT",
          body: { role },
        }),
      removeMember: (tenant: string, username: string) =>
        client.request<void>(`${V1}/tenants/${seg(tenant)}/members/${seg(username)}`, { method: "DELETE" }),
      trees: (tenant: string) => list(client.request<TreeSummary[] | null>(`${V1}/tenants/${seg(tenant)}/trees`)),
      createTree: (tenant: string, body: CreateTreeRequest) =>
        client.request<Tree>(`${V1}/tenants/${seg(tenant)}/trees`, { method: "POST", body }),
    },

    trees: {
      get: (tenant: string, name: string) => client.request<TreeSummary>(tree(tenant, name)),
      update: (tenant: string, name: string, body: UpdateTreeRequest) =>
        client.request<Tree>(tree(tenant, name), { method: "PATCH", body }),
      remove: (tenant: string, name: string) => client.request<void>(tree(tenant, name), { method: "DELETE" }),
      branches: (tenant: string, name: string) => list(client.request<Branch[] | null>(`${tree(tenant, name)}/branches`)),
      snapshots: (tenant: string, name: string, opts: { branch?: string; limit?: number } = {}) =>
        list(
          client.request<Snapshot[] | null>(`${tree(tenant, name)}/snapshots`, {
            query: { branch: opts.branch, limit: opts.limit ?? 50 },
          }),
        ),
      snapshot: (tenant: string, name: string, id: string) =>
        client.request<SnapshotDetail>(`${tree(tenant, name)}/snapshots/${seg(id)}`),
      /** Raw file bytes of `path` at `snapshot`. */
      blob: async (tenant: string, name: string, snapshot: string, path: string, signal?: AbortSignal) => {
        const res = await client.raw(`${tree(tenant, name)}/blob/${seg(snapshot)}/${encodePath(path)}`, { signal });
        return res.arrayBuffer();
      },
      tags: (tenant: string, name: string) => list(client.request<Tag[] | null>(`${tree(tenant, name)}/tags`)),
      staged: (tenant: string, name: string, branch?: string) =>
        list(client.request<StagedSnapshot[] | null>(`${tree(tenant, name)}/staged`, { query: { branch } })),
      protections: (tenant: string, name: string) =>
        list(client.request<BranchProtection[] | null>(`${tree(tenant, name)}/protections`)),
      putProtection: (tenant: string, name: string, body: BranchProtection) =>
        client.request<BranchProtection>(`${tree(tenant, name)}/protections`, { method: "PUT", body }),
      deleteProtection: (tenant: string, name: string, pattern: string) =>
        client.request<void>(`${tree(tenant, name)}/protections`, { method: "DELETE", query: { pattern } }),
      licenses: (tenant: string, name: string) =>
        list(client.request<LicenseRule[] | null>(`${tree(tenant, name)}/licenses`)),
      putLicense: (tenant: string, name: string, body: LicenseRule) =>
        client.request<LicenseRule>(`${tree(tenant, name)}/licenses`, { method: "PUT", body }),
      deleteLicense: (tenant: string, name: string, pathGlob: string) =>
        client.request<void>(`${tree(tenant, name)}/licenses`, { method: "DELETE", query: { path_glob: pathGlob } }),
      createBranch: (tenant: string, name: string, body: CreateBranchRequest) =>
        client.request<Branch>(`${tree(tenant, name)}/branches`, { method: "POST", body }),
      deleteBranch: (tenant: string, name: string, branch: string) =>
        client.request<void>(`${tree(tenant, name)}/branches/${encodePath(branch)}`, { method: "DELETE" }),
      createTag: (tenant: string, name: string, body: CreateTagRequest) =>
        client.request<Tag>(`${tree(tenant, name)}/tags`, { method: "POST", body }),
      deleteTag: (tenant: string, name: string, tag: string) =>
        client.request<void>(`${tree(tenant, name)}/tags/${seg(tag)}`, { method: "DELETE" }),
      /** Diff between two revisions; `from` defaults to `to`'s first parent. */
      compare: (tenant: string, name: string, q: { to: string; from?: string; context?: number }) =>
        client.request<CompareResult>(`${tree(tenant, name)}/compare`, { query: { ...q } }),
      /** Snapshots that changed `path`, newest first. */
      fileHistory: (tenant: string, name: string, q: { path: string; branch?: string; limit?: number }) =>
        list(client.request<FileHistoryEntry[] | null>(`${tree(tenant, name)}/history`, { query: { ...q } })),
      stats: (tenant: string, name: string, q: { branch?: string; days?: number } = {}) =>
        client.request<TreeStats>(`${tree(tenant, name)}/stats`, { query: { ...q } }),
      stagedSummary: (tenant: string, name: string) =>
        list(client.request<StagedBranchActivity[] | null>(`${tree(tenant, name)}/staged/summary`)),
    },

    mergeRequests: {
      list: (tenant: string, name: string, state?: MergeRequestState) =>
        list(client.request<MergeRequest[] | null>(`${tree(tenant, name)}/merge-requests`, { query: { state } })),
      create: (tenant: string, name: string, body: CreateMergeRequestRequest) =>
        client.request<MergeRequest>(`${tree(tenant, name)}/merge-requests`, { method: "POST", body }),
      get: (tenant: string, name: string, n: number) =>
        client.request<MergeRequest>(`${tree(tenant, name)}/merge-requests/${n}`),
      approve: (tenant: string, name: string, n: number) =>
        client.request<MergeRequest>(`${tree(tenant, name)}/merge-requests/${n}/approve`, { method: "POST" }),
      merge: (tenant: string, name: string, n: number) =>
        client.request<MergeRequest>(`${tree(tenant, name)}/merge-requests/${n}/merge`, { method: "POST" }),
      close: (tenant: string, name: string, n: number) =>
        client.request<MergeRequest>(`${tree(tenant, name)}/merge-requests/${n}/close`, { method: "POST" }),
    },

    events: {
      /** Recently published events (server ring buffer), oldest first. */
      recent: (q: { tenant?: string; tree?: string; limit?: number } = {}) =>
        list(client.request<ServerEvent[] | null>(`${V1}/events/recent`, { query: { ...q } })),
    },

    search: (q: string, limit = 20) => client.request<SearchResults>(`${V1}/search`, { query: { q, limit } }),

    audit: {
      list: (q: AuditQuery = {}) => list(client.request<AuditEvent[] | null>(`${V1}/audit`, { query: { ...q } })),
    },

    admin: {
      stats: () => client.request<ServerStats>(`${V1}/admin/stats`),
      accounts: () => list(client.request<Account[] | null>(`${V1}/admin/accounts`)),
      updateAccount: (username: string, body: UpdateAccountRequest) =>
        client.request<Account>(`${V1}/admin/accounts/${seg(username)}`, { method: "PATCH", body }),
      createAccount: (body: CreateAccountRequest) =>
        client.request<Account>(`${V1}/admin/accounts`, { method: "POST", body }),
      setPassword: (username: string, password: string) =>
        client.request<void>(`${V1}/admin/accounts/${seg(username)}/password`, { method: "POST", body: { password } }),
      system: () => client.request<SystemInfo>(`${V1}/admin/system`),
    },
  };
}

export type Api = ReturnType<typeof createApi>;
