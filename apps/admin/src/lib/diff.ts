/**
 * Unified-diff parsing for the compare endpoint's per-file patches
 * (`--- a/P` / `+++ b/P` headers, `/dev/null` for added or deleted files).
 */

export type DiffLineKind = "context" | "add" | "del";

export interface DiffLine {
  kind: DiffLineKind;
  /** Line text without the leading "+", "-" or " " marker. */
  content: string;
  /** 1-based line number in the old file (null for additions). */
  oldNo: number | null;
  /** 1-based line number in the new file (null for deletions). */
  newNo: number | null;
  /** Followed by "\ No newline at end of file". */
  noNewline?: boolean;
}

export interface DiffHunk {
  /** The raw "@@ -a,b +c,d @@ section" line. */
  header: string;
  oldStart: number;
  oldLines: number;
  newStart: number;
  newLines: number;
  /** Text after the closing "@@" (often the enclosing function). */
  section: string;
  lines: DiffLine[];
}

export interface ParsedPatch {
  /** Path from the "--- " header; null for /dev/null or when absent. */
  oldPath: string | null;
  /** Path from the "+++ " header; null for /dev/null or when absent. */
  newPath: string | null;
  hunks: DiffHunk[];
  additions: number;
  deletions: number;
}

const HUNK_RE = /^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@ ?(.*)$/;

function headerPath(raw: string): string | null {
  // Strip an optional tab-separated timestamp, then the a/ or b/ prefix.
  const path = raw.split("\t", 1)[0]!.trim();
  if (path === "/dev/null" || path === "") return null;
  return path.replace(/^[ab]\//, "");
}

/**
 * Parses one file's unified diff. Header lines outside hunks (`diff --git`,
 * `index`, `---`, `+++`) are consumed; hunk bodies are read by their declared
 * line counts, so content such as a removed "-- comment" line is never
 * mistaken for a header.
 */
export function parsePatch(text: string): ParsedPatch {
  const result: ParsedPatch = { oldPath: null, newPath: null, hunks: [], additions: 0, deletions: 0 };
  if (!text) return result;

  const lines = text.split("\n");
  if (lines.at(-1) === "") lines.pop();

  let hunk: DiffHunk | null = null;
  let oldLeft = 0;
  let newLeft = 0;
  let oldNo = 0;
  let newNo = 0;

  for (const rawLine of lines) {
    const line = rawLine.endsWith("\r") ? rawLine.slice(0, -1) : rawLine;

    if (hunk && (oldLeft > 0 || newLeft > 0)) {
      const marker = line[0];
      const content = line.slice(1);
      if (marker === "+") {
        hunk.lines.push({ kind: "add", content, oldNo: null, newNo: newNo++ });
        newLeft -= 1;
        result.additions += 1;
        continue;
      }
      if (marker === "-") {
        hunk.lines.push({ kind: "del", content, oldNo: oldNo++, newNo: null });
        oldLeft -= 1;
        result.deletions += 1;
        continue;
      }
      if (marker === " " || line === "") {
        hunk.lines.push({ kind: "context", content, oldNo: oldNo++, newNo: newNo++ });
        oldLeft -= 1;
        newLeft -= 1;
        continue;
      }
      if (marker !== "\\") {
        // Malformed or truncated hunk: stop reading it as hunk content.
        oldLeft = 0;
        newLeft = 0;
      }
    }

    if (line.startsWith("\\")) {
      const last = hunk?.lines.at(-1);
      if (last) last.noNewline = true;
      continue;
    }

    const m = HUNK_RE.exec(line);
    if (m) {
      const oldStart = Number(m[1]);
      const newStart = Number(m[3]);
      hunk = {
        header: line,
        oldStart,
        oldLines: m[2] === undefined ? 1 : Number(m[2]),
        newStart,
        newLines: m[4] === undefined ? 1 : Number(m[4]),
        section: m[5] ?? "",
        lines: [],
      };
      result.hunks.push(hunk);
      oldLeft = hunk.oldLines;
      newLeft = hunk.newLines;
      oldNo = oldStart;
      newNo = newStart;
      continue;
    }

    if (line.startsWith("--- ")) result.oldPath = headerPath(line.slice(4));
    else if (line.startsWith("+++ ")) result.newPath = headerPath(line.slice(4));
  }

  return result;
}

/** One row of a side-by-side diff; either side may be empty. */
export interface SplitRow {
  left: DiffLine | null;
  right: DiffLine | null;
}

/**
 * Pairs a hunk's lines for split view: context lines appear on both sides,
 * and each run of deletions is aligned with the additions that follow it.
 */
export function toSplitRows(lines: readonly DiffLine[]): SplitRow[] {
  const rows: SplitRow[] = [];
  let i = 0;
  while (i < lines.length) {
    const line = lines[i]!;
    if (line.kind === "context") {
      rows.push({ left: line, right: line });
      i += 1;
      continue;
    }
    const dels: DiffLine[] = [];
    const adds: DiffLine[] = [];
    while (i < lines.length && lines[i]!.kind === "del") dels.push(lines[i++]!);
    while (i < lines.length && lines[i]!.kind === "add") adds.push(lines[i++]!);
    for (let k = 0; k < Math.max(dels.length, adds.length); k++) {
      rows.push({ left: dels[k] ?? null, right: adds[k] ?? null });
    }
  }
  return rows;
}
