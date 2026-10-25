import { useQueryClient } from "@tanstack/react-query";
import {
  Building2,
  FolderTree,
  GitBranch,
  GitPullRequest,
  Globe,
  History,
  LayoutDashboard,
  Layers,
  Lock,
  Scale,
  Settings,
  ShieldCheck,
  Tag,
  type LucideIcon,
} from "lucide-react";
import { Fragment, useEffect } from "react";
import { Link, Outlet, useLocation, useParams } from "react-router";
import { keys, useTree } from "@/api/queries";
import type { ServerEvent, TreeSummary, Visibility } from "@/api/types";
import { useServerEvents } from "@/api/useServerEvents";
import { canMaintain, useAuth } from "@/auth/context";
import { Badge } from "@/components/ui/badge";
import { Breadcrumb, BreadcrumbItem, BreadcrumbLink, BreadcrumbList, BreadcrumbPage, BreadcrumbSeparator } from "@/components/ui/breadcrumb";
import { Skeleton } from "@/components/ui/skeleton";
import { formatNumber } from "@/lib/format";
import { tenantPath, treePath } from "@/lib/paths";
import { cn } from "@/lib/utils";
import type { TreeContext } from "./context";
import { LoadingRows, QueryError } from "./history/shared";

type CountKey = "branch_count" | "snapshot_count" | "staged_count" | "open_mr_count";

interface TabDef {
  to: string;
  label: string;
  icon: LucideIcon;
  count?: CountKey;
  /** Extra sub-paths that keep this tab highlighted. */
  also?: string[];
}

const TABS: TabDef[] = [
  { to: "overview", label: "Overview", icon: LayoutDashboard },
  { to: "history", label: "History", icon: History, count: "snapshot_count", also: ["snapshots", "compare"] },
  { to: "branches", label: "Branches", icon: GitBranch, count: "branch_count" },
  { to: "tags", label: "Tags", icon: Tag },
  { to: "files", label: "Files", icon: FolderTree },
  { to: "staged", label: "Staged WIP", icon: Layers, count: "staged_count" },
  { to: "merge-requests", label: "Merge requests", icon: GitPullRequest, count: "open_mr_count" },
  { to: "protections", label: "Protections", icon: ShieldCheck },
  { to: "licenses", label: "Licenses", icon: Scale },
  { to: "settings", label: "Settings", icon: Settings },
];

const ALL_TREE_DATA = [
  "branches",
  "snapshots",
  "staged",
  "staged-summary",
  "mrs",
  "mr",
  "compare",
  "file-history",
  "stats",
];

/** Query-key suffixes (under ["tree", tenant, tree]) to refresh per event type. */
const INVALIDATE: Record<string, string[]> = {
  "snapshot.staged": ["staged", "staged-summary"],
  "staged.cleared": ["staged", "staged-summary"],
  "branch.pushed": ALL_TREE_DATA,
  "branch.created": ["branches", "compare"],
  "tag.pushed": ["tags", "compare"],
  "mr.opened": ["mrs"],
  "mr.approved": ["mrs", "mr"],
  "mr.merged": ALL_TREE_DATA,
  "mr.closed": ["mrs", "mr"],
};

const VISIBILITY_ICON: Record<Visibility, LucideIcon> = { private: Lock, internal: Building2, public: Globe };

function VisibilityBadge({ visibility }: { visibility: Visibility }) {
  const Icon = VISIBILITY_ICON[visibility];
  return (
    <Badge variant={visibility === "public" ? "default" : "outline"} className="capitalize">
      <Icon aria-hidden="true" />
      {visibility}
    </Badge>
  );
}

function QuickStats({ s }: { s: TreeSummary }) {
  const items: [string, number][] = [
    ["snapshots", s.snapshot_count],
    ["branches", s.branch_count],
    ["staged", s.staged_count],
    ["open merge requests", s.open_mr_count],
  ];
  return (
    <dl className="flex flex-wrap gap-x-4 gap-y-1 text-xs text-muted-foreground">
      {items.map(([label, n]) => (
        <div key={label}>
          <dt className="sr-only">{label}</dt>
          <dd>
            <span className="font-medium text-foreground tabular-nums">{formatNumber(n)}</span> <span aria-hidden="true">{label}</span>
          </dd>
        </div>
      ))}
      <div className="flex items-baseline gap-1">
        <dt>default</dt>
        <dd className="font-mono text-foreground">{s.tree.default_branch}</dd>
      </div>
    </dl>
  );
}

export function TreeLayout() {
  const { tenant = "", tree = "" } = useParams();
  const { pathname } = useLocation();
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
    void queryClient.invalidateQueries({ queryKey: ["events", "recent"] });
  });

  useEffect(() => {
    document.title = `${tenant}/${tree} · W0rkTree Admin`;
  }, [tenant, tree]);

  const s = summary.data;
  const base = treePath(tenant, tree);
  const section = pathname.startsWith(`${base}/`) ? (pathname.slice(base.length + 1).split("/", 1)[0] ?? "") : "";

  const ctx: TreeContext | undefined = s && {
    tenant,
    tree,
    summary: s,
    live,
    canManage: canMaintain(roleIn(s.tree.tenant_id), isAdmin),
  };

  return (
    <div className="flex flex-col gap-5">
      <header className="flex flex-col gap-2">
        <Breadcrumb>
          <BreadcrumbList>
            {[
              { label: "Trees", to: "/trees" },
              { label: tenant, to: tenantPath(tenant) },
            ].map((c) => (
              <Fragment key={c.to}>
                <BreadcrumbItem>
                  <BreadcrumbLink asChild>
                    <Link to={c.to}>{c.label}</Link>
                  </BreadcrumbLink>
                </BreadcrumbItem>
                <BreadcrumbSeparator />
              </Fragment>
            ))}
            <BreadcrumbItem>
              <BreadcrumbPage>{tree}</BreadcrumbPage>
            </BreadcrumbItem>
          </BreadcrumbList>
        </Breadcrumb>
        <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
          <h1 className="min-w-0 truncate font-mono text-xl font-semibold tracking-tight">
            <span className="text-muted-foreground">{tenant}/</span>
            {tree}
          </h1>
          {s && <VisibilityBadge visibility={s.tree.visibility} />}
        </div>
        {s ? (
          <>
            {s.tree.description && <p className="text-sm text-muted-foreground">{s.tree.description}</p>}
            <QuickStats s={s} />
          </>
        ) : (
          !summary.isError && <Skeleton className="h-4 w-64" />
        )}
      </header>

      <nav aria-label="Tree sections" className="-mx-1 overflow-x-auto border-b [scrollbar-width:none]">
        <ul className="flex min-w-max gap-1 px-1">
          {TABS.map((t) => {
            const active = section === t.to || (t.also?.includes(section) ?? false);
            const n = t.count && s ? s[t.count] : 0;
            return (
              <li key={t.to}>
                <Link
                  to={treePath(tenant, tree, t.to)}
                  aria-current={active ? "page" : undefined}
                  className={cn(
                    "relative -mb-px flex h-9 items-center gap-1.5 border-b-2 px-2.5 text-sm font-medium whitespace-nowrap transition-colors outline-none focus-visible:ring-[3px] focus-visible:ring-ring/50",
                    active ? "border-foreground text-foreground" : "border-transparent text-muted-foreground hover:text-foreground",
                  )}
                >
                  <t.icon aria-hidden="true" className="size-4" />
                  {t.label}
                  {n > 0 && (
                    <Badge variant="secondary" className="h-4.5 min-w-4.5 px-1 text-[11px] tabular-nums">
                      {formatNumber(n)}
                    </Badge>
                  )}
                </Link>
              </li>
            );
          })}
        </ul>
      </nav>

      {summary.isError ? (
        <QueryError error={summary.error} onRetry={() => void summary.refetch()} />
      ) : ctx ? (
        <Outlet context={ctx} />
      ) : (
        <LoadingRows label="Loading tree" rows={3} />
      )}
    </div>
  );
}
