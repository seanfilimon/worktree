/**
 * staged-v2 endpoints (docs/ENGINEERING_PLAN.md §16.8) used by StagedTab:
 * staged history and per-label unstage.
 *
 * Field names mirror services/server-go/internal/domain/types.go
 * (StagedSnapshot, StagedHistoryEntry). Go encodes empty slices as `null`;
 * the helpers below normalise lists to `[]`.
 */
import { useQuery } from "@tanstack/react-query";
import { useMemo } from "react";
import type { ApiClient } from "@/api/client";
import { useApi } from "@/api/context";
import type { ISODateString, StagedSnapshot } from "@/api/types";
import { retryV06, type StagedSnapshotV06 } from "@/api/v06";

// ---- wire types ----

export const STAGED_HISTORY_STATUSES = ["pushed", "cleared", "expired"] as const;
export type StagedHistoryStatus = (typeof STAGED_HISTORY_STATUSES)[number];

export function isStagedHistoryStatus(v: unknown): v is StagedHistoryStatus {
  return typeof v === "string" && (STAGED_HISTORY_STATUSES as readonly string[]).includes(v);
}

/** An ended staged snapshot, kept 30 days: the StagedSnapshot fields plus how it ended. */
export interface StagedHistoryEntry extends StagedSnapshotV06 {
  id: string;
  status: StagedHistoryStatus | (string & {});
  ended_at: ISODateString;
  /** Username that pushed or unstaged it, or "system" (expiry). */
  ended_by: string;
  /** Branch tip produced by the push (status "pushed"). */
  pushed_tip?: string;
}

/** GET {tree}/staged/history. */
export interface StagedHistoryResponse {
  history: StagedHistoryEntry[];
}

export interface StagedHistoryFilter {
  branch?: string;
  /** Username or account id. */
  account?: string;
  status?: StagedHistoryStatus;
  limit?: number;
}

/** DELETE {tree}/staged. */
export interface UnstageResponse {
  removed: StagedSnapshot[];
}

export interface UnstageTarget {
  branch: string;
  /** The slot label; "" = the default slot. */
  label: string;
  /** Another account's entry (username or id; requires tree admin). Omit for your own. */
  account?: string;
}

// ---- helpers ----

function arr<T>(v: T[] | null | undefined): T[] {
  return Array.isArray(v) ? v : [];
}

/** The slot label of a staged entry ("" = default). */
export function labelOf(s: { label?: string }): string {
  return s.label ?? "";
}

/** "feature/login" for the default slot, "feature/login (spike)" for a labeled one. */
export function slotName(s: { branch_name: string; label?: string }): string {
  const label = labelOf(s);
  return label ? `${s.branch_name} (${label})` : s.branch_name;
}

// ---- client ----

export function createStagedV2Api(client: ApiClient) {
  const tree = (t: string, n: string) => `/api/v1/trees/${encodeURIComponent(t)}/${encodeURIComponent(n)}`;
  return {
    history: async (t: string, n: string, f: StagedHistoryFilter = {}): Promise<StagedHistoryEntry[]> => {
      const r = await client.request<Partial<StagedHistoryResponse> | null>(`${tree(t, n)}/staged/history`, {
        query: { branch: f.branch, account: f.account, status: f.status, limit: f.limit },
      });
      return arr(r?.history).map((e) => ({ ...e, files_changed: arr(e.files_changed) }));
    },
    /** Removes one slot. `label=` is always sent (empty = the default slot), so other labels survive. */
    unstage: async (t: string, n: string, target: UnstageTarget): Promise<StagedSnapshot[]> => {
      // buildQuery drops empty values, so the query string is assembled here to keep `label=`.
      const q = new URLSearchParams({ branch: target.branch, label: target.label });
      if (target.account) q.set("account", target.account);
      const r = await client.request<Partial<UnstageResponse> | null>(`${tree(t, n)}/staged?${q.toString()}`, { method: "DELETE" });
      return arr(r?.removed);
    },
  };
}

export type StagedV2Api = ReturnType<typeof createStagedV2Api>;

// ---- query keys + hooks ----

/**
 * History lives under ["tree", t, n, "staged", …] so the tree layout's live
 * "staged" invalidation (snapshot.staged, staged.cleared, branch.pushed)
 * refreshes it too. "~" cannot appear in a branch name, so it never collides
 * with the per-branch live list key.
 */
export const stagedV2Keys = {
  staged: (t: string, n: string) => ["tree", t, n, "staged"] as const,
  history: (t: string, n: string, f: StagedHistoryFilter) =>
    ["tree", t, n, "staged", "~history", f.branch ?? "", f.account ?? "", f.status ?? "", f.limit ?? 0] as const,
};

export function useStagedV2Api(): StagedV2Api {
  const { client } = useApi();
  return useMemo(() => createStagedV2Api(client), [client]);
}

export function useStagedHistory(t: string, n: string, f: StagedHistoryFilter) {
  const api = useStagedV2Api();
  return useQuery({ queryKey: stagedV2Keys.history(t, n, f), queryFn: () => api.history(t, n, f), retry: retryV06 });
}
