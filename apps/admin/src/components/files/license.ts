import { pathMatch } from "@/lib/glob";

/**
 * License path globs, mirroring services/server-go/internal/license/glob.go:
 * anchored at the tree root, "**" (a whole segment) spans any depth, other
 * segments use path.Match, and a pattern that matches a directory covers
 * everything beneath it ("vendor" == "vendor/" == "vendor/**").
 */
function normalizeGlob(raw: string): string[] {
  let s = raw.replace(/\\/g, "/");
  while (s.startsWith("./") || s.startsWith("/")) s = s.startsWith("./") ? s.slice(2) : s.slice(1);
  const dir = s.endsWith("/");
  const segs: string[] = [];
  for (const seg of s.split("/")) {
    if (seg === "" || seg === ".") continue;
    if (seg === "**" && segs[segs.length - 1] === "**") continue;
    segs.push(seg);
  }
  if (dir && segs.length && segs[segs.length - 1] !== "**") segs.push("**");
  return segs.length ? segs : ["**"];
}

function malformedSegment(seg: string): boolean {
  let open = false;
  for (let i = 0; i < seg.length; i++) {
    const c = seg.charAt(i);
    if (c === "\\") i++;
    else if (c === "[") {
      if (open) return true;
      open = true;
    } else if (c === "]") open = false;
  }
  return open;
}

/** Why the server would reject the glob, or undefined when it is valid. */
export function licenseGlobError(raw: string): string | undefined {
  if (!raw.trim()) return "Required, e.g. vendor/** or **/*.rs.";
  if (raw.length > 4096) return "At most 4096 characters.";
  const segs = normalizeGlob(raw);
  if (segs.includes("..")) return "Must not contain “..”.";
  const bad = segs.find((s) => s !== "**" && malformedSegment(s));
  return bad ? `Malformed segment “${bad}”.` : undefined;
}

/** Whether `glob` covers the tree-relative file `path`. */
export function licenseGlobMatch(glob: string, path: string): boolean {
  if (licenseGlobError(glob)) return false;
  const g = normalizeGlob(glob);
  const p = path.split("/").filter(Boolean);
  const memo = new Map<number, boolean>();
  const m = (i: number, j: number): boolean => {
    if (i === g.length) return true; // the path itself or a descendant
    const key = i * (p.length + 1) + j;
    const cached = memo.get(key);
    if (cached !== undefined) return cached;
    const seg = g[i] ?? "";
    const ok = seg === "**" ? m(i + 1, j) || (j < p.length && m(i, j + 1)) : j < p.length && pathMatch(seg, p[j] ?? "") && m(i + 1, j + 1);
    memo.set(key, ok);
    return ok;
  };
  return m(0, 0);
}

const SPDX_ID = /^(?:DocumentRef-[A-Za-z0-9.-]+:)?(?:LicenseRef-)?[A-Za-z0-9.-]+\+?$/;

/** Syntactic check of an SPDX license expression (ids are not checked against the list). */
export function spdxError(raw: string): string | undefined {
  const s = raw.trim();
  if (!s) return "Required, e.g. MIT or Apache-2.0.";
  if (s.length > 1024) return "At most 1024 characters.";
  const tokens = s.replace(/[()]/g, " $& ").split(/\s+/).filter(Boolean);
  let depth = 0;
  /** Whether an operand (id or closed group) was just read. */
  let operand = false;
  let afterWith = false;
  for (const t of tokens) {
    if (t === "(") {
      if (operand) return "Expected AND, OR or WITH before “(”.";
      depth++;
    } else if (t === ")") {
      if (!operand || --depth < 0) return "Unbalanced parentheses.";
    } else if (t === "AND" || t === "OR" || t === "WITH") {
      if (!operand) return `Unexpected ${t}.`;
      operand = false;
      afterWith = t === "WITH";
      continue;
    } else if (/^(and|or|with)$/i.test(t)) {
      return `Operators are upper case: ${t.toUpperCase()}.`;
    } else {
      if (operand) return `Expected AND, OR or WITH before “${t}”.`;
      if (!SPDX_ID.test(t)) return `“${t}” is not a valid SPDX identifier.`;
      operand = true;
      afterWith = false;
      continue;
    }
    operand = t === ")";
  }
  if (afterWith || !operand) return "The expression is incomplete.";
  return depth === 0 ? undefined : "Unbalanced parentheses.";
}

/** Frequently used SPDX identifiers offered as suggestions. */
export const COMMON_SPDX: readonly { id: string; name: string }[] = [
  { id: "MIT", name: "MIT License" },
  { id: "Apache-2.0", name: "Apache License 2.0" },
  { id: "BSD-3-Clause", name: "BSD 3-Clause" },
  { id: "BSD-2-Clause", name: "BSD 2-Clause" },
  { id: "ISC", name: "ISC License" },
  { id: "MPL-2.0", name: "Mozilla Public License 2.0" },
  { id: "GPL-3.0-only", name: "GNU GPL v3.0 only" },
  { id: "GPL-3.0-or-later", name: "GNU GPL v3.0 or later" },
  { id: "GPL-2.0-only", name: "GNU GPL v2.0 only" },
  { id: "LGPL-3.0-only", name: "GNU LGPL v3.0 only" },
  { id: "LGPL-2.1-only", name: "GNU LGPL v2.1 only" },
  { id: "AGPL-3.0-only", name: "GNU AGPL v3.0 only" },
  { id: "EPL-2.0", name: "Eclipse Public License 2.0" },
  { id: "BSL-1.0", name: "Boost Software License 1.0" },
  { id: "Unlicense", name: "The Unlicense" },
  { id: "CC0-1.0", name: "Creative Commons Zero 1.0" },
  { id: "CC-BY-4.0", name: "Creative Commons Attribution 4.0" },
  { id: "Zlib", name: "zlib License" },
  { id: "Proprietary", name: "All rights reserved" },
];
