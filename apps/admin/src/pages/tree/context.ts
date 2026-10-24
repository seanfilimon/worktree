import { useOutletContext } from "react-router";
import type { StreamStatus } from "@/api/events";
import type { TreeSummary } from "@/api/types";

export interface TreeContext {
  tenant: string;
  tree: string;
  summary: TreeSummary;
  /** State of the tree's live event subscription. */
  live: StreamStatus;
  /** Caller may manage protections / licenses / settings. */
  canManage: boolean;
}

export function useTreeContext(): TreeContext {
  return useOutletContext<TreeContext>();
}
