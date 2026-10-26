/** Line helpers for review comments and suggestions (§15.7). */
import { parsePatch } from "@/lib/diff";
import type { FileDiff } from "@/api/types";

/** Lines of a text without the trailing empty line a final newline leaves (CR stripped). */
export function splitLines(text: string): string[] {
  const lines = text.split("\n").map((l) => (l.endsWith("\r") ? l.slice(0, -1) : l));
  if (lines.length > 0 && lines.at(-1) === "") lines.pop();
  return lines;
}

/** New-side (source tip) line texts visible in each file's patch: path → line → text. */
export function newSideLines(files: readonly FileDiff[]): Map<string, Map<number, string>> {
  const out = new Map<string, Map<number, string>>();
  for (const f of files) {
    if (f.binary || f.too_large || !f.patch) continue;
    const m = new Map<number, string>();
    for (const h of parsePatch(f.patch).hunks) {
      for (const l of h.lines) if (l.newNo !== null) m.set(l.newNo, l.content);
    }
    out.set(f.path, m);
  }
  return out;
}
