import { Upload } from "lucide-react";
import { Link } from "react-router";
import { useAudit, useTenants } from "@/api/queries";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { treePath } from "@/lib/paths";
import { EmptyState, ErrorState, SkeletonRows } from "./states";
import { TimeAgo } from "./TimeAgo";

/** The last ten `branch.push` audit events the caller may see. */
export function RecentPushes({ className }: { className?: string }) {
  const pushes = useAudit({ action: "branch.push", limit: 10 });
  const tenants = useTenants();
  const slugOf = (id: string) => tenants.data?.find((t) => t.id === id)?.slug;

  return (
    <Card className={className}>
      <CardHeader>
        <CardTitle>Recent pushes</CardTitle>
        <CardDescription>Branches published with wt push</CardDescription>
      </CardHeader>
      <CardContent className="px-0">
        {pushes.isPending ? (
          <SkeletonRows rows={4} label="Loading recent pushes" />
        ) : pushes.isError ? (
          <ErrorState error={pushes.error} onRetry={() => void pushes.refetch()} />
        ) : pushes.data.length === 0 ? (
          <EmptyState icon={Upload} title="No pushes yet">
            Run <code className="font-mono">wt push</code> to publish a branch.
          </EmptyState>
        ) : (
          <ol aria-label="Recent pushes" className="divide-y border-t">
            {pushes.data.map((e) => {
              const tree = e.details?.tree;
              const branch = e.details?.branch ?? e.resource_id;
              const slug = slugOf(e.tenant_id);
              const count = Number(e.details?.count);
              return (
                <li key={e.id} className="flex items-baseline gap-3 px-6 py-2.5 text-[13px]">
                  <span className="min-w-0 flex-1 truncate">
                    <span className="font-medium">{e.actor}</span> pushed <span className="font-mono">{branch}</span>
                    {tree && (
                      <>
                        {" to "}
                        {slug ? (
                          <Link to={treePath(slug, tree)} className="font-medium underline-offset-4 hover:underline">
                            {slug}/{tree}
                          </Link>
                        ) : (
                          <span className="font-medium">{tree}</span>
                        )}
                      </>
                    )}
                    {count > 0 && (
                      <span className="text-muted-foreground">
                        {" "}
                        · {count} {count === 1 ? "snapshot" : "snapshots"}
                      </span>
                    )}
                  </span>
                  <TimeAgo value={e.at} className="shrink-0 text-xs text-muted-foreground" />
                </li>
              );
            })}
          </ol>
        )}
      </CardContent>
    </Card>
  );
}
