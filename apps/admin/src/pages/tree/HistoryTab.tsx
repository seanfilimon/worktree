import { Link, useSearchParams } from "react-router";
import { useSnapshots } from "@/api/queries";
import { Badge } from "@/components/legacy/Badge";
import { Section } from "@/components/legacy/Card";
import { EmptyState, ErrorState, SkeletonRows } from "@/components/legacy/States";
import { RelativeTime } from "@/components/legacy/Time";
import { firstLine, shortId } from "@/lib/format";
import { filesPath } from "@/lib/paths";
import { BranchSelect } from "./BranchSelect";
import { useTreeContext } from "./context";

export function HistoryTab() {
  const { tenant, tree, summary } = useTreeContext();
  const [params, setParams] = useSearchParams();
  const branch = params.get("branch") || summary.tree.default_branch;
  const snapshots = useSnapshots(tenant, tree, branch);

  return (
    <Section
      title="Snapshot history"
      description="Newest first, up to 50 snapshots."
      flush
      actions={
        <BranchSelect
          tenant={tenant}
          tree={tree}
          value={branch}
          onChange={(b) => setParams({ branch: b }, { replace: true })}
        />
      }
    >
      {snapshots.isPending ? (
        <SkeletonRows label="Loading history" />
      ) : snapshots.isError ? (
        <ErrorState error={snapshots.error} onRetry={() => void snapshots.refetch()} />
      ) : snapshots.data.length === 0 ? (
        <EmptyState title="No snapshots on this branch" />
      ) : (
        <ol aria-label={`Snapshots on ${branch}`} className="divide-y divide-border">
          {snapshots.data.map((s) => {
            const parents = s.parents ?? [];
            return (
              <li key={s.id} className="flex flex-wrap items-start gap-x-4 gap-y-1 px-4 py-3 sm:flex-nowrap">
                <div className="min-w-0 flex-1">
                  <p className="flex flex-wrap items-center gap-2">
                    <span className="truncate font-medium">{firstLine(s.message) || <span className="text-subtle italic">no message</span>}</span>
                    {s.auto_generated && (
                      <Badge tone="info" title="Created automatically by the background daemon">
                        auto
                      </Badge>
                    )}
                    {parents.length > 1 && <Badge tone="accent">merge</Badge>}
                  </p>
                  <p className="mt-0.5 text-xs text-muted-foreground">
                    <span className="font-medium text-fg">{s.author}</span> snapshotted <RelativeTime value={s.created_at} />
                    {s.pushed_at && (
                      <>
                        {" "}
                        · pushed <RelativeTime value={s.pushed_at} />
                      </>
                    )}
                  </p>
                </div>
                <Link
                  to={filesPath(tenant, tree, { snapshot: s.id })}
                  className="shrink-0 rounded border border-border px-1.5 py-0.5 font-mono text-xs text-foreground hover:bg-surface-2"
                  title={`Browse files at ${s.id}`}
                  aria-label={`Browse files at snapshot ${shortId(s.id)}`}
                >
                  {shortId(s.id)}
                </Link>
              </li>
            );
          })}
        </ol>
      )}
    </Section>
  );
}
