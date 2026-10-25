import { isApiError, errorMessage } from "@/api/client";

export type MergeErrorKind =
  | "review_required"
  | "diverged"
  | "target_moved"
  | "self_approval"
  | "already_open"
  | "not_open"
  | "forbidden"
  | "invalid"
  | "other";

export interface MergeErrorInfo {
  kind: MergeErrorKind;
  title: string;
  message: string;
  /** Server-provided text, shown verbatim for diagnosis. */
  detail: string;
}

/**
 * Classifies a failed create/approve/merge/close call. The server maps
 * `*PushDenied{Kind:"review_required"}` onto code `review_required` and
 * `ErrConflict` / `ErrForbidden` onto the generic codes with the specifics in
 * the message, so both the code and the message are inspected.
 */
export function describeMergeError(err: unknown): MergeErrorInfo {
  const detail = errorMessage(err);
  if (!isApiError(err)) {
    return { kind: "other", title: "Request failed", message: detail, detail };
  }
  const kind = typeof err.details.kind === "string" ? err.details.kind : "";
  const text = `${err.code} ${kind} ${err.message}`.toLowerCase();
  const info = (k: MergeErrorKind, title: string, message: string = detail): MergeErrorInfo => ({
    kind: k,
    title,
    message,
    detail,
  });

  if (/review[_ ]required|(not enough|insufficient|more|requires? \d+) approvals?/.test(text)) {
    return info(
      "review_required",
      "Review required",
      "The target branch is protected and this merge request does not have enough approvals yet.",
    );
  }
  if (err.code === "forbidden" && /(cannot|can't) approve (their|your) own/.test(text)) {
    return info("self_approval", "You cannot approve your own merge request", "Ask another member of the tree to review it.");
  }
  if (err.code === "conflict" && /moved during|retry/.test(text)) {
    return info(
      "target_moved",
      "Target branch moved",
      "Someone pushed to the target branch while merging. Review the latest changes and retry.",
    );
  }
  if (/diverged|not an ancestor|fast-forward|non-fast-forward/.test(text) || (err.code === "conflict" && !/state|already|closed|merged/.test(text))) {
    return info(
      "diverged",
      "Branches have diverged",
      "The target branch has moved and can no longer be fast-forwarded. Merge the target into the source branch locally (wt merge), push, then retry.",
    );
  }
  if (err.code === "conflict" && /already open/.test(text)) {
    return info("already_open", "A merge request is already open", "There is already an open merge request for these branches.");
  }
  if (err.code === "conflict") return info("not_open", "Merge request is not open");
  if (err.code === "forbidden") {
    return info("forbidden", "Permission denied", "You do not have permission to perform this action on this tree.");
  }
  if (err.code === "bad_request") return info("invalid", "Invalid merge request");
  return info("other", "Request failed");
}
