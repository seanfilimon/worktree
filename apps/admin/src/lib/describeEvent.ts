import type { ServerEvent } from "@/api/types";

const LABELS: Record<string, string> = {
  "snapshot.staged": "staged work in progress",
  "staged.cleared": "cleared staged work",
  "branch.pushed": "pushed",
  "branch.created": "created a branch",
  "tag.pushed": "pushed a tag",
  "tree.created": "created a tree",
  "mr.opened": "opened a merge request",
  "mr.approved": "approved a merge request",
  "mr.merged": "merged a merge request",
  "mr.closed": "closed a merge request",
};

function str(payload: Record<string, unknown>, ...names: string[]): string | undefined {
  for (const n of names) {
    const v = payload[n];
    if (typeof v === "string" && v) return v;
    if (typeof v === "number") return String(v);
  }
  return undefined;
}

/** One-line human summary of an SSE event, robust to unknown payloads. */
export function describeEvent(e: ServerEvent): { action: string; target: string } {
  const p = typeof e.payload === "object" && e.payload !== null ? (e.payload as Record<string, unknown>) : {};
  const action = LABELS[e.type] ?? e.type;
  const branch = str(p, "branch", "branch_name", "target_branch");
  const parts: string[] = [];

  if (e.type.startsWith("mr.")) {
    const n = str(p, "number");
    const title = str(p, "title");
    if (n) parts.push(`!${n}`);
    if (title) parts.push(title);
  } else if (e.type === "tag.pushed") {
    const tag = str(p, "tag", "name");
    if (tag) parts.push(tag);
  } else if (e.type === "tree.created") {
    const name = str(p, "name", "tree");
    if (name) parts.push(name);
  } else if (branch) {
    parts.push(branch);
  }
  const tree = str(p, "tree", "tree_name");
  if (tree && e.type !== "tree.created") parts.unshift(tree);
  return { action, target: parts.join(" · ") };
}
