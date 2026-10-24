import { describe, expect, it } from "vitest";
import { ApiError } from "@/api/client";
import type { SnapshotFile } from "@/api/types";
import { describeEvent } from "./describeEvent";
import { decodeText, listDirectory, splitLines } from "./files";
import { formatBytes, formatDuration, relativeTime, shortId } from "./format";
import { pathMatch } from "./glob";
import { describeMergeError } from "./mergeErrors";

const file = (path: string, size = 1): SnapshotFile => ({ path, size, hash: "00", executable: false });

describe("listDirectory", () => {
  const files = [file("README.md"), file("src/app.ts", 10), file("src/lib/a.ts", 5), file("src/lib/b.ts", 5), file("docs/x.md")];

  it("lists root directories first, then files, sorted", () => {
    const l = listDirectory(files, "");
    expect(l.dirs.map((d) => d.name)).toEqual(["docs", "src"]);
    expect(l.files.map((f) => f.path)).toEqual(["README.md"]);
    expect(l.dirs.find((d) => d.name === "src")).toMatchObject({ path: "src", fileCount: 3, size: 20 });
  });

  it("lists nested directories and tolerates slashes", () => {
    const l = listDirectory(files, "/src/");
    expect(l.dirs).toEqual([{ name: "lib", path: "src/lib", fileCount: 2, size: 10 }]);
    expect(l.files.map((f) => f.path)).toEqual(["src/app.ts"]);
  });
});

describe("decodeText", () => {
  it("decodes UTF-8 text", () => {
    expect(decodeText(new TextEncoder().encode("héllo\n").buffer)).toBe("héllo\n");
  });

  it("detects binary content", () => {
    expect(decodeText(new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x00, 0x01]).buffer)).toBeNull();
    expect(decodeText(new Uint8Array([0xff, 0xfe, 0xfd]).buffer)).toBeNull();
  });

  it("splitLines drops the trailing newline only", () => {
    expect(splitLines("a\r\nb\n")).toEqual(["a", "b"]);
    expect(splitLines("a\n\n")).toEqual(["a", ""]);
  });
});

describe("pathMatch (Go path.Match)", () => {
  it.each([
    ["main", "main", true],
    ["main", "mainline", false],
    ["release/*", "release/1.0", true],
    ["release/*", "release/1.0/hotfix", false],
    ["v?", "v1", true],
    ["[a-c]x", "bx", true],
    ["[^a-c]x", "bx", false],
    ["feat.x", "featyx", false],
  ])("%s ~ %s → %s", (pattern, name, expected) => {
    expect(pathMatch(pattern, name)).toBe(expected);
  });
});

describe("format", () => {
  it("formats relative times", () => {
    const now = Date.parse("2026-10-24T12:00:00Z");
    expect(relativeTime("2026-10-24T11:59:50Z", now)).toBe("just now");
    expect(relativeTime("2026-10-24T11:55:00Z", now)).toBe("5 minutes ago");
    expect(relativeTime("2026-10-23T12:00:00Z", now)).toBe("yesterday");
    expect(relativeTime("not a date", now)).toBe("—");
  });

  it("formats sizes, durations and ids", () => {
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(1536)).toBe("1.5 KiB");
    expect(formatBytes(5 * 1024 ** 3)).toBe("5.0 GiB");
    expect(formatDuration(3700)).toBe("1h 1m");
    expect(formatDuration(90061)).toBe("1d 1h");
    expect(shortId("0f3c9a2e-1111-4222-8333-444455556666")).toBe("0f3c9a2e");
  });
});

describe("describeMergeError", () => {
  it("recognises review_required from the code", () => {
    expect(describeMergeError(new ApiError(403, "review_required", "needs 2 approvals")).kind).toBe("review_required");
  });

  it("recognises review_required from push-denied details or message", () => {
    expect(describeMergeError(new ApiError(403, "forbidden", "push denied", { kind: "review_required" })).kind).toBe("review_required");
    expect(describeMergeError(new ApiError(403, "forbidden", "push denied (review_required): 1 of 2 approvals")).kind).toBe("review_required");
  });

  it("recognises diverged branches", () => {
    expect(describeMergeError(new ApiError(409, "conflict", "target is not an ancestor of source")).kind).toBe("diverged");
    expect(describeMergeError(new ApiError(409, "diverged", "branches diverged")).kind).toBe("diverged");
  });

  it("distinguishes state conflicts and permissions", () => {
    expect(describeMergeError(new ApiError(409, "conflict", "merge request is already merged")).kind).toBe("not_open");
    expect(describeMergeError(new ApiError(403, "forbidden", "not allowed")).kind).toBe("forbidden");
    expect(describeMergeError(new Error("boom"))).toMatchObject({ kind: "other", message: "boom" });
  });
});

describe("describeEvent", () => {
  it("summarises known events from their payload", () => {
    expect(describeEvent({ type: "branch.pushed", tenant_id: "t", at: "", payload: { branch: "main", tree: "web" } })).toEqual({
      action: "pushed",
      target: "web · main",
    });
    expect(describeEvent({ type: "mr.merged", tenant_id: "t", at: "", payload: { number: 7, title: "Login" } }).target).toBe("!7 · Login");
  });

  it("handles unknown types and missing payloads", () => {
    expect(describeEvent({ type: "custom.thing", tenant_id: "t", at: "" })).toEqual({ action: "custom.thing", target: "" });
  });
});
