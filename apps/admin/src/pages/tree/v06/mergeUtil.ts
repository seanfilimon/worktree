/** v0.6 server-side merge helpers (docs/ENGINEERING_PLAN.md §14.1, §14.9). */
import { errorMessage, isApiError } from "@/api/client";
import type { MergeRequest } from "@/api/types";
import { mergeConflictsOf, type Mergeability, type MergeMethod } from "@/api/v06";
import type { ReadinessCheck } from "@/components/mr/model";
import type { MergeErrorInfo } from "@/lib/mergeErrors";
import { describeV05MergeError } from "../v05/util";

/** The method preselected for a merge: fast-forward when possible, else the first allowed one. */
export function defaultMethod(m: Mergeability | undefined): MergeMethod | undefined {
  if (!m || m.methods.length === 0) return undefined;
  return m.methods.includes("fast-forward") ? "fast-forward" : m.methods[0];
}

/**
 * Replaces the client-side fast-forward check with the server's
 * mergeability: diverged branches can still merge with `merge`/`squash`,
 * conflicts or a protection allowing no possible method block.
 */
export function applyMergeability(checks: ReadinessCheck[], m: Mergeability | undefined, target: string): ReadinessCheck[] {
  if (!m) return checks;
  return checks.map((c): ReadinessCheck => {
    if (c.id !== "fast_forward") return c;
    if (m.conflicts.length > 0) {
      const n = m.conflicts.length;
      return {
        id: c.id,
        label: `${n} conflicting ${n === 1 ? "path" : "paths"}`,
        status: "fail",
        detail: "Conflicts: merge the target into the source locally (wt merge), resolve, push, then merge.",
      };
    }
    if (m.methods.length === 0) {
      return {
        id: c.id,
        label: "No merge method available",
        status: "fail",
        detail: `The protections of ${target} allow no merge method that is possible here.`,
      };
    }
    if (c.status === "fail" || c.status === "pending") {
      return { id: c.id, label: `Mergeable on the server (${m.methods.join(", ")})`, status: "pass" };
    }
    return c;
  });
}

/** `describeV05MergeError` plus the v0.6 `merge_conflict` / `merge_method_not_allowed` rejections. */
export function describeV06MergeError(err: unknown): MergeErrorInfo {
  const conflicts = mergeConflictsOf(err);
  if (conflicts) {
    const n = conflicts.length;
    return {
      kind: "diverged",
      title: "Merge conflicts",
      message:
        n > 0
          ? `${n} ${n === 1 ? "path conflicts" : "paths conflict"} between the branches. Merge the target into the source locally (wt merge), resolve, push, then retry.`
          : "The branches conflict. Merge the target into the source locally (wt merge), resolve, push, then retry.",
      detail: errorMessage(err),
    };
  }
  if (isApiError(err) && err.code === "merge_method_not_allowed") {
    return {
      kind: "forbidden",
      title: "Merge method not allowed",
      message: "The target branch's protection does not allow this merge method. Pick another one.",
      detail: errorMessage(err),
    };
  }
  return describeV05MergeError(err);
}

/** Confirmation text for a method. */
export function mergeMethodSummary(method: MergeMethod, mr: Pick<MergeRequest, "source_branch" | "target_branch">): string {
  switch (method) {
    case "fast-forward":
      return `${mr.target_branch} will be fast-forwarded to the tip of ${mr.source_branch}.`;
    case "merge":
      return `A merge snapshot of ${mr.source_branch} into ${mr.target_branch} is created on the server.`;
    case "squash":
      return `All changes of ${mr.source_branch} are squashed into one new snapshot on ${mr.target_branch}.`;
  }
}
