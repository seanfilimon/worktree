import { describe, expect, it } from "vitest";
import { addedPatch, deletedPatch, modifiedPatch } from "@/test/fixtures.history";
import { parsePatch, toSplitRows } from "./diff";

describe("parsePatch", () => {
  it("parses headers, hunks, line numbers and counts", () => {
    const p = parsePatch(modifiedPatch);
    expect(p.oldPath).toBe("src/app.ts");
    expect(p.newPath).toBe("src/app.ts");
    expect(p.additions).toBe(3);
    expect(p.deletions).toBe(2);
    expect(p.hunks).toHaveLength(2);

    const [h1, h2] = p.hunks;
    expect(h1).toMatchObject({ oldStart: 1, oldLines: 4, newStart: 1, newLines: 5, section: "export function main()" });
    expect(h1!.lines.map((l) => [l.kind, l.oldNo, l.newNo, l.content])).toEqual([
      ["context", 1, 1, "const a = 1;"],
      ["del", 2, null, "const b = 2;"],
      ["add", null, 2, "const b = 3;"],
      ["add", null, 3, "const c = 4;"],
      ["context", 3, 4, "export default a;"],
      ["context", 4, 5, "// end"],
    ]);
    expect(h2).toMatchObject({ oldStart: 10, oldLines: 2, newStart: 11, newLines: 2, section: "" });
    expect(h2!.lines.at(-1)).toMatchObject({ kind: "context", content: "last", oldNo: 11, newNo: 12, noNewline: true });
  });

  it("treats /dev/null as a missing side and defaults omitted hunk counts to 1", () => {
    const added = parsePatch(addedPatch);
    expect(added.oldPath).toBeNull();
    expect(added.newPath).toBe("src/login.tsx");
    expect(added.hunks[0]!.lines.every((l) => l.kind === "add")).toBe(true);

    const deleted = parsePatch(deletedPatch);
    expect(deleted.newPath).toBeNull();
    expect(deleted.hunks[0]).toMatchObject({ oldStart: 1, oldLines: 1, newStart: 0, newLines: 0 });
    expect(deleted.deletions).toBe(1);
  });

  it("reads hunk bodies by count, so header-like content stays content", () => {
    const p = parsePatch(["--- a/x.sql", "+++ b/x.sql", "@@ -1,2 +1,2 @@", "--- a comment", "+++ counter", " ok", ""].join("\n"));
    expect(p.oldPath).toBe("x.sql");
    expect(p.hunks[0]!.lines.map((l) => [l.kind, l.content])).toEqual([
      ["del", "-- a comment"],
      ["add", "++ counter"],
      ["context", "ok"],
    ]);
  });

  it("handles CRLF line endings, git headers and empty input", () => {
    const p = parsePatch("diff --git a/a b/a\r\nindex 1..2 100644\r\n--- a/a\r\n+++ b/a\r\n@@ -1 +1 @@\r\n-x\r\n+y\r\n");
    expect(p.hunks[0]!.lines.map((l) => l.content)).toEqual(["x", "y"]);
    expect(parsePatch("")).toEqual({ oldPath: null, newPath: null, hunks: [], additions: 0, deletions: 0 });
  });
});

describe("toSplitRows", () => {
  it("aligns deletions with the additions that follow them", () => {
    const rows = toSplitRows(parsePatch(modifiedPatch).hunks[0]!.lines);
    expect(rows.map((r) => [r.left?.content ?? null, r.right?.content ?? null])).toEqual([
      ["const a = 1;", "const a = 1;"],
      ["const b = 2;", "const b = 3;"],
      [null, "const c = 4;"],
      ["export default a;", "export default a;"],
      ["// end", "// end"],
    ]);
  });
});
