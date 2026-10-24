import { keepPreviousData, useQuery } from "@tanstack/react-query";
import { useApi } from "./context";
import type { AuditQuery, MergeRequestState } from "./types";

/** Query-key factory; prefixes allow broad invalidation (e.g. a whole tree). */
export const keys = {
  health: ["health"] as const,
  me: ["me"] as const,
  tokens: ["tokens"] as const,
  stats: ["admin", "stats"] as const,
  accounts: ["admin", "accounts"] as const,
  audit: (q: AuditQuery) => ["audit", q] as const,
  tenants: ["tenants"] as const,
  tenant: (t: string) => ["tenant", t] as const,
  members: (t: string) => ["tenant", t, "members"] as const,
  tenantTrees: (t: string) => ["tenant", t, "trees"] as const,
  tree: (t: string, n: string) => ["tree", t, n] as const,
  branches: (t: string, n: string) => ["tree", t, n, "branches"] as const,
  snapshots: (t: string, n: string, branch: string) => ["tree", t, n, "snapshots", branch] as const,
  snapshot: (t: string, n: string, id: string) => ["tree", t, n, "snapshot", id] as const,
  blob: (t: string, n: string, snap: string, path: string) => ["tree", t, n, "blob", snap, path] as const,
  tags: (t: string, n: string) => ["tree", t, n, "tags"] as const,
  staged: (t: string, n: string, branch: string) => ["tree", t, n, "staged", branch] as const,
  protections: (t: string, n: string) => ["tree", t, n, "protections"] as const,
  licenses: (t: string, n: string) => ["tree", t, n, "licenses"] as const,
  mergeRequests: (t: string, n: string, state: string) => ["tree", t, n, "mrs", state] as const,
  mergeRequest: (t: string, n: string, num: number) => ["tree", t, n, "mr", num] as const,
  compare: (t: string, n: string, from: string, to: string) => ["tree", t, n, "compare", from, to] as const,
  fileHistory: (t: string, n: string, branch: string, path: string) =>
    ["tree", t, n, "file-history", branch, path] as const,
  treeStats: (t: string, n: string, branch: string, days: number) => ["tree", t, n, "stats", branch, days] as const,
  stagedSummary: (t: string, n: string) => ["tree", t, n, "staged-summary"] as const,
  recentEvents: (q: { tenant?: string; tree?: string; limit?: number }) => ["events", "recent", q] as const,
  search: (q: string) => ["search", q] as const,
  system: ["admin", "system"] as const,
};

export function useHealth() {
  const api = useApi();
  return useQuery({ queryKey: keys.health, queryFn: api.system.health, staleTime: 60_000 });
}

export function useStats(enabled: boolean) {
  const api = useApi();
  return useQuery({ queryKey: keys.stats, queryFn: api.admin.stats, enabled, refetchInterval: 30_000 });
}

export function useAccounts() {
  const api = useApi();
  return useQuery({ queryKey: keys.accounts, queryFn: api.admin.accounts });
}

export function useTokens() {
  const api = useApi();
  return useQuery({ queryKey: keys.tokens, queryFn: api.auth.listTokens });
}

export function useAudit(q: AuditQuery, enabled = true) {
  const api = useApi();
  return useQuery({
    queryKey: keys.audit(q),
    queryFn: () => api.audit.list(q),
    enabled,
    placeholderData: keepPreviousData,
  });
}

export function useTenants() {
  const api = useApi();
  return useQuery({ queryKey: keys.tenants, queryFn: api.tenants.list });
}

export function useTenant(tenant: string) {
  const api = useApi();
  return useQuery({ queryKey: keys.tenant(tenant), queryFn: () => api.tenants.get(tenant) });
}

export function useMembers(tenant: string) {
  const api = useApi();
  return useQuery({ queryKey: keys.members(tenant), queryFn: () => api.tenants.members(tenant) });
}

export function useTenantTrees(tenant: string) {
  const api = useApi();
  return useQuery({ queryKey: keys.tenantTrees(tenant), queryFn: () => api.tenants.trees(tenant) });
}

export function useTree(tenant: string, tree: string) {
  const api = useApi();
  return useQuery({ queryKey: keys.tree(tenant, tree), queryFn: () => api.trees.get(tenant, tree) });
}

export function useBranches(tenant: string, tree: string) {
  const api = useApi();
  return useQuery({ queryKey: keys.branches(tenant, tree), queryFn: () => api.trees.branches(tenant, tree) });
}

export function useSnapshots(tenant: string, tree: string, branch: string) {
  const api = useApi();
  return useQuery({
    queryKey: keys.snapshots(tenant, tree, branch),
    queryFn: () => api.trees.snapshots(tenant, tree, { branch, limit: 50 }),
    enabled: branch !== "",
  });
}

export function useSnapshot(tenant: string, tree: string, id: string) {
  const api = useApi();
  return useQuery({
    queryKey: keys.snapshot(tenant, tree, id),
    queryFn: () => api.trees.snapshot(tenant, tree, id),
    enabled: id !== "",
    // Snapshots are immutable.
    staleTime: Infinity,
  });
}

export function useBlob(tenant: string, tree: string, snapshot: string, path: string, enabled: boolean) {
  const api = useApi();
  return useQuery({
    queryKey: keys.blob(tenant, tree, snapshot, path),
    queryFn: ({ signal }) => api.trees.blob(tenant, tree, snapshot, path, signal),
    enabled,
    staleTime: Infinity,
  });
}

export function useTags(tenant: string, tree: string) {
  const api = useApi();
  return useQuery({ queryKey: keys.tags(tenant, tree), queryFn: () => api.trees.tags(tenant, tree) });
}

export function useStaged(tenant: string, tree: string, branch: string) {
  const api = useApi();
  return useQuery({
    queryKey: keys.staged(tenant, tree, branch),
    queryFn: () => api.trees.staged(tenant, tree, branch || undefined),
  });
}

export function useProtections(tenant: string, tree: string) {
  const api = useApi();
  return useQuery({ queryKey: keys.protections(tenant, tree), queryFn: () => api.trees.protections(tenant, tree) });
}

export function useLicenses(tenant: string, tree: string) {
  const api = useApi();
  return useQuery({ queryKey: keys.licenses(tenant, tree), queryFn: () => api.trees.licenses(tenant, tree) });
}

export function useMergeRequests(tenant: string, tree: string, state: MergeRequestState | "") {
  const api = useApi();
  return useQuery({
    queryKey: keys.mergeRequests(tenant, tree, state),
    queryFn: () => api.mergeRequests.list(tenant, tree, state || undefined),
    placeholderData: keepPreviousData,
  });
}

export function useMergeRequest(tenant: string, tree: string, n: number) {
  const api = useApi();
  return useQuery({
    queryKey: keys.mergeRequest(tenant, tree, n),
    queryFn: () => api.mergeRequests.get(tenant, tree, n),
    enabled: Number.isInteger(n) && n > 0,
  });
}

// ---- v0.3 admin expansion ----

export function useCompare(tenant: string, tree: string, to: string, from?: string) {
  const api = useApi();
  return useQuery({
    queryKey: keys.compare(tenant, tree, from ?? "", to),
    queryFn: () => api.trees.compare(tenant, tree, { to, from: from || undefined }),
    enabled: to !== "",
    placeholderData: keepPreviousData,
  });
}

export function useFileHistory(tenant: string, tree: string, path: string, branch: string) {
  const api = useApi();
  return useQuery({
    queryKey: keys.fileHistory(tenant, tree, branch, path),
    queryFn: () => api.trees.fileHistory(tenant, tree, { path, branch: branch || undefined, limit: 100 }),
    enabled: path !== "",
  });
}

export function useTreeStats(tenant: string, tree: string, branch: string, days = 30) {
  const api = useApi();
  return useQuery({
    queryKey: keys.treeStats(tenant, tree, branch, days),
    queryFn: () => api.trees.stats(tenant, tree, { branch: branch || undefined, days }),
    placeholderData: keepPreviousData,
  });
}

export function useStagedSummary(tenant: string, tree: string) {
  const api = useApi();
  return useQuery({
    queryKey: keys.stagedSummary(tenant, tree),
    queryFn: () => api.trees.stagedSummary(tenant, tree),
  });
}

export function useRecentEvents(q: { tenant?: string; tree?: string; limit?: number } = {}) {
  const api = useApi();
  return useQuery({ queryKey: keys.recentEvents(q), queryFn: () => api.events.recent(q) });
}

export function useSearch(q: string) {
  const api = useApi();
  const term = q.trim();
  return useQuery({
    queryKey: keys.search(term),
    queryFn: () => api.search(term),
    enabled: term.length > 0,
    placeholderData: keepPreviousData,
    staleTime: 10_000,
  });
}

export function useSystemInfo(enabled: boolean) {
  const api = useApi();
  return useQuery({ queryKey: keys.system, queryFn: api.admin.system, enabled, refetchInterval: 10_000 });
}
