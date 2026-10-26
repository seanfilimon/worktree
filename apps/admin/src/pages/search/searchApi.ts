/**
 * Code search v2 endpoints (docs/ENGINEERING_PLAN.md §15.9, workstream
 * `search-v2`): code search with a branch scope, symbol search and the
 * code index status / rebuild endpoints for server admins.
 *
 * Field names mirror services/server-go/internal/codesearch/{symsearch.go,status.go}.
 * Go encodes empty slices as `null`; lists are normalised to `[]`.
 */
import { keepPreviousData, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { ApiClient } from "@/api/client";
import { useApi } from "@/api/context";
import { MIN_CODE_QUERY, retryV06, type CodeSearchHit, type CodeSearchQuery, type CodeSearchResult } from "@/api/v06";

const V1 = "/api/v1";

export const SYMBOL_KINDS = ["function", "method", "class", "struct", "interface", "trait", "enum", "type", "const"] as const;
export type SymbolKind = (typeof SYMBOL_KINDS)[number];

export function isSymbolKind(v: unknown): v is SymbolKind {
  return typeof v === "string" && (SYMBOL_KINDS as readonly string[]).includes(v);
}

/** Which branches a search covers: the default branch, one branch, or all. */
export interface BranchScope {
  branch?: string;
  allBranches?: boolean;
}

export type CodeSearchV2Query = CodeSearchQuery & BranchScope;

/** One symbol definition (codesearch.SymbolHit). */
export interface SymbolHit {
  tenant: string;
  tree: string;
  branch: string;
  snapshot_id: string;
  path: string;
  /** 1-based. */
  line: number;
  name: string;
  kind: SymbolKind;
  container?: string;
  signature: string;
  language: string;
  match: "exact" | "prefix" | "substring";
}

export interface SymbolQuery extends BranchScope {
  q: string;
  kind?: SymbolKind;
  tenant?: string;
  tree?: string;
  path?: string;
  limit?: number;
}

export interface SymbolSearchResult {
  symbols: SymbolHit[];
  truncated: boolean;
  searched_trees: number;
}

/** codesearch.CodeIndexBranch. */
export interface CodeIndexBranch {
  branch: string;
  snapshot_id: string;
  manifest: string;
  files: number;
  bytes: number;
  symbols: number;
  reused: number;
  built_at: string;
  build_ms: number;
  loaded: boolean;
  building: boolean;
  error?: string;
}

/** codesearch.CodeIndexTree. */
export interface CodeIndexTree {
  tree_id: string;
  tenant: string;
  tree: string;
  default_branch: string;
  /** "ready", "building" or "error". */
  state: string;
  files: number;
  bytes: number;
  symbols: number;
  trigrams: number;
  built_at: string;
  branch_count: number;
  segments: number;
  memory_bytes: number;
  disk_bytes: number;
  branches: CodeIndexBranch[];
  error?: string;
}

/** codesearch.IndexStatus. */
export interface CodeIndexStatus {
  format_version: number;
  persistent: boolean;
  memory_bytes: number;
  memory_limit: number;
  loaded_segments: number;
  evictions: number;
  disk_bytes: number;
  max_branches: number;
  builds: number;
  disk_loads: number;
  trees: CodeIndexTree[];
}

export interface CodeIndexRebuildResult {
  rebuilt: number;
  trees: CodeIndexTree[];
}

function arr<T>(v: T[] | null | undefined): T[] {
  return Array.isArray(v) ? v : [];
}

function scopeQuery(s: BranchScope): { branch?: string; branches?: string } {
  if (s.allBranches) return { branches: "all" };
  return { branch: s.branch || undefined };
}

function normTree(t: CodeIndexTree): CodeIndexTree {
  return { ...t, branches: arr(t.branches) };
}

export function createSearchApi(client: ApiClient) {
  return {
    code: async (q: CodeSearchV2Query): Promise<CodeSearchResult> => {
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
          ...scopeQuery(q),
        },
      });
      const hits = arr(r?.hits).map((h: CodeSearchHit) => ({ ...h, before: arr(h.before), after: arr(h.after) }));
      return { hits, truncated: Boolean(r?.truncated), searched_trees: r?.searched_trees ?? 0 };
    },
    symbols: async (q: SymbolQuery): Promise<SymbolSearchResult> => {
      const r = await client.request<Partial<SymbolSearchResult> | null>(`${V1}/search/symbols`, {
        query: { q: q.q, kind: q.kind, tenant: q.tenant, tree: q.tree, path: q.path, limit: q.limit, ...scopeQuery(q) },
      });
      return { symbols: arr(r?.symbols), truncated: Boolean(r?.truncated), searched_trees: r?.searched_trees ?? 0 };
    },
    indexStatus: async (): Promise<CodeIndexStatus> => {
      const r = await client.request<CodeIndexStatus>(`${V1}/admin/code-index`);
      return { ...r, trees: arr(r?.trees).map(normTree) };
    },
    rebuild: async (tree?: string): Promise<CodeIndexRebuildResult> => {
      const r = await client.request<CodeIndexRebuildResult>(`${V1}/admin/code-index/rebuild`, {
        method: "POST",
        query: { tree: tree || undefined },
      });
      return { rebuilt: r?.rebuilt ?? 0, trees: arr(r?.trees).map(normTree) };
    },
  };
}

export const searchKeys = {
  code: (q: CodeSearchV2Query) => ["search", "code-v2", q] as const,
  symbols: (q: SymbolQuery) => ["search", "symbols", q] as const,
  index: ["admin", "code-index"] as const,
};

function useSearchApi() {
  const { client } = useApi();
  return createSearchApi(client);
}

export function useCodeSearchV2(q: CodeSearchV2Query, enabled = true) {
  const api = useSearchApi();
  const term = q.q.trim();
  const ok = q.regex ? term.length > 0 : term.length >= MIN_CODE_QUERY;
  return useQuery({
    queryKey: searchKeys.code({ ...q, q: term }),
    queryFn: () => api.code({ ...q, q: term }),
    enabled: enabled && ok,
    retry: retryV06,
    placeholderData: keepPreviousData,
    staleTime: 30_000,
  });
}

export function useSymbolSearch(q: SymbolQuery, enabled = true) {
  const api = useSearchApi();
  const term = q.q.trim();
  return useQuery({
    queryKey: searchKeys.symbols({ ...q, q: term }),
    queryFn: () => api.symbols({ ...q, q: term }),
    enabled: enabled && term.length > 0,
    retry: retryV06,
    placeholderData: keepPreviousData,
    staleTime: 30_000,
  });
}

/** Branch names of one tree for the branch selector (only when a tree is chosen). */
export function useTreeBranchNames(ref: { tenant: string; tree: string } | null) {
  const api = useApi();
  return useQuery({
    queryKey: ["search", "branches", ref?.tenant ?? "", ref?.tree ?? ""] as const,
    queryFn: async () => (await api.trees.branches(ref!.tenant, ref!.tree)).map((b) => b.name).sort(),
    enabled: ref !== null,
    retry: false,
    staleTime: 60_000,
  });
}

export function useCodeIndexStatus(enabled: boolean) {
  const api = useSearchApi();
  return useQuery({ queryKey: searchKeys.index, queryFn: api.indexStatus, enabled, retry: retryV06 });
}

export function useRebuildCodeIndex() {
  const api = useSearchApi();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (tree?: string) => api.rebuild(tree),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: searchKeys.index });
      void qc.invalidateQueries({ queryKey: ["search"] });
    },
  });
}
