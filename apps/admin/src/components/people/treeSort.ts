import type { TreeSummary } from "@/api/types";

export interface TreeRow {
  tenant: string;
  summary: TreeSummary;
}

export const TREE_SORT_KEYS = ["name", "updated", "snapshots"] as const;
export type TreeSortKey = (typeof TREE_SORT_KEYS)[number];
export type SortDir = "asc" | "desc";

export interface TreeSort {
  key: TreeSortKey;
  dir: SortDir;
}

export function isTreeSortKey(v: string | null): v is TreeSortKey {
  return (TREE_SORT_KEYS as readonly string[]).includes(v ?? "");
}

/** Natural first direction per column: A→Z, newest first, most snapshots first. */
export const DEFAULT_DIR: Record<TreeSortKey, SortDir> = { name: "asc", updated: "desc", snapshots: "desc" };

const fullName = (r: TreeRow) => `${r.tenant}/${r.summary.tree.name}`;

const COMPARE: Record<TreeSortKey, (a: TreeRow, b: TreeRow) => number> = {
  name: (a, b) => fullName(a).localeCompare(fullName(b)),
  updated: (a, b) => (Date.parse(a.summary.tree.updated_at) || 0) - (Date.parse(b.summary.tree.updated_at) || 0),
  snapshots: (a, b) => a.summary.snapshot_count - b.summary.snapshot_count,
};

/** Stable sort; ties fall back to the full tree name. */
export function sortTreeRows(rows: readonly TreeRow[], { key, dir }: TreeSort): TreeRow[] {
  const sign = dir === "asc" ? 1 : -1;
  return [...rows].sort((a, b) => sign * COMPARE[key](a, b) || COMPARE.name(a, b));
}
