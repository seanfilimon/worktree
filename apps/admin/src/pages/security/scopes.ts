import type { IamAction } from "@/api/security";

/** Used when GET /iam/actions is unavailable (older servers). */
export const FALLBACK_ACTIONS: IamAction[] = [
  { action: "tenant:read", area: "tenant", description: "View tenants and members" },
  { action: "tenant:admin", area: "tenant", description: "Manage members and tenant settings" },
  { action: "tree:read", area: "tree", description: "Read trees, history and files" },
  { action: "tree:write", area: "tree", description: "Push, stage and tag" },
  { action: "tree:create", area: "tree", description: "Create trees" },
  { action: "tree:admin", area: "tree", description: "Protections, licenses, settings, delete" },
  { action: "branch:create", area: "branch", description: "Create branches" },
  { action: "branch:delete", area: "branch", description: "Delete branches" },
  { action: "branch:push", area: "branch", description: "Push to a specific branch" },
  { action: "mr:create", area: "mr", description: "Open merge requests" },
  { action: "mr:approve", area: "mr", description: "Approve merge requests" },
  { action: "mr:merge", area: "mr", description: "Merge merge requests" },
  { action: "release:create", area: "release", description: "Create and edit releases, upload assets" },
  { action: "release:delete", area: "release", description: "Delete releases and assets" },
  { action: "ci:report", area: "ci", description: "Report CI check results for snapshots" },
  { action: "iam:read", area: "iam", description: "View teams, roles, policies and grants" },
  { action: "iam:admin", area: "iam", description: "Manage teams, roles, policies and grants" },
  { action: "server:admin", area: "server", description: "Server administration" },
];

export interface ActionArea {
  area: string;
  actions: IamAction[];
}

/** Groups actions by area, keeping the server's order. */
export function groupActions(actions: IamAction[]): ActionArea[] {
  const groups = new Map<string, IamAction[]>();
  for (const a of actions) {
    const area = a.area || a.action.split(":")[0] || "other";
    groups.set(area, [...(groups.get(area) ?? []), a]);
  }
  return [...groups].map(([area, list]) => ({ area, actions: list }));
}

/**
 * Normalises a scope selection: a checked `area:*` subsumes that area's
 * concrete actions. Sorted for stable payloads.
 */
export function normalizeScopes(selected: Iterable<string>): string[] {
  const set = new Set(selected);
  if (set.has("*")) return ["*"];
  const wildcards = new Set([...set].filter((s) => s.endsWith(":*")).map((s) => s.slice(0, -2)));
  return [...set].filter((s) => s.endsWith(":*") || !wildcards.has(s.split(":")[0] ?? "")).sort();
}
