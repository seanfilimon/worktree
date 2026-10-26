/**
 * Fork endpoints (docs/ENGINEERING_PLAN.md §16.2, specs/tree/Forks.md) used
 * by ForksTab, the fork badge of the tree header and merge requests from a
 * fork.
 *
 * Field names mirror services/server-go/internal/fork (Fork, Upstream,
 * BranchStatus, SyncResult) and internal/review (MRSource). Go encodes
 * empty slices as `null`; the helpers below normalise lists to `[]`.
 */
import { useQuery } from "@tanstack/react-query";
import { useMemo } from "react";
import type { ApiClient } from "@/api/client";
import { useApi } from "@/api/context";
import type { ISODateString, MergeRequest, Tree, Visibility } from "@/api/types";
import { retryV06 } from "@/api/v06";

// ---- wire types ----

/** domain.Tree with the v0.8 fork field. */
export type ForkTree = Tree & { fork_of?: string };

/** domain.MergeRequest with the v0.8 fork field. */
export type ForkMergeRequest = MergeRequest & { source_tree_id?: string };

/** A fork of a tree (fork.Fork). */
export interface Fork {
  tree_id: string;
  tenant_id: string;
  /** Tenant slug of the fork. */
  tenant: string;
  name: string;
  description: string;
  visibility: Visibility;
  branches: string[];
  created_by: string;
  created_at: ISODateString;
  last_sync_at?: ISODateString;
}

export type BranchRelation = "up_to_date" | "ahead" | "behind" | "diverged" | "fork_only" | "upstream_only";

export interface ForkBranchStatus {
  name: string;
  /** Fork tip ("" = absent or empty). */
  tip: string;
  /** Upstream tip (an upstream snapshot id). */
  upstream_tip: string;
  status: BranchRelation;
}

/** The upstream of a tree (fork.Upstream). */
export interface Upstream {
  /** False for trees that are not forks. */
  fork: boolean;
  upstream_tree_id?: string;
  upstream_missing: boolean;
  upstream_hidden: boolean;
  tenant?: string;
  name?: string;
  visibility?: Visibility;
  default_branch?: string;
  forked_by?: string;
  forked_at?: ISODateString;
  last_sync_at?: ISODateString;
  branches: ForkBranchStatus[];
}

export type SyncOutcome = "fast_forwarded" | "created" | "up_to_date" | "ahead" | "diverged" | "upstream_only" | "moved";

export interface SyncBranch {
  name: string;
  result: SyncOutcome;
  old_tip?: string;
  new_tip?: string;
}

export interface SyncResult {
  branches: SyncBranch[];
  updated: number;
  diverged: string[];
}

export interface CreateForkRequest {
  tenant?: string;
  name?: string;
  description?: string;
  visibility?: Visibility;
  branches?: string[];
}

/** Where a merge request's source lives (review.MRSource). */
export interface MRSource {
  cross_tree: boolean;
  tree_id: string;
  tenant: string;
  tree: string;
  branch: string;
  tip: string;
  /** The source tip as a snapshot of the merge request's tree. */
  target_tip: string;
  missing: boolean;
}

export interface CreateForkMergeRequest {
  title: string;
  description: string;
  source_branch: string;
  target_branch: string;
  source_tenant: string;
  source_tree: string;
}

// ---- endpoints ----

const seg = encodeURIComponent;

function arr<T>(v: T[] | null | undefined): T[] {
  return Array.isArray(v) ? v : [];
}

const treeBase = (tenant: string, tree: string) => `/api/v1/trees/${seg(tenant)}/${seg(tree)}`;

export function createForksApi(client: ApiClient, tenant: string, tree: string) {
  const base = treeBase(tenant, tree);
  return {
    list: async (): Promise<Fork[]> => {
      const v = await client.request<Fork[] | null>(`${base}/forks`);
      return arr(v).map((f) => ({ ...f, branches: arr(f.branches) }));
    },
    create: (body: CreateForkRequest) => client.request<ForkTree>(`${base}/forks`, { method: "POST", body }),
    upstream: async (): Promise<Upstream> => {
      const v = await client.request<Upstream>(`${base}/fork`);
      return { ...v, fork: Boolean(v.fork), upstream_missing: Boolean(v.upstream_missing), upstream_hidden: Boolean(v.upstream_hidden), branches: arr(v.branches) };
    },
    sync: async (branches?: string[]): Promise<SyncResult> => {
      const v = await client.request<SyncResult>(`${base}/fork/sync`, { method: "POST", body: branches?.length ? { branches } : {} });
      return { branches: arr(v.branches), updated: v.updated ?? 0, diverged: arr(v.diverged) };
    },
    mrSource: (n: number) => client.request<MRSource>(`${base}/merge-requests/${n}/source`),
  };
}

/** POST {upstream}/merge-requests from a branch of a fork. */
export function createForkMergeRequest(client: ApiClient, upTenant: string, upTree: string, body: CreateForkMergeRequest) {
  return client.request<ForkMergeRequest>(`${treeBase(upTenant, upTree)}/merge-requests`, { method: "POST", body });
}

export type ForksApi = ReturnType<typeof createForksApi>;

export const forkKeys = {
  forks: (tenant: string, tree: string) => ["tree", tenant, tree, "forks"] as const,
  upstream: (tenant: string, tree: string) => ["tree", tenant, tree, "fork-upstream"] as const,
  mrSource: (tenant: string, tree: string, n: number) => ["tree", tenant, tree, "mr", n, "source"] as const,
};

export function useForksApi(tenant: string, tree: string) {
  const { client } = useApi();
  return useMemo(() => createForksApi(client, tenant, tree), [client, tenant, tree]);
}

/** GET {tree}/fork; enabled only when the tree is known to be a fork. */
export function useUpstream(tenant: string, tree: string, enabled = true) {
  const api = useForksApi(tenant, tree);
  return useQuery({ queryKey: forkKeys.upstream(tenant, tree), queryFn: api.upstream, enabled, retry: retryV06 });
}

/**
 * The source of a merge request as seen from its tree: for a merge request
 * from a fork, the fork's tip copied into this tree (`target_tip`), which
 * is what its diff compares. `cross` is false for same-tree requests (the
 * source endpoint is then not called).
 */
export function useForkSource(tenant: string, tree: string, mr: MergeRequest) {
  const cross = Boolean((mr as ForkMergeRequest).source_tree_id) && (mr as ForkMergeRequest).source_tree_id !== mr.tree_id;
  const api = useForksApi(tenant, tree);
  const q = useQuery({
    queryKey: forkKeys.mrSource(tenant, tree, mr.number),
    queryFn: () => api.mrSource(mr.number),
    enabled: cross,
    retry: retryV06,
  });
  const src = q.data;
  return {
    cross,
    pending: cross && q.isPending,
    source: src,
    /** The source branch as a branch of this tree (tip = target_tip); undefined when missing. */
    branch: src && !src.missing ? { name: src.branch, tip: src.target_tip } : undefined,
    /** "tenant/tree:branch" for display ("" for same-tree requests). */
    label: cross ? (src?.tenant && src.tree ? `${src.tenant}/${src.tree}:${mr.source_branch}` : `fork:${mr.source_branch}`) : "",
  };
}

// ---- helpers ----

const RANK: Record<Visibility, number> = { private: 0, internal: 1, public: 2 };

/** Visibilities a fork of a tree with visibility `v` may have (never wider). */
export function allowedVisibilities(v: Visibility): Visibility[] {
  return (["private", "internal", "public"] as const).filter((x) => RANK[x] <= RANK[v]);
}

const TREE_NAME_RE = /^[A-Za-z0-9][A-Za-z0-9._-]{0,99}$/;

/** Validation message for a fork name ("" = valid; empty = upstream name). */
export function forkNameError(name: string): string {
  const n = name.trim();
  if (!n) return "";
  if (!TREE_NAME_RE.test(n) || n.endsWith(".")) return "Use 1-100 letters, digits, '.', '_' or '-', starting with a letter or digit.";
  return "";
}

export const RELATION_LABEL: Record<BranchRelation, string> = {
  up_to_date: "Up to date",
  ahead: "Ahead",
  behind: "Behind",
  diverged: "Diverged",
  fork_only: "Only in fork",
  upstream_only: "Only upstream",
};

/** One-line summary of a sync for the toast. */
export function syncSummary(r: SyncResult): string {
  const updated = r.updated === 0 ? "Everything up to date" : `${r.updated} branch${r.updated === 1 ? "" : "es"} updated`;
  if (r.diverged.length === 0) return updated;
  return `${updated}; diverged: ${r.diverged.join(", ")} (merge the upstream locally)`;
}
