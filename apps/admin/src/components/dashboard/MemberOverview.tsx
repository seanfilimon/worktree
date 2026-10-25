import { useQueries } from "@tanstack/react-query";
import { Building2, FolderTree } from "lucide-react";
import { Link } from "react-router";
import { useApi } from "@/api/context";
import { keys } from "@/api/queries";
import type { Role, Tenant, TreeSummary } from "@/api/types";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardAction, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { tenantPath, treePath } from "@/lib/paths";
import { EmptyState, ErrorState, SkeletonRows } from "./states";
import { TimeAgo } from "./TimeAgo";

const MAX_TREES = 8;

interface MemberOverviewProps {
  memberships: { tenant: Tenant; role: Role }[];
}

/** "Your tenants" and "Your trees" for accounts without server-wide stats. */
export function MemberOverview({ memberships }: MemberOverviewProps) {
  const api = useApi();
  const results = useQueries({
    queries: memberships.map(({ tenant }) => ({
      queryKey: keys.tenantTrees(tenant.slug),
      queryFn: () => api.tenants.trees(tenant.slug),
    })),
  });
  const trees = results
    .flatMap((r, i) => (r.data ?? []).map((summary) => ({ tenant: memberships[i]!.tenant.slug, summary })))
    .sort((a, b) => b.summary.tree.updated_at.localeCompare(a.summary.tree.updated_at));
  const loading = results.some((r) => r.isPending);
  const failed = results.find((r) => r.isError);

  return (
    <div className="grid gap-5 lg:grid-cols-5">
      <Card className="lg:col-span-2">
        <CardHeader>
          <CardTitle>Your tenants</CardTitle>
          <CardDescription>
            Member of {memberships.length} {memberships.length === 1 ? "tenant" : "tenants"}. Server-wide statistics are
            visible to server administrators.
          </CardDescription>
        </CardHeader>
        <CardContent className="px-0">
          {memberships.length === 0 ? (
            <EmptyState icon={Building2} title="No tenants yet" action={<CreateLink to="/tenants?new=1" label="Create a tenant" />}>
              Create an organization or ask an owner to invite you.
            </EmptyState>
          ) : (
            <ul aria-label="Your tenants" className="divide-y border-t">
              {memberships.map(({ tenant, role }) => (
                <li key={tenant.id} className="flex items-center gap-3 px-6 py-2.5 text-[13px]">
                  <Link to={tenantPath(tenant.slug)} className="min-w-0 flex-1 truncate underline-offset-4 hover:underline">
                    <span className="font-medium">{tenant.name}</span>{" "}
                    <span className="font-mono text-xs text-muted-foreground">{tenant.slug}</span>
                  </Link>
                  <Badge variant={role === "owner" || role === "admin" ? "default" : "outline"}>{role}</Badge>
                </li>
              ))}
            </ul>
          )}
        </CardContent>
      </Card>

      <Card className="lg:col-span-3">
        <CardHeader>
          <CardTitle>Your trees</CardTitle>
          <CardDescription>Most recently updated first</CardDescription>
          {trees.length > MAX_TREES && (
            <CardAction>
              <Button asChild size="sm" variant="ghost">
                <Link to="/trees">View all</Link>
              </Button>
            </CardAction>
          )}
        </CardHeader>
        <CardContent className="px-0">
          {failed ? (
            <ErrorState error={failed.error} onRetry={() => void failed.refetch()} />
          ) : loading ? (
            <SkeletonRows rows={4} label="Loading your trees" />
          ) : trees.length === 0 ? (
            <EmptyState icon={FolderTree} title="No trees yet" action={<CreateLink to="/trees?new=1" label="Create a tree" />}>
              Trees you can read appear here.
            </EmptyState>
          ) : (
            <ul aria-label="Your trees" className="divide-y border-t">
              {trees.slice(0, MAX_TREES).map(({ tenant, summary }) => (
                <TreeRow key={summary.tree.id} tenant={tenant} summary={summary} />
              ))}
            </ul>
          )}
        </CardContent>
      </Card>
    </div>
  );
}

function TreeRow({ tenant, summary }: { tenant: string; summary: TreeSummary }) {
  const { tree } = summary;
  const name = `${tenant}/${tree.name}`;
  const quick: [string, string][] = [
    ["Files", "files"],
    ["History", "history"],
    [`Merge requests (${summary.open_mr_count})`, "merge-requests"],
  ];
  return (
    <li className="flex flex-col gap-1.5 px-6 py-2.5 text-[13px] sm:flex-row sm:items-center sm:gap-3">
      <div className="min-w-0 flex-1">
        <Link to={treePath(tenant, tree.name)} className="font-mono font-medium underline-offset-4 hover:underline">
          {name}
        </Link>
        <p className="truncate text-xs text-muted-foreground">
          {summary.branch_count} {summary.branch_count === 1 ? "branch" : "branches"} · {summary.snapshot_count} snapshots · updated{" "}
          <TimeAgo value={tree.updated_at} />
        </p>
      </div>
      <nav aria-label={`Quick links for ${name}`} className="flex flex-wrap gap-1">
        {quick.map(([label, section]) => (
          <Button key={section} asChild size="xs" variant="outline">
            <Link to={treePath(tenant, tree.name, section)}>{label}</Link>
          </Button>
        ))}
      </nav>
    </li>
  );
}

function CreateLink({ to, label }: { to: string; label: string }) {
  return (
    <Button asChild size="sm">
      <Link to={to}>{label}</Link>
    </Button>
  );
}
