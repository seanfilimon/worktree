/**
 * v0.6 endpoints (docs/ENGINEERING_PLAN.md §14, workstream `admin-v06`):
 * webhooks and deliveries, code search, blame, server-side merge methods
 * and mergeability, policy version history, staged file browsing and the
 * tree-level declarative access config.
 *
 * Field names mirror services/server-go/internal/domain/{v06.go,types.go,iam.go}.
 * Go encodes empty slices as `null`; every function here normalises lists to
 * `[]`. Backends that are not deployed yet answer `501 not_implemented` or a
 * `404 "no such endpoint"` from the API's catch-all — see {@link isUnavailable}.
 */
import { keepPreviousData, useQuery } from "@tanstack/react-query";
import { useMemo } from "react";
import { encodePath, isApiError, type ApiClient } from "./client";
import { useApi } from "./context";
import type { Policy, PolicyStatement } from "./iam";
import type { ISODateString, MergeRequest, SnapshotFile, StagedSnapshot } from "./types";
import type { AccessConfigDoc, AccessPlanResult } from "./v05ops";

const V1 = "/api/v1";
const seg = encodeURIComponent;

// ---- wire types ----

// v0.8 conflict-ui (§16.9): "rebase" replays the source snapshots onto the target.
export const MERGE_METHODS = ["fast-forward", "merge", "squash", "rebase"] as const;
export type MergeMethod = (typeof MERGE_METHODS)[number];

export const MERGE_METHOD_LABEL: Record<MergeMethod, string> = {
  "fast-forward": "Fast-forward",
  merge: "Merge snapshot",
  squash: "Squash",
  rebase: "Rebase",
};

export function isMergeMethod(v: unknown): v is MergeMethod {
  return typeof v === "string" && (MERGE_METHODS as readonly string[]).includes(v);
}

/** A webhook (domain.Webhook). `tree_id` empty = every tree of the tenant. */
export interface Webhook {
  id: string;
  tenant_id: string;
  tree_id?: string;
  url: string;
  description?: string;
  /** Event-type patterns ("branch.pushed", "mr.*", "*"). */
  events: string[];
  active: boolean;
  has_secret: boolean;
  created_by: string;
  created_at: ISODateString;
  updated_at: ISODateString;
  last_delivery_at?: ISODateString;
  /** HTTP status of the last attempt (0 = network error). */
  last_status?: number;
  /** Consecutive failed deliveries. */
  failure_count: number;
}

/** The create response: `secret` is present exactly once, when the server generated it. */
export interface CreatedWebhook extends Webhook {
  secret?: string;
}

export interface CreateWebhookRequest {
  url: string;
  events: string[];
  /** Empty = the server generates one (returned once). */
  secret?: string;
  description?: string;
  active?: boolean;
}

export type UpdateWebhookRequest = Partial<CreateWebhookRequest>;

/** One delivery (with its retries) of an event (domain.WebhookDelivery). */
export interface WebhookDelivery {
  id: string;
  webhook_id: string;
  event: string;
  payload?: unknown;
  attempts: number;
  status_code: number;
  /** First 2 KiB of the response body. */
  response?: string;
  error?: string;
  duration_ms: number;
  succeeded: boolean;
  created_at: ISODateString;
  delivered_at?: ISODateString;
  next_retry_at?: ISODateString;
  redelivery?: boolean;
}

/** Tenant-wide (`tree` unset) or tree-scoped webhooks. */
export interface WebhookScope {
  tenant: string;
  tree?: string;
}

/** One matching line of a code search (domain.CodeSearchHit). */
export interface CodeSearchHit {
  tenant: string;
  tree: string;
  branch: string;
  snapshot_id: string;
  path: string;
  /** 1-based. */
  line: number;
  text: string;
  before?: string[];
  after?: string[];
}

export interface CodeSearchQuery {
  q: string;
  tenant?: string;
  tree?: string;
  /** Glob over file paths. */
  path?: string;
  regex?: boolean;
  /** Case-sensitive. */
  case?: boolean;
  context?: number;
  limit?: number;
}

export interface CodeSearchResult {
  hits: CodeSearchHit[];
  truncated: boolean;
  searched_trees: number;
}

/** One attributed line (domain.BlameLine). */
export interface BlameLine {
  line: number;
  text: string;
  snapshot_id: string;
  author: string;
  /** First line of the snapshot message. */
  message: string;
  at: ISODateString;
}

export interface BlameResult {
  lines: BlameLine[];
  /** The resolved snapshot id the blame was computed at. */
  snapshot: string;
}

export interface MergeConflict {
  path: string;
  kind: string;
}

/** GET {tree}/merge-requests/{n}/mergeability. */
export interface Mergeability {
  fast_forward: boolean;
  /** Methods both allowed by the protections and possible. */
  methods: MergeMethod[];
  conflicts: MergeConflict[];
}

export interface MergeOptions {
  method?: MergeMethod;
  message?: string;
  /** v0.8 conflict-ui (§16.9): delete the source branch after the merge. */
  delete_source_branch?: boolean;
}

/** A stored policy revision (domain.PolicyVersion). */
export interface PolicyVersion {
  tenant_id: string;
  policy_id: string;
  version: number;
  change: "create" | "update" | "delete" | "rollback" | (string & {});
  policy: Policy;
  changed_by: string;
  changed_at: ISODateString;
  comment?: string;
}

export interface PolicyDiffChange {
  op: "add" | "remove" | "change" | (string & {});
  sid: string;
  statement?: PolicyStatement;
  before?: PolicyStatement;
  after?: PolicyStatement;
}

export interface PolicyDiff {
  from: number;
  to: number;
  changes: PolicyDiffChange[];
}

/** StagedSnapshot with the v0.5 working-manifest hash. */
export interface StagedSnapshotV06 extends StagedSnapshot {
  manifest_hash?: string;
}

/** GET {tree}/staged/{snapshot}/files. */
export interface StagedFiles {
  staged: StagedSnapshotV06;
  files: SnapshotFile[];
  restricted_count: number;
}

// ---- helpers ----

function arr<T>(v: T[] | null | undefined): T[] {
  return Array.isArray(v) ? v : [];
}

/** Lists may come bare or wrapped (`{"webhooks":[...]}`); both are accepted. */
function listOf<T>(v: unknown, key: string): T[] {
  if (Array.isArray(v)) return v as T[];
  if (v && typeof v === "object") {
    const inner = (v as Record<string, unknown>)[key] ?? (v as Record<string, unknown>).items;
    if (Array.isArray(inner)) return inner as T[];
  }
  return [];
}

/**
 * True when the feature's backend is not deployed on this server: `501
 * not_implemented`, the API catch-all's `404 no such endpoint` or a `405`.
 */
export function isUnavailable(err: unknown): boolean {
  if (!isApiError(err)) return false;
  if (err.status === 501 || err.code === "not_implemented") return true;
  if (err.status === 405 || err.code === "method_not_allowed") return true;
  return err.status === 404 && /no such endpoint/i.test(err.message);
}

/** Query retry policy: never retry 4xx or unavailable, otherwise twice. */
export function retryV06(count: number, err: unknown): boolean {
  if (isUnavailable(err)) return false;
  if (isApiError(err) && err.status >= 400 && err.status < 500) return false;
  return count < 2;
}

function normWebhook<T extends Webhook>(w: T): T {
  return { ...w, events: arr(w.events), failure_count: w.failure_count ?? 0, has_secret: Boolean(w.has_secret) };
}

function normDelivery(d: WebhookDelivery): WebhookDelivery {
  return { ...d, attempts: d.attempts ?? 0, status_code: d.status_code ?? 0, duration_ms: d.duration_ms ?? 0, succeeded: Boolean(d.succeeded) };
}

function normHit(h: CodeSearchHit): CodeSearchHit {
  return { ...h, before: arr(h.before), after: arr(h.after) };
}

function normStatement(s: PolicyStatement): PolicyStatement {
  return { ...s, actions: arr(s.actions), resources: arr(s.resources), principals: arr(s.principals), conditions: s.conditions ?? {} };
}

function normPolicyVersion(v: PolicyVersion): PolicyVersion {
  const p = v.policy ?? ({} as Policy);
  return { ...v, policy: { ...p, statements: arr(p.statements).map(normStatement) } };
}

function normMergeability(m: Partial<Mergeability> | null | undefined): Mergeability {
  return {
    fast_forward: Boolean(m?.fast_forward),
    methods: arr(m?.methods).filter(isMergeMethod),
    conflicts: arr(m?.conflicts),
  };
}

function normPlan(r: AccessPlanResult): AccessPlanResult {
  return { accepted: Boolean(r.accepted), errors: arr(r.errors), changes: arr(r.changes) };
}

/** Snapshot id from a blame response's `snapshot` (an id, or a Snapshot object). */
function snapshotIdOf(v: unknown): string {
  if (typeof v === "string") return v;
  if (v && typeof v === "object" && typeof (v as { id?: unknown }).id === "string") return (v as { id: string }).id;
  return "";
}

/** Conflicts carried by a `409 merge_conflict` error, or null for other errors. */
export function mergeConflictsOf(err: unknown): MergeConflict[] | null {
  if (!isApiError(err) || err.code !== "merge_conflict") return null;
  return arr(err.details.conflicts as MergeConflict[] | null).filter((c) => c && typeof c.path === "string");
}

/** Default merge message per §14.1. */
export function defaultMergeMessage(method: MergeMethod, mr: Pick<MergeRequest, "source_branch" | "target_branch" | "number" | "title">): string {
  if (method === "squash") return mr.title;
  return `Merge branch '${mr.source_branch}' into ${mr.target_branch} (!${mr.number})`;
}

// ---- endpoints ----

export function createV06Api(client: ApiClient) {
  const tenant = (t: string) => `${V1}/tenants/${seg(t)}`;
  const tree = (t: string, n: string) => `${V1}/trees/${seg(t)}/${seg(n)}`;
  const hooks = (s: WebhookScope) => `${s.tree ? tree(s.tenant, s.tree) : tenant(s.tenant)}/webhooks`;
  const hook = (s: WebhookScope, id: string) => `${hooks(s)}/${seg(id)}`;

  return {
    webhooks: {
      list: async (s: WebhookScope) => listOf<Webhook>(await client.request<unknown>(hooks(s)), "webhooks").map(normWebhook),
      get: async (s: WebhookScope, id: string) => normWebhook(await client.request<Webhook>(hook(s, id))),
      create: async (s: WebhookScope, body: CreateWebhookRequest) =>
        normWebhook(await client.request<CreatedWebhook>(hooks(s), { method: "POST", body })),
      update: async (s: WebhookScope, id: string, body: UpdateWebhookRequest) =>
        normWebhook(await client.request<Webhook>(hook(s, id), { method: "PATCH", body })),
      remove: (s: WebhookScope, id: string) => client.request<void>(hook(s, id), { method: "DELETE" }),
      deliveries: async (s: WebhookScope, id: string, limit = 50) =>
        listOf<WebhookDelivery>(await client.request<unknown>(`${hook(s, id)}/deliveries`, { query: { limit } }), "deliveries").map(
          normDelivery,
        ),
      delivery: async (s: WebhookScope, id: string, did: string) =>
        normDelivery(await client.request<WebhookDelivery>(`${hook(s, id)}/deliveries/${seg(did)}`)),
      redeliver: async (s: WebhookScope, id: string, did: string) => {
        const d = await client.request<WebhookDelivery | undefined>(`${hook(s, id)}/deliveries/${seg(did)}/redeliver`, { method: "POST" });
        return d ? normDelivery(d) : undefined;
      },
      test: async (s: WebhookScope, id: string) => {
        const d = await client.request<WebhookDelivery | undefined>(`${hook(s, id)}/test`, { method: "POST" });
        return d && typeof d === "object" && "id" in d ? normDelivery(d) : undefined;
      },
    },

    codeSearch: async (q: CodeSearchQuery): Promise<CodeSearchResult> => {
      const r = await client.request<Partial<CodeSearchResult> | null>(`${V1}/search/code`, {
        query: {
          q: q.q,
          tenant: q.tenant,
          tree: q.tree,
          path: q.path,
          regex: q.regex ? "true" : undefined,
          case: q.case ? "true" : undefined,
          context: q.context,
          limit: q.limit,
        },
      });
      return { hits: arr(r?.hits).map(normHit), truncated: Boolean(r?.truncated), searched_trees: r?.searched_trees ?? 0 };
    },

    blame: async (t: string, n: string, rev: string, path: string): Promise<BlameResult> => {
      const r = await client.request<{ lines?: BlameLine[] | null; snapshot?: unknown } | null>(
        `${tree(t, n)}/blame/${seg(rev)}/${encodePath(path)}`,
      );
      return { lines: arr(r?.lines), snapshot: snapshotIdOf(r?.snapshot) || rev };
    },

    mergeRequests: {
      mergeability: async (t: string, n: string, num: number) =>
        normMergeability(await client.request<Mergeability>(`${tree(t, n)}/merge-requests/${num}/mergeability`)),
      /** POST .../merge; without options the body is omitted (pre-v0.6 servers fast-forward). */
      merge: (t: string, n: string, num: number, opts?: MergeOptions) => {
        const body: MergeOptions = {};
        if (opts?.method) body.method = opts.method;
        if (opts?.message?.trim()) body.message = opts.message.trim();
        // v0.8 conflict-ui (§16.9)
        if (opts?.delete_source_branch) body.delete_source_branch = true;
        return client.request<MergeRequest>(`${tree(t, n)}/merge-requests/${num}/merge`, {
          method: "POST",
          body: Object.keys(body).length ? body : undefined,
        });
      },
    },

    policyHistory: {
      versions: async (t: string, id: string) =>
        listOf<PolicyVersion>(await client.request<unknown>(`${tenant(t)}/policies/${seg(id)}/versions`), "versions")
          .map(normPolicyVersion)
          .sort((a, b) => b.version - a.version),
      version: async (t: string, id: string, v: number) =>
        normPolicyVersion(await client.request<PolicyVersion>(`${tenant(t)}/policies/${seg(id)}/versions/${v}`)),
      diff: async (t: string, id: string, from: number, to: number): Promise<PolicyDiff> => {
        const d = await client.request<Partial<PolicyDiff> | null>(`${tenant(t)}/policies/${seg(id)}/diff`, { query: { from, to } });
        return { from: d?.from ?? from, to: d?.to ?? to, changes: arr(d?.changes) };
      },
      rollback: (t: string, id: string, version: number, comment: string, force = false) =>
        client.request<Policy>(`${tenant(t)}/policies/${seg(id)}/rollback`, {
          method: "POST",
          query: { force: force ? "true" : undefined },
          body: comment.trim() ? { version, comment: comment.trim() } : { version },
        }),
      deleted: async (t: string) => listOf<Policy>(await client.request<unknown>(`${tenant(t)}/policies/deleted`), "policies"),
    },

    staged: {
      files: async (t: string, n: string, snapshot: string): Promise<StagedFiles> => {
        const r = await client.request<StagedFiles>(`${tree(t, n)}/staged/${seg(snapshot)}/files`);
        return { staged: r.staged, files: arr(r.files), restricted_count: r.restricted_count ?? 0 };
      },
      file: async (t: string, n: string, snapshot: string, path: string, signal?: AbortSignal) => {
        const res = await client.raw(`${tree(t, n)}/staged/${seg(snapshot)}/files/${encodePath(path)}`, { signal });
        return res.arrayBuffer();
      },
    },

    treeAccessConfig: {
      /** The last applied tree document, or null when none was applied. */
      get: async (t: string, n: string): Promise<AccessConfigDoc | null> => {
        try {
          return await client.request<AccessConfigDoc>(`${tree(t, n)}/access-config`);
        } catch (err) {
          if (isApiError(err) && err.status === 404 && !isUnavailable(err)) return null;
          throw err;
        }
      },
      plan: async (t: string, n: string, content: unknown) =>
        normPlan(await client.request<AccessPlanResult>(`${tree(t, n)}/access-config/plan`, { method: "POST", body: { content } })),
      apply: async (t: string, n: string, content: unknown) =>
        normPlan(await client.request<AccessPlanResult>(`${tree(t, n)}/access-config/apply`, { method: "POST", body: { content } })),
    },
  };
}

export type V06Api = ReturnType<typeof createV06Api>;

// ---- query keys + hooks ----

export const v06Keys = {
  webhooks: (s: WebhookScope) => (s.tree ? (["tree", s.tenant, s.tree, "webhooks"] as const) : (["tenant", s.tenant, "webhooks"] as const)),
  deliveries: (s: WebhookScope, id: string) => [...v06Keys.webhooks(s), id, "deliveries"] as const,
  delivery: (s: WebhookScope, id: string, did: string) => [...v06Keys.webhooks(s), id, "deliveries", did] as const,
  codeSearch: (q: CodeSearchQuery) => ["search", "code", q] as const,
  blame: (t: string, n: string, rev: string, path: string) => ["tree", t, n, "blame", rev, path] as const,
  mergeability: (t: string, n: string, num: number) => ["tree", t, n, "mergeability", num] as const,
  policyVersions: (t: string, id: string) => ["tenant", t, "iam", "policies", id, "versions"] as const,
  policyDiff: (t: string, id: string, from: number, to: number) => ["tenant", t, "iam", "policies", id, "diff", from, to] as const,
  deletedPolicies: (t: string) => ["tenant", t, "iam", "policies", "deleted"] as const,
  stagedFiles: (t: string, n: string, snapshot: string) => ["tree", t, n, "staged-files", snapshot] as const,
  stagedFile: (t: string, n: string, snapshot: string, path: string) => ["tree", t, n, "staged-files", snapshot, path] as const,
  treeAccessConfig: (t: string, n: string) => ["tree", t, n, "access-config"] as const,
};

/** The v0.6 API bound to the app's client. */
export function useV06Api(): V06Api {
  const { client } = useApi();
  return useMemo(() => createV06Api(client), [client]);
}

export function useWebhooks(scope: WebhookScope) {
  const api = useV06Api();
  return useQuery({ queryKey: v06Keys.webhooks(scope), queryFn: () => api.webhooks.list(scope), retry: retryV06 });
}

export function useWebhookDeliveries(scope: WebhookScope, id: string, enabled = true) {
  const api = useV06Api();
  return useQuery({
    queryKey: v06Keys.deliveries(scope, id),
    queryFn: () => api.webhooks.deliveries(scope, id),
    enabled: enabled && id !== "",
    retry: retryV06,
  });
}

export function useWebhookDelivery(scope: WebhookScope, id: string, did: string) {
  const api = useV06Api();
  return useQuery({
    queryKey: v06Keys.delivery(scope, id, did),
    queryFn: () => api.webhooks.delivery(scope, id, did),
    enabled: did !== "",
    retry: retryV06,
  });
}

/** Minimum literal query length the server accepts (§14.3). */
export const MIN_CODE_QUERY = 3;

export function useCodeSearch(q: CodeSearchQuery) {
  const api = useV06Api();
  const term = q.q.trim();
  const enabled = q.regex ? term.length > 0 : term.length >= MIN_CODE_QUERY;
  return useQuery({
    queryKey: v06Keys.codeSearch({ ...q, q: term }),
    queryFn: () => api.codeSearch({ ...q, q: term }),
    enabled,
    retry: retryV06,
    placeholderData: keepPreviousData,
    staleTime: 30_000,
  });
}

export function useBlame(t: string, n: string, rev: string, path: string, enabled = true) {
  const api = useV06Api();
  return useQuery({
    queryKey: v06Keys.blame(t, n, rev, path),
    queryFn: () => api.blame(t, n, rev, path),
    enabled: enabled && rev !== "" && path !== "",
    retry: retryV06,
    staleTime: Infinity,
  });
}

export function useMergeability(t: string, n: string, num: number, enabled = true) {
  const api = useV06Api();
  return useQuery({
    queryKey: v06Keys.mergeability(t, n, num),
    queryFn: () => api.mergeRequests.mergeability(t, n, num),
    enabled,
    retry: retryV06,
  });
}

export function usePolicyVersions(t: string, id: string, enabled = true) {
  const api = useV06Api();
  return useQuery({
    queryKey: v06Keys.policyVersions(t, id),
    queryFn: () => api.policyHistory.versions(t, id),
    enabled: enabled && id !== "",
    retry: retryV06,
  });
}

export function usePolicyDiff(t: string, id: string, from: number, to: number, enabled = true) {
  const api = useV06Api();
  return useQuery({
    queryKey: v06Keys.policyDiff(t, id, from, to),
    queryFn: () => api.policyHistory.diff(t, id, from, to),
    enabled: enabled && id !== "" && from > 0 && to > 0 && from !== to,
    retry: retryV06,
    staleTime: Infinity,
  });
}

export function useDeletedPolicies(t: string, enabled = true) {
  const api = useV06Api();
  return useQuery({ queryKey: v06Keys.deletedPolicies(t), queryFn: () => api.policyHistory.deleted(t), enabled, retry: retryV06 });
}

export function useStagedFiles(t: string, n: string, snapshot: string, enabled = true) {
  const api = useV06Api();
  return useQuery({
    queryKey: v06Keys.stagedFiles(t, n, snapshot),
    queryFn: () => api.staged.files(t, n, snapshot),
    enabled: enabled && snapshot !== "",
    retry: retryV06,
  });
}

export function useStagedFile(t: string, n: string, snapshot: string, path: string, enabled = true) {
  const api = useV06Api();
  return useQuery({
    queryKey: v06Keys.stagedFile(t, n, snapshot, path),
    queryFn: ({ signal }) => api.staged.file(t, n, snapshot, path, signal),
    enabled: enabled && snapshot !== "" && path !== "",
    retry: retryV06,
  });
}

export function useTreeAccessConfig(t: string, n: string) {
  const api = useV06Api();
  return useQuery({ queryKey: v06Keys.treeAccessConfig(t, n), queryFn: () => api.treeAccessConfig.get(t, n), retry: retryV06 });
}
