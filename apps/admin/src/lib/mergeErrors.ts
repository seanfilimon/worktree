import { isApiError, errorMessage } from "@/api/client";

export type MergeErrorKind = "review_required" | "diverged" | "forbidden" | "not_open" | "other";

export interface MergeErrorInfo {
  kind: MergeErrorKind;
  title: string;
  message: string;
  /** Server-provided text, shown verbatim for diagnosis. */
  detail: string;
}

/**
 * Classifies a failed approve/merge/close call. The server maps
 * `*PushDenied{Kind:"review_required"}` and `ErrConflict` (target is not an
 * ancestor of source) onto the error envelope; the exact code may be the
 * generic one (forbidden / conflict) with the specifics in the message, so
 * both are inspected.
 */
export function describeMergeError(err: unknown): MergeErrorInfo {
  const detail = errorMessage(err);
  if (!isApiError(err)) {
    return { kind: "other", title: "Request failed", message: detail, detail };
  }
  const kind = typeof err.details.kind === "string" ? err.details.kind : "";
  const text = `${err.code} ${kind} ${err.message}`.toLowerCase();

  if (/review[_ ]required|(not enough|insufficient|more|requires? \d+) approvals?/.test(text)) {
    return {
      kind: "review_required",
      title: "Review required",
      message: "The target branch is protected and this merge request does not have enough approvals yet.",
      detail,
    };
  }
  if (/diverged|not an ancestor|fast-forward|non-fast-forward/.test(text) || (err.code === "conflict" && !/state|already|closed|merged/.test(text))) {
    return {
      kind: "diverged",
      title: "Branches have diverged",
      message:
        "The target branch has moved and can no longer be fast-forwarded. Merge the target into the source branch locally (wt merge), push, then retry.",
      detail,
    };
  }
  if (err.code === "conflict") {
    return { kind: "not_open", title: "Merge request is not open", message: detail, detail };
  }
  if (err.code === "forbidden") {
    return {
      kind: "forbidden",
      title: "Permission denied",
      message: "You do not have permission to perform this action on this tree.",
      detail,
    };
  }
  return { kind: "other", title: "Request failed", message: detail, detail };
}
