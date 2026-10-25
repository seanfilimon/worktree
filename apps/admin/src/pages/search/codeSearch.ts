import type { CodeSearchHit } from "@/api/v06";
import { treePath } from "@/lib/paths";

/** Link to a file (pinned to the searched snapshot) at a line. */
export function codeHitHref(h: Pick<CodeSearchHit, "tenant" | "tree" | "snapshot_id" | "path">, line?: number): string {
  const q = new URLSearchParams();
  if (h.snapshot_id) q.set("snapshot", h.snapshot_id);
  q.set("path", h.path);
  return `${treePath(h.tenant, h.tree, "files")}?${q.toString()}${line ? `#L${line}` : ""}`;
}

/** Splits `text` into plain / matched parts for the query (literal or regex). */
export function highlightParts(text: string, q: string, regex: boolean, caseSensitive: boolean): { text: string; match: boolean }[] {
  if (!q) return [{ text, match: false }];
  let re: RegExp;
  try {
    re = new RegExp(regex ? q : q.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"), caseSensitive ? "g" : "gi");
  } catch {
    return [{ text, match: false }];
  }
  const parts: { text: string; match: boolean }[] = [];
  let last = 0;
  for (const m of text.matchAll(re)) {
    const i = m.index;
    if (m[0] === "") continue;
    if (i > last) parts.push({ text: text.slice(last, i), match: false });
    parts.push({ text: m[0], match: true });
    last = i + m[0].length;
  }
  if (last < text.length) parts.push({ text: text.slice(last), match: false });
  return parts.length ? parts : [{ text, match: false }];
}

export interface FileGroup {
  key: string;
  tenant: string;
  tree: string;
  branch: string;
  snapshot_id: string;
  path: string;
  hits: CodeSearchHit[];
}

/** Hits grouped by tree, then file, in server order. */
export function groupHits(hits: readonly CodeSearchHit[]): { tree: string; files: FileGroup[] }[] {
  const trees = new Map<string, Map<string, FileGroup>>();
  for (const h of hits) {
    const treeKey = `${h.tenant}/${h.tree}`;
    let files = trees.get(treeKey);
    if (!files) {
      files = new Map<string, FileGroup>();
      trees.set(treeKey, files);
    }
    const fileKey = `${treeKey}:${h.path}`;
    let g = files.get(fileKey);
    if (!g) {
      g = { key: fileKey, tenant: h.tenant, tree: h.tree, branch: h.branch, snapshot_id: h.snapshot_id, path: h.path, hits: [] };
      files.set(fileKey, g);
    }
    g.hits.push(h);
  }
  return [...trees].map(([tree, files]) => ({ tree, files: [...files.values()] }));
}
