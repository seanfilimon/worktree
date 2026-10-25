import type { Branch, BranchProtection, CompareResult, MergeRequest } from "@/api/types";
import { pathMatch } from "@/lib/glob";

/**
 * How a merge request's source relates to its target, derived from
 * `compare?from=<target tip>&to=<source tip>`:
 * - `fast_forward`: the target tip is an ancestor of the source tip.
 * - `up_to_date`: nothing to merge (the target already contains the source).
 * - `diverged`: both moved since the merge base; needs a local merge first.
 */
export type BranchRelation = "fast_forward" | "up_to_date" | "diverged";

export function branchRelation(c: CompareResult): BranchRelation {
  if (c.snapshots.length === 0) return "up_to_date";
  if (c.merge_base && c.merge_base !== c.from?.id) return "diverged";
  return "fast_forward";
}

/**
 * Every protection rule whose pattern matches `branch`. The server merges all
 * of them into their most restrictive combination (ceiling model).
 */
export function matchingProtections(rules: readonly BranchProtection[], branch: string): BranchProtection[] {
  return rules.filter((r) => pathMatch(r.pattern, branch));
}

/** Approvals the target branch demands: the maximum over matching rules. */
export function requiredApprovals(rules: readonly BranchProtection[]): number {
  return rules.reduce((max, r) => Math.max(max, r.required_approvals), 0);
}

export function approvalCount(mr: MergeRequest): number {
  return mr.approvals?.length ?? 0;
}

/** "feature/login-page" -> "Login page". */
export function suggestTitle(branch: string): string {
  const leaf = branch.split("/").filter(Boolean).pop() ?? "";
  const words = leaf.replace(/[-_.]+/g, " ").trim();
  return words ? words.charAt(0).toUpperCase() + words.slice(1) : "";
}

export function findBranch(branches: readonly Branch[] | undefined, name: string): Branch | undefined {
  return branches?.find((b) => b.name === name);
}

export type CheckStatus = "pass" | "fail" | "pending";

export interface ReadinessCheck {
  id: "open" | "approvals" | "fast_forward";
  label: string;
  status: CheckStatus;
  detail?: string;
}

export interface ReadinessInput {
  mr: MergeRequest;
  required: number;
  /** Null while the comparison is loading or unavailable. */
  relation: BranchRelation | null;
  /** Why the comparison is unavailable (e.g. a deleted source branch). */
  relationProblem?: string;
}

/** The merge readiness checklist and the first reason merging is blocked. */
export function readiness({ mr, required, relation, relationProblem }: ReadinessInput): {
  checks: ReadinessCheck[];
  blocker: string | null;
} {
  const have = approvalCount(mr);
  const open = mr.state === "open";
  const checks: ReadinessCheck[] = [
    {
      id: "open",
      label: "Merge request is open",
      status: open ? "pass" : "fail",
      detail: open ? undefined : `This merge request is ${mr.state}.`,
    },
    {
      id: "approvals",
      label: required > 0 ? `${Math.min(have, required)} of ${required} required approvals` : "No approvals required",
      status: have >= required ? "pass" : "fail",
      detail: have >= required ? undefined : `${required - have} more ${required - have === 1 ? "approval" : "approvals"} needed.`,
    },
    fastForwardCheck(relation, relationProblem),
  ];
  // Checks are ordered by precedence: the first one not passing blocks.
  const failed = checks.find((c) => c.status !== "pass");
  return { checks, blocker: failed ? (failed.detail ?? failed.label) : null };
}

function fastForwardCheck(relation: BranchRelation | null, problem: string | undefined): ReadinessCheck {
  if (problem) return { id: "fast_forward", label: "Fast-forwardable", status: "fail", detail: problem };
  switch (relation) {
    case null:
      return { id: "fast_forward", label: "Fast-forwardable", status: "pending", detail: "Checking branches…" };
    case "fast_forward":
      return { id: "fast_forward", label: "Fast-forwardable", status: "pass" };
    case "up_to_date":
      return {
        id: "fast_forward",
        label: "Target already contains the source",
        status: "pass",
      };
    case "diverged":
      return {
        id: "fast_forward",
        label: "Branches have diverged",
        status: "fail",
        detail: "Diverged: merge target into source locally, push, then merge.",
      };
  }
}
