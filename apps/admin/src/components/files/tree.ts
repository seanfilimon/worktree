import type { SnapshotFile } from "@/api/types";

export interface DirNode {
  kind: "dir";
  name: string;
  /** Full '/'-separated path. */
  path: string;
  children: TreeNode[];
}

export interface FileNode {
  kind: "file";
  name: string;
  path: string;
  file: SnapshotFile;
}

export type TreeNode = DirNode | FileNode;

const byName = (a: TreeNode, b: TreeNode) => (a.kind === b.kind ? a.name.localeCompare(b.name) : a.kind === "dir" ? -1 : 1);

/** Nested directory tree of a flat manifest; directories first, then files, by name. */
export function buildTree(files: readonly SnapshotFile[]): TreeNode[] {
  const root: DirNode = { kind: "dir", name: "", path: "", children: [] };
  const dirs = new Map<string, DirNode>([["", root]]);

  const dirFor = (path: string): DirNode => {
    const existing = dirs.get(path);
    if (existing) return existing;
    const slash = path.lastIndexOf("/");
    const parent = dirFor(slash === -1 ? "" : path.slice(0, slash));
    const node: DirNode = { kind: "dir", name: path.slice(slash + 1), path, children: [] };
    parent.children.push(node);
    dirs.set(path, node);
    return node;
  };

  for (const f of files) {
    const slash = f.path.lastIndexOf("/");
    dirFor(slash === -1 ? "" : f.path.slice(0, slash)).children.push({
      kind: "file",
      name: f.path.slice(slash + 1),
      path: f.path,
      file: f,
    });
  }
  for (const d of dirs.values()) d.children.sort(byName);
  return root.children;
}

/** Every ancestor directory of `path`, outermost first: "a/b/c" -> ["a", "a/b"]. */
export function ancestorDirs(path: string): string[] {
  const parts = path.split("/").filter(Boolean);
  return parts.slice(0, -1).map((_, i) => parts.slice(0, i + 1).join("/"));
}

export interface FuzzyMatch {
  file: SnapshotFile;
  /** Indices into `file.path` of the matched characters. */
  indices: number[];
  score: number;
}

/**
 * Subsequence match of `query` in `text` (case-insensitive). Rewards runs of
 * consecutive characters, matches at word starts and matches in the base
 * name. Returns null when not every query character is found in order.
 */
export function fuzzyMatch(query: string, text: string): { score: number; indices: number[] } | null {
  const q = query.toLowerCase().replace(/\s+/g, "");
  if (!q) return { score: 0, indices: [] };
  const t = text.toLowerCase();
  const baseStart = text.lastIndexOf("/") + 1;
  const indices: number[] = [];
  let score = 0;
  let from = 0;
  for (const ch of q) {
    const at = t.indexOf(ch, from);
    if (at === -1) return null;
    const prev = indices[indices.length - 1];
    if (prev !== undefined && at === prev + 1) score += 5;
    const before = at > 0 ? text.charAt(at - 1) : "/";
    if (/[/_.\-\s]/.test(before) || (/[a-z]/.test(before) && /[A-Z]/.test(text.charAt(at)))) score += 3;
    if (at >= baseStart) score += 2;
    score -= Math.min(at - from, 10) * 0.1;
    indices.push(at);
    from = at + 1;
  }
  // Prefer shorter paths among equal matches.
  return { score: score - text.length * 0.01, indices };
}

/** Files matching `query`, best first, at most `limit`. */
export function filterFiles(files: readonly SnapshotFile[], query: string, limit = 200): FuzzyMatch[] {
  const out: FuzzyMatch[] = [];
  for (const file of files) {
    const m = fuzzyMatch(query, file.path);
    if (m) out.push({ file, ...m });
  }
  out.sort((a, b) => b.score - a.score || a.file.path.localeCompare(b.file.path));
  return out.slice(0, limit);
}
