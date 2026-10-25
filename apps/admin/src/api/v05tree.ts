/**
 * v0.5 tree features (docs/ENGINEERING_PLAN.md §13.4–§13.6, §13.1, §13.12
 * `admin-tree`): TODOs, dependencies, releases and assets, CI status, review
 * decisions, MR comments, merge readiness, linked groups, tag protections,
 * plus the v0.5 fields on snapshots, protections and merge requests.
 *
 * Field names mirror services/server-go/internal/domain/{v05.go,types.go}.
 * Go encodes empty slices as `null` and omits `omitempty` fields; every
 * function here normalises lists to `[]`. Features whose service is not
 * configured answer `501 not_implemented` (see `isNotImplemented`).
 */
import { keepPreviousData, useQuery } from "@tanstack/react-query";
import { useMemo } from "react";
import { encodePath, isApiError, type ApiClient } from "./client";
import { useApi } from "./context";
import type { Approval, BranchProtection, ISODateString, MergeRequest, Snapshot, SnapshotFile } from "./types";

// ---- wire types ----

export const TODO_STATUSES = ["open", "in_progress", "done", "cancelled"] as const;
export type TodoStatus = (typeof TODO_STATUSES)[number];

export const TODO_PRIORITIES = ["low", "medium", "high", "critical"] as const;
export type TodoPriority = (typeof TODO_PRIORITIES)[number];

/** A dependency declared on a snapshot (domain.Dependency). */
export interface Dependency {
  /** "" = the tenant of the dependent tree. */
  tenant_slug?: string;
  tree: string;
  /** "" = default branch. */
  branch?: string;
  title: string;
  details?: string;
  /** "" = medium. */
  priority?: TodoPriority | "";
  blocking?: boolean;
  linked?: boolean;
}

/** Work requested from this tree by another tree's dependency (domain.Todo). */
export interface Todo {
  id: string;
  tenant_id: string;
  tree_id: string;
  number: number;
  target_branch: string;
  title: string;
  details?: string;
  priority: TodoPriority;
  blocking: boolean;
  linked: boolean;
  status: TodoStatus;
  source_tenant_id: string;
  source_tree_id: string;
  source_branch: string;
  source_snapshot_id: string;
  created_by: string;
  author: string;
  assignee_id?: string;
  assignee?: string;
  /** Snapshot that completed it. */
  resolved_by?: string;
  created_at: ISODateString;
  updated_at: ISODateString;
  closed_at?: ISODateString;
}

export interface TodoQuery {
  status?: TodoStatus | "";
  branch?: string;
  assignee?: string;
}

export interface CreateTodoRequest {
  title: string;
  details?: string;
  priority?: TodoPriority;
  blocking?: boolean;
  target_branch?: string;
}

export interface UpdateTodoRequest {
  title?: string;
  details?: string;
  priority?: TodoPriority;
  blocking?: boolean;
  status?: TodoStatus;
}

export interface OutgoingDependency {
  snapshot_id: string;
  dependency: Dependency;
  todo: { tenant: string; tree: string; number: number; status: TodoStatus } | null;
}

export interface DependencyStatus {
  outgoing: OutgoingDependency[];
  incoming: Todo[];
}

export interface ReleaseAsset {
  name: string;
  size: number;
  hash: string;
  content_type: string;
  uploaded_at: ISODateString;
}

export interface Release {
  id: string;
  tree_id: string;
  tag: string;
  snapshot_id: string;
  name: string;
  notes?: string;
  draft?: boolean;
  prerelease?: boolean;
  assets: ReleaseAsset[];
  created_by: string;
  author: string;
  created_at: ISODateString;
  published_at?: ISODateString;
}

export interface CreateReleaseRequest {
  tag: string;
  name: string;
  notes: string;
  draft: boolean;
  prerelease: boolean;
  /** Revision for a new tag; omit when the tag exists. */
  snapshot?: string;
}

export interface UpdateReleaseRequest {
  name?: string;
  notes?: string;
  draft?: boolean;
  prerelease?: boolean;
}

export const CI_STATES = ["pending", "success", "failure", "error"] as const;
export type CIState = (typeof CI_STATES)[number];

/** One check result for a snapshot (domain.CIStatus). */
export interface CIStatus {
  tree_id: string;
  snapshot_id: string;
  context: string;
  state: CIState;
  description?: string;
  target_url?: string;
  reporter: string;
  updated_at: ISODateString;
}

export interface CIStatusResult {
  snapshot: string;
  /** Combined state; "" when nothing was reported. */
  state: CIState | "";
  checks: CIStatus[];
}

export const REVIEW_DECISIONS = ["approved", "changes_requested", "commented"] as const;
export type ReviewDecision = (typeof REVIEW_DECISIONS)[number];

/** domain.Approval with the v0.5 review fields. */
export interface ReviewApproval extends Approval {
  /** Source tip at review time ("" = recorded before v0.5: counts). */
  snapshot_id?: string;
  /** "" = approved. */
  decision?: ReviewDecision | "";
  body?: string;
}

/** domain.MergeRequest with the v0.5 fields. */
export type MergeRequestV05 = Omit<MergeRequest, "approvals"> & {
  approvals: ReviewApproval[] | null;
  linked_group?: string;
};

export interface ReviewRequest {
  decision: ReviewDecision;
  body?: string;
}

export interface ReviewComment {
  id: string;
  tree_id: string;
  mr_number: number;
  author_id: string;
  author: string;
  body: string;
  path?: string;
  line?: number;
  snapshot_id?: string;
  resolved?: boolean;
  created_at: ISODateString;
  updated_at: ISODateString;
}

export interface CreateCommentRequest {
  body: string;
  path?: string;
  line?: number;
  snapshot_id?: string;
}

export interface UpdateCommentRequest {
  body?: string;
  resolved?: boolean;
}

export interface MergeBlocker {
  kind: string;
  message: string;
}

/** GET {tree}/merge-requests/{n}/checks (§13.4). */
export interface MergeChecks {
  mergeable: boolean;
  blockers: MergeBlocker[];
  approvals: { required: number; current: number; stale: number };
  ci: { state: CIState | ""; required: string[]; missing: string[] };
  todos: { blocking_open: number };
}

export interface LinkedGroupMember {
  tree_id: string;
  mr_number: number;
}

export interface LinkedGroup {
  id: string;
  tenant_id: string;
  name: string;
  members: LinkedGroupMember[];
  state: "open" | "merged" | "closed";
  created_by: string;
  created_at: ISODateString;
  merged_at?: ISODateString;
}

export interface TagProtection {
  tree_id: string;
  /** path.Match glob, e.g. "v*". */
  pattern: string;
  no_delete: boolean;
  no_move: boolean;
  /** Usernames; empty = anyone with tree:write. */
  allowed_taggers?: string[] | null;
}

/** domain.BranchProtection with the v0.5 rule fields (all omitempty). */
export interface BranchProtectionV05 extends BranchProtection {
  require_ci_pass?: boolean;
  /** CI contexts; empty with require_ci_pass = every reported check. */
  required_checks?: string[] | null;
  require_signed?: boolean;
  require_linear?: boolean;
  block_on_todos?: boolean;
}

/** domain.Snapshot with the v0.5 fields. */
export interface SnapshotV05 extends Snapshot {
  dependencies?: Dependency[] | null;
  /** Id of the snapshot this one reverts. */
  reverts?: string;
  /** Base64 Ed25519 signature. */
  signature?: string;
  signing_key_id?: string;
  /** Set by the server when the signature verified at push time. */
  verified?: boolean;
}

/** GET {tree}/snapshots/{id} with the §13.1 restricted-path count. */
export interface SnapshotDetailV05 {
  snapshot: SnapshotV05;
  files: SnapshotFile[] | null;
  /** Paths omitted because the caller may not read them. */
  restricted_count?: number;
}

// ---- helpers ----

/** The server has no service for this feature (`501 not_implemented`). */
export function isNotImplemented(err: unknown): boolean {
  return isApiError(err) && (err.status === 501 || err.code === "not_implemented");
}

/**
 * For optional panels backed by v0.5 endpoints: the endpoint is missing
 * (older server: 404 for the route) or not configured (501).
 */
export function isUnavailable(err: unknown): boolean {
  return isNotImplemented(err) || (isApiError(err) && err.status === 404);
}

/** The v0.5 view of a merge request returned by the v0.2 endpoints. */
export function asV05(mr: MergeRequest): MergeRequestV05 {
  return mr;
}

export type ReviewState = "approved" | "stale" | "changes_requested" | "commented";

/**
 * Classifies a recorded review against the current source tip: approvals
 * recorded for an older tip are stale and no longer count.
 */
export function reviewState(a: ReviewApproval, sourceTip: string | undefined): ReviewState {
  const decision = a.decision || "approved";
  if (decision !== "approved") return decision;
  if (a.snapshot_id && sourceTip && a.snapshot_id !== sourceTip) return "stale";
  return "approved";
}

/** Latest review per account (newest `at` wins), in first-seen order. */
export function latestReviews(approvals: readonly ReviewApproval[] | null | undefined): ReviewApproval[] {
  const byAccount = new Map<string, ReviewApproval>();
  for (const a of approvals ?? []) {
    const prev = byAccount.get(a.account_id);
    if (!prev || Date.parse(a.at) >= Date.parse(prev.at)) byAccount.set(a.account_id, a);
  }
  return [...byAccount.values()];
}

/** Combined CI state: failure if any failure/error, else pending if any pending, else success. */
export function combinedState(checks: readonly CIStatus[]): CIState | "" {
  if (checks.length === 0) return "";
  if (checks.some((c) => c.state === "failure" || c.state === "error")) return "failure";
  if (checks.some((c) => c.state === "pending")) return "pending";
  return "success";
}

const V1 = "/api/v1";
const seg = encodeURIComponent;
const arr = <T>(v: T[] | null | undefined): T[] => v ?? [];

/** Accepts `[T]` or `{"<field>": [T]}` list responses. */
function listOf<T>(v: unknown, field: string): T[] {
  if (Array.isArray(v)) return v as T[];
  if (v && typeof v === "object") {
    const inner = (v as Record<string, unknown>)[field];
    if (Array.isArray(inner)) return inner as T[];
  }
  return [];
}

function normRelease(r: Release): Release {
  return { ...r, assets: arr(r.assets) };
}

function normTagProtection(p: TagProtection): TagProtection {
  return { ...p, allowed_taggers: arr(p.allowed_taggers) };
}

function normChecks(c: MergeChecks): MergeChecks {
  return {
    mergeable: c.mergeable,
    blockers: arr(c.blockers),
    approvals: c.approvals ?? { required: 0, current: 0, stale: 0 },
    ci: { state: c.ci?.state ?? "", required: arr(c.ci?.required), missing: arr(c.ci?.missing) },
    todos: c.todos ?? { blocking_open: 0 },
  };
}

/** v0.5 tree endpoints bound to a client. */
export function createTreeV05Api(client: ApiClient) {
  const tree = (tenant: string, name: string) => `${V1}/trees/${seg(tenant)}/${seg(name)}`;
  const mr = (tenant: string, name: string, n: number) => `${tree(tenant, name)}/merge-requests/${n}`;
  const todo = (tenant: string, name: string, n: number) => `${tree(tenant, name)}/todos/${n}`;
  const release = (tenant: string, name: string, tag: string) => `${tree(tenant, name)}/releases/${seg(tag)}`;
  const asset = (tenant: string, name: string, tag: string, file: string) =>
    `${release(tenant, name, tag)}/assets/${encodePath(file)}`;
  const groups = (tenant: string) => `${V1}/tenants/${seg(tenant)}/linked-groups`;

  return {
    todos: {
      list: async (tenant: string, name: string, q: TodoQuery = {}) =>
        listOf<Todo>(await client.request<unknown>(`${tree(tenant, name)}/todos`, { query: { ...q } }), "todos"),
      get: (tenant: string, name: string, n: number) => client.request<Todo>(todo(tenant, name, n)),
      create: (tenant: string, name: string, body: CreateTodoRequest) =>
        client.request<Todo>(`${tree(tenant, name)}/todos`, { method: "POST", body }),
      update: (tenant: string, name: string, n: number, body: UpdateTodoRequest) =>
        client.request<Todo>(todo(tenant, name, n), { method: "PATCH", body }),
      claim: (tenant: string, name: string, n: number) =>
        client.request<Todo>(`${todo(tenant, name, n)}/claim`, { method: "POST" }),
      complete: (tenant: string, name: string, n: number, snapshot?: string) =>
        client.request<Todo>(`${todo(tenant, name, n)}/complete`, { method: "POST", body: snapshot ? { snapshot } : {} }),
      cancel: (tenant: string, name: string, n: number) =>
        client.request<Todo>(`${todo(tenant, name, n)}/cancel`, { method: "POST" }),
    },

    dependencies: async (tenant: string, name: string, branch?: string): Promise<DependencyStatus> => {
      const d = await client.request<Partial<DependencyStatus> | null>(`${tree(tenant, name)}/dependencies`, { query: { branch } });
      return { outgoing: arr(d?.outgoing), incoming: arr(d?.incoming) };
    },

    releases: {
      list: async (tenant: string, name: string) =>
        listOf<Release>(await client.request<unknown>(`${tree(tenant, name)}/releases`), "releases").map(normRelease),
      get: async (tenant: string, name: string, tag: string) => normRelease(await client.request<Release>(release(tenant, name, tag))),
      create: async (tenant: string, name: string, body: CreateReleaseRequest) =>
        normRelease(await client.request<Release>(`${tree(tenant, name)}/releases`, { method: "POST", body })),
      update: async (tenant: string, name: string, tag: string, body: UpdateReleaseRequest) =>
        normRelease(await client.request<Release>(release(tenant, name, tag), { method: "PATCH", body })),
      remove: (tenant: string, name: string, tag: string) => client.request<void>(release(tenant, name, tag), { method: "DELETE" }),
      uploadAsset: (tenant: string, name: string, tag: string, file: Blob, fileName: string) =>
        client.request<unknown>(asset(tenant, name, tag, fileName), {
          method: "PUT",
          rawBody: file,
          contentType: file.type || "application/octet-stream",
        }),
      /** Asset bytes (authenticated, so not a plain link). */
      downloadAsset: async (tenant: string, name: string, tag: string, fileName: string) =>
        (await client.raw(asset(tenant, name, tag, fileName))).blob(),
      removeAsset: (tenant: string, name: string, tag: string, fileName: string) =>
        client.request<void>(asset(tenant, name, tag, fileName), { method: "DELETE" }),
    },

    ci: {
      status: async (tenant: string, name: string, q: { snapshot?: string; branch?: string }): Promise<CIStatusResult> => {
        const r = await client.request<Partial<CIStatusResult> | null>(`${tree(tenant, name)}/ci/status`, { query: { ...q } });
        const checks = arr(r?.checks);
        return { snapshot: r?.snapshot ?? q.snapshot ?? "", state: r?.state ?? combinedState(checks), checks };
      },
    },

    reviews: {
      review: (tenant: string, name: string, n: number, body: ReviewRequest) =>
        client.request<MergeRequestV05>(`${mr(tenant, name, n)}/review`, { method: "POST", body }),
      comments: async (tenant: string, name: string, n: number) =>
        listOf<ReviewComment>(await client.request<unknown>(`${mr(tenant, name, n)}/comments`), "comments"),
      addComment: (tenant: string, name: string, n: number, body: CreateCommentRequest) =>
        client.request<ReviewComment>(`${mr(tenant, name, n)}/comments`, { method: "POST", body }),
      updateComment: (tenant: string, name: string, n: number, id: string, body: UpdateCommentRequest) =>
        client.request<ReviewComment>(`${mr(tenant, name, n)}/comments/${seg(id)}`, { method: "PATCH", body }),
      removeComment: (tenant: string, name: string, n: number, id: string) =>
        client.request<void>(`${mr(tenant, name, n)}/comments/${seg(id)}`, { method: "DELETE" }),
      checks: async (tenant: string, name: string, n: number) =>
        normChecks(await client.request<MergeChecks>(`${mr(tenant, name, n)}/checks`)),
    },

    linkedGroups: {
      list: async (tenant: string) => listOf<LinkedGroup>(await client.request<unknown>(groups(tenant)), "groups"),
      get: async (tenant: string, id: string) => {
        const g = await client.request<LinkedGroup>(`${groups(tenant)}/${seg(id)}`);
        return { ...g, members: arr(g.members) };
      },
      merge: (tenant: string, id: string) => client.request<unknown>(`${groups(tenant)}/${seg(id)}/merge`, { method: "POST" }),
    },

    tagProtections: {
      list: async (tenant: string, name: string) =>
        listOf<TagProtection>(await client.request<unknown>(`${tree(tenant, name)}/tag-protections`), "tag_protections").map(
          normTagProtection,
        ),
      put: (tenant: string, name: string, body: TagProtection) =>
        client.request<TagProtection>(`${tree(tenant, name)}/tag-protections`, { method: "PUT", body }),
      remove: (tenant: string, name: string, pattern: string) =>
        client.request<void>(`${tree(tenant, name)}/tag-protections`, { method: "DELETE", query: { pattern } }),
    },
  };
}

export type TreeV05Api = ReturnType<typeof createTreeV05Api>;

// ---- query keys + hooks ----

/**
 * Keys live under ["tree", tenant, tree, <suffix>] so the tree layout's live
 * invalidation (by suffix) and "refresh the whole tree" both reach them.
 */
export const v05Keys = {
  todos: (t: string, n: string, q: TodoQuery) => ["tree", t, n, "todos", q] as const,
  dependencies: (t: string, n: string, branch: string) => ["tree", t, n, "dependencies", branch] as const,
  releases: (t: string, n: string) => ["tree", t, n, "releases"] as const,
  ciStatus: (t: string, n: string, ref: string) => ["tree", t, n, "ci", ref] as const,
  mrComments: (t: string, n: string, num: number) => ["tree", t, n, "mr-comments", num] as const,
  mrChecks: (t: string, n: string, num: number) => ["tree", t, n, "mr-checks", num] as const,
  tagProtections: (t: string, n: string) => ["tree", t, n, "tag-protections"] as const,
  linkedGroups: (t: string) => ["tenant", t, "linked-groups"] as const,
  linkedGroup: (t: string, id: string) => ["tenant", t, "linked-groups", id] as const,
};

/** The v0.5 tree API bound to the app's client. */
export function useTreeV05Api(): TreeV05Api {
  const { client } = useApi();
  return useMemo(() => createTreeV05Api(client), [client]);
}

export function useTodos(tenant: string, tree: string, q: TodoQuery) {
  const api = useTreeV05Api();
  return useQuery({
    queryKey: v05Keys.todos(tenant, tree, q),
    queryFn: () => api.todos.list(tenant, tree, q),
    placeholderData: keepPreviousData,
  });
}

export function useDependencies(tenant: string, tree: string, branch: string, enabled = true) {
  const api = useTreeV05Api();
  return useQuery({
    queryKey: v05Keys.dependencies(tenant, tree, branch),
    queryFn: () => api.dependencies(tenant, tree, branch || undefined),
    enabled,
  });
}

export function useReleases(tenant: string, tree: string) {
  const api = useTreeV05Api();
  return useQuery({ queryKey: v05Keys.releases(tenant, tree), queryFn: () => api.releases.list(tenant, tree) });
}

/** CI status of a snapshot (preferred) or a branch tip. */
export function useCIStatus(tenant: string, tree: string, ref: { snapshot?: string; branch?: string }) {
  const api = useTreeV05Api();
  const key = ref.snapshot ? `snapshot:${ref.snapshot}` : ref.branch ? `branch:${ref.branch}` : "";
  return useQuery({
    queryKey: v05Keys.ciStatus(tenant, tree, key),
    queryFn: () => api.ci.status(tenant, tree, ref.snapshot ? { snapshot: ref.snapshot } : { branch: ref.branch }),
    enabled: key !== "",
  });
}

export function useMrComments(tenant: string, tree: string, n: number) {
  const api = useTreeV05Api();
  return useQuery({ queryKey: v05Keys.mrComments(tenant, tree, n), queryFn: () => api.reviews.comments(tenant, tree, n) });
}

export function useMrChecks(tenant: string, tree: string, n: number, enabled = true) {
  const api = useTreeV05Api();
  return useQuery({ queryKey: v05Keys.mrChecks(tenant, tree, n), queryFn: () => api.reviews.checks(tenant, tree, n), enabled });
}

export function useTagProtections(tenant: string, tree: string) {
  const api = useTreeV05Api();
  return useQuery({ queryKey: v05Keys.tagProtections(tenant, tree), queryFn: () => api.tagProtections.list(tenant, tree) });
}

export function useLinkedGroup(tenant: string, id: string) {
  const api = useTreeV05Api();
  return useQuery({
    queryKey: v05Keys.linkedGroup(tenant, id),
    queryFn: () => api.linkedGroups.get(tenant, id),
    enabled: id !== "",
  });
}
