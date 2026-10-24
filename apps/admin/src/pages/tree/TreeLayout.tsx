import { useQueryClient } from "@tanstack/react-query";
import { NavLink, Outlet, useParams } from "react-router";
import { keys, useTree } from "@/api/queries";
import type { ServerEvent } from "@/api/types";
import { useServerEvents } from "@/api/useServerEvents";
import { canMaintain, useAuth } from "@/auth/context";
import { PageHeader } from "@/components/PageHeader";
import { StateBadge } from "@/components/legacy/Badge";
import { ErrorState, Skeleton } from "@/components/legacy/States";
import { cn } from "@/lib/cn";
import { treePath } from "@/lib/paths";
import type { TreeContext } from "./context";

const TABS: { to: string; label: string; count?: "branch_count" | "staged_count" | "open_mr_count" }[] = [
  { to: "branches", label: "Branches", count: "branch_count" },
  { to: "history", label: "History" },
  { to: "files", label: "Files" },
  { to: "staged", label: "Staged WIP", count: "staged_count" },
  { to: "tags", label: "Tags" },
  { to: "merge-requests", label: "Merge requests", count: "open_mr_count" },
  { to: "protections", label: "Protections" },
  { to: "licenses", label: "Licenses" },
  { to: "settings", label: "Settings" },
];

/** Query-key suffixes to refresh for each event type. */
const INVALIDATE: Record<string, string[]> = {
  "snapshot.staged": ["staged"],
  "staged.cleared": ["staged"],
  "branch.pushed": ["branches", "snapshots", "staged", "mrs", "mr"],
  "branch.created": ["branches"],
  "tag.pushed": ["tags"],
  "mr.opened": ["mrs"],
  "mr.approved": ["mrs", "mr"],
  "mr.merged": ["mrs", "mr", "branches", "snapshots"],
  "mr.closed": ["mrs", "mr"],
};

export function TreeLayout() {
  const { tenant = "", tree = "" } = useParams();
  const summary = useTree(tenant, tree);
  const queryClient = useQueryClient();
  const { roleIn, isAdmin } = useAuth();
  const treeId = summary.data?.tree.id;

  const live = useServerEvents({ tenant, tree }, (e: ServerEvent) => {
    if (treeId && e.tree_id && e.tree_id !== treeId) return;
    const suffixes = INVALIDATE[e.type];
    if (!suffixes) return;
    for (const s of suffixes) void queryClient.invalidateQueries({ queryKey: ["tree", tenant, tree, s] });
    void queryClient.invalidateQueries({ queryKey: keys.tree(tenant, tree), exact: true });
  });

  const breadcrumbs = [{ label: "Trees", to: "/trees" }, { label: tenant, to: `/tenants/${encodeURIComponent(tenant)}` }, { label: tree }];

  if (summary.isError) {
    return (
      <>
        <PageHeader title={tree} breadcrumbs={breadcrumbs} />
        <ErrorState error={summary.error} onRetry={() => void summary.refetch()} />
      </>
    );
  }

  const s = summary.data;
  const ctx: TreeContext | undefined = s && {
    tenant,
    tree,
    summary: s,
    live,
    canManage: canMaintain(roleIn(s.tree.tenant_id), isAdmin),
  };

  return (
    <>
      <PageHeader
        title={
          <span className="font-mono">
            <span className="text-muted-foreground">{tenant}/</span>
            {tree}
          </span>
        }
        documentTitle={`${tenant}/${tree}`}
        breadcrumbs={breadcrumbs}
        description={
          s ? (
            <span className="flex flex-wrap items-center gap-2">
              <StateBadge value={s.tree.visibility} />
              {s.tree.description && <span>{s.tree.description}</span>}
              <span className="text-subtle">
                {s.snapshot_count} snapshots · default <span className="font-mono">{s.tree.default_branch}</span>
              </span>
            </span>
          ) : (
            <Skeleton className="h-4 w-64" />
          )
        }
      />
      <nav aria-label="Tree sections" className="mb-5 overflow-x-auto border-b border-border [scrollbar-width:none]">
        <ul className="flex min-w-max gap-1">
          {TABS.map((t) => (
            <li key={t.to}>
              <NavLink
                to={treePath(tenant, tree, t.to)}
                className={({ isActive }) =>
                  cn(
                    "-mb-px flex items-center gap-1.5 border-b-2 px-2.5 py-2 text-sm font-medium whitespace-nowrap transition-colors",
                    isActive ? "border-foreground text-fg" : "border-transparent text-muted-foreground hover:text-fg",
                  )
                }
              >
                {t.label}
                {t.count && s && s[t.count] > 0 && (
                  <span className="rounded-full bg-surface-2 px-1.5 text-[11px] text-muted-foreground tabular-nums">{s[t.count]}</span>
                )}
              </NavLink>
            </li>
          ))}
        </ul>
      </nav>
      {ctx ? (
        <Outlet context={ctx} />
      ) : (
        <div role="status" aria-label="Loading tree" className="flex flex-col gap-2">
          <Skeleton className="h-8 w-full" />
          <Skeleton className="h-8 w-full" />
          <Skeleton className="h-8 w-2/3" />
        </div>
      )}
    </>
  );
}
