/**
 * v0.8 history-v2 (docs/ENGINEERING_PLAN.md §16.10): force pushes, the
 * server reflog of branch moves and the `no_force_push` protection flag.
 *
 * Field names mirror services/server-go/internal/syncsvc/push_force_ops.go
 * (`ReflogEntry`), restore.go (`RestoreResult`) and domain.BranchProtection.
 * Lists are normalised to `[]`.
 */
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useMemo } from "react";
import type { ApiClient } from "./client";
import { useApi } from "./context";
import type { ISODateString } from "./types";
import type { BranchProtectionV07 } from "./v07review";

const V1 = "/api/v1";
const seg = encodeURIComponent;

/** domain.BranchProtection with the v0.8 field (omitted when false). */
export interface BranchProtectionV08 extends BranchProtectionV07 {
  /** Refuse force pushes, even from admins and owners bypassing protection. */
  no_force_push?: boolean;
}

/** One branch move in the server reflog (GET {tree}/reflog). */
export interface ReflogEntry {
  tree_id: string;
  branch: string;
  /** Tip before the push; "" when the push created the branch. */
  old_tip: string;
  new_tip: string;
  /** `restore`: POST {tree}/reflog/restore moved the branch to new_tip. */
  kind: "push" | "force_push" | "restore";
  /** A force push or rewinding restore: old_tip is no longer an ancestor of new_tip. */
  forced: boolean;
  /** Branch protection was bypassed (owners, branch:bypass_protection). */
  bypass: boolean;
  bypassed: string[];
  actor: string;
  actor_id: string;
  count: number;
  at: ISODateString;
}

interface ReflogWire extends Omit<ReflogEntry, "bypassed"> {
  bypassed?: string[] | null;
}

export function normReflog(
  v: { entries?: ReflogWire[] | null } | null | undefined,
): ReflogEntry[] {
  const list = Array.isArray(v?.entries) ? v.entries : [];
  return list.map((e) => ({
    ...e,
    old_tip: e.old_tip ?? "",
    forced: Boolean(e.forced),
    bypass: Boolean(e.bypass),
    bypassed: Array.isArray(e.bypassed) ? e.bypassed : [],
  }));
}

/** Body of POST {tree}/reflog/restore. */
export interface RestoreBranchRequest {
  branch: string;
  /** old_tip or new_tip of a reflog entry of the branch. */
  tip: string;
  /** The branch tip the caller saw (lease; 409 when it moved). */
  expected_tip: string;
  bypass?: boolean;
}

/** Response of POST {tree}/reflog/restore. */
export interface RestoreBranchResult {
  branch: string;
  old_tip: string;
  new_tip: string;
  forced: boolean;
  bypass: boolean;
  bypassed: string[];
  entry: ReflogEntry;
}

export function createHistoryV2Api(client: ApiClient) {
  const tree = (t: string, n: string) => `${V1}/trees/${seg(t)}/${seg(n)}`;
  return {
    /** GET {tree}/reflog?branch=&limit= (newest first). */
    reflog: async (t: string, n: string, branch?: string, limit = 50) => {
      const q = new URLSearchParams({ limit: String(limit) });
      if (branch) q.set("branch", branch);
      return normReflog(
        await client.request<{ entries?: ReflogWire[] | null }>(
          `${tree(t, n)}/reflog?${q.toString()}`,
        ),
      );
    },
    /** POST {tree}/reflog/restore: move a branch to a tip of its reflog (branch:force_push). */
    restore: (t: string, n: string, body: RestoreBranchRequest) =>
      client.request<RestoreBranchResult>(`${tree(t, n)}/reflog/restore`, { method: "POST", body }),
  };
}

export type HistoryV2Api = ReturnType<typeof createHistoryV2Api>;

/** Keys live under ["tree", tenant, tree, ...] so tree-wide invalidations refresh them. */
export const historyV2Keys = {
  reflog: (t: string, n: string, branch: string) =>
    ["tree", t, n, "reflog", branch] as const,
};

export function useHistoryV2Api(): HistoryV2Api {
  const { client } = useApi();
  return useMemo(() => createHistoryV2Api(client), [client]);
}

export function useReflog(t: string, n: string, branch: string) {
  const api = useHistoryV2Api();
  return useQuery({
    queryKey: historyV2Keys.reflog(t, n, branch),
    queryFn: () => api.reflog(t, n, branch),
  });
}

/** Restore a branch from its reflog; refreshes every tree query on success. */
export function useRestoreBranch(t: string, n: string) {
  const api = useHistoryV2Api();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (body: RestoreBranchRequest) => api.restore(t, n, body),
    // RestoreButton reports refusals (permissions, protection, a moved tip).
    meta: { inlineError: true },
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: ["tree", t, n] }),
  });
}

/** The force pushes (and rewinding restores) among reflog entries (newest first). */
export function forcePushes(entries: readonly ReflogEntry[]): ReflogEntry[] {
  return entries.filter((e) => e.forced || e.kind === "force_push");
}

/** Force pushes of `branch` (newest first) and, by new tip, the push that made it the tip. */
export function useForcePushes(tenant: string, tree: string, branch: string) {
  const reflog = useReflog(tenant, tree, branch);
  return useMemo(() => {
    const list = forcePushes(reflog.data ?? []);
    const byTip = new Map<string, ReflogEntry>();
    for (const e of list) if (!byTip.has(e.new_tip)) byTip.set(e.new_tip, e);
    return { list, byTip };
  }, [reflog.data]);
}
