/** v0.7 merge refusals (§15.7): drafts, missing owner approval and storage quotas. */
import { errorMessage, isApiError } from "@/api/client";
import type { MergeErrorInfo } from "@/lib/mergeErrors";
import { describeV06MergeError } from "../v06/mergeUtil";

export function describeV07MergeError(err: unknown): MergeErrorInfo {
  if (isApiError(err)) {
    const detail = errorMessage(err);
    if (err.code === "draft") {
      return { kind: "other", title: "Draft merge request", message: "Drafts cannot be merged. Mark it ready for review first.", detail };
    }
    if (err.code === "quota_exceeded") {
      return {
        kind: "other",
        title: "Storage quota exceeded",
        message: "The merge would exceed the tenant's storage quota. Free up space or raise the quota, then retry.",
        detail,
      };
    }
    if (err.code === "merge_blocked" && err.details.kind === "owners") {
      return { kind: "other", title: "Owner approval missing", message: detail, detail };
    }
  }
  return describeV06MergeError(err);
}
