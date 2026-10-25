import { describe, expect, it } from "vitest";
import { ApiError } from "@/api/client";
import { describeMergeError } from "./mergeErrors";

const kind = (status: number, code: string, message: string) => describeMergeError(new ApiError(status, code, message)).kind;

describe("describeMergeError (server messages)", () => {
  it.each([
    [403, "review_required", "1 of 2 approvals", "review_required"],
    [409, "conflict", "merge request !7: target has diverged; merge target into source locally and push: conflict", "diverged"],
    [409, "conflict", "merge request !7: target branch 'main' moved during the merge; retry: conflict", "target_moved"],
    [403, "forbidden", "merge request !7: authors cannot approve their own merge request: forbidden", "self_approval"],
    [409, "conflict", "merge request !7 from 'feature/login' into 'main' is already open: conflict", "already_open"],
    [409, "conflict", "merge request !7 is merged: conflict", "not_open"],
    [409, "conflict", "merge request !7 is closed: conflict", "not_open"],
    [403, "forbidden", "permission denied", "forbidden"],
    [400, "bad_request", "merge request title must not be empty: invalid argument", "invalid"],
    [500, "internal", "internal server error", "other"],
  ])("%i %s %j -> %s", (status, code, message, expected) => {
    expect(kind(status, code, message)).toBe(expected);
  });

  it("keeps the server text as detail and explains the fix", () => {
    const info = describeMergeError(new ApiError(409, "conflict", "target has diverged"));
    expect(info.title).toBe("Branches have diverged");
    expect(info.message).toContain("wt merge");
    expect(info.detail).toBe("target has diverged");
  });

  it("shows the server message itself when there is nothing to add", () => {
    const info = describeMergeError(new ApiError(400, "bad_request", "source and target must differ"));
    expect(info.message).toBe("source and target must differ");
    expect(info.detail).toBe(info.message);
  });
});
