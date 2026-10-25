/** Non-component helpers for the v0.5 tree screens (§13.12 admin-tree). */
import { errorMessage, isApiError } from "@/api/client";
import type { CIState, TodoStatus } from "@/api/v05tree";
import { useAuth } from "@/auth/context";
import { describeMergeError, type MergeErrorInfo } from "@/lib/mergeErrors";
import { useTreeContext } from "../context";

/** `describeMergeError` plus the v0.5 `merge_blocked` / `linked_group` rejections (§13.4). */
export function describeV05MergeError(err: unknown): MergeErrorInfo {
  if (isApiError(err) && (err.code === "merge_blocked" || err.code === "linked_group")) {
    const detail = errorMessage(err);
    return err.code === "linked_group"
      ? {
          kind: "other",
          title: "Part of a linked group",
          message: "This merge request merges only together with the other members of its linked group. Merge the group instead.",
          detail,
        }
      : { kind: "other", title: "Merge blocked", message: detail, detail };
  }
  return describeMergeError(err);
}

/** Whether the caller may write to the current tree (developer and up). UI gating only. */
export function useCanWrite(): boolean {
  const { summary } = useTreeContext();
  const { roleIn, isAdmin } = useAuth();
  const role = roleIn(summary.tree.tenant_id);
  return isAdmin || role === "owner" || role === "admin" || role === "maintainer" || role === "developer";
}

export const TODO_STATUS_LABEL: Record<TodoStatus, string> = {
  open: "Open",
  in_progress: "In progress",
  done: "Done",
  cancelled: "Cancelled",
};

const CI_LABEL: Record<CIState, string> = { success: "Passed", failure: "Failed", error: "Errored", pending: "Pending" };

export function ciStateLabel(state: CIState | ""): string {
  return state ? CI_LABEL[state] : "No checks";
}

/** Saves a blob through a temporary object URL (no-op where unsupported, e.g. jsdom). */
export function saveBlob(blob: Blob, fileName: string): void {
  if (typeof URL.createObjectURL !== "function") return;
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = fileName;
  document.body.appendChild(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 0);
}
