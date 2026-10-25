import { ArrowLeftRight, GitCommitHorizontal } from "lucide-react";
import { Link, useSearchParams } from "react-router";
import { useCompare } from "@/api/queries";
import type { CompareResult } from "@/api/types";
import { DiffViewer } from "@/components/diff/DiffViewer";
import { ChangeCounts } from "@/components/diff/parts";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { firstLine, formatNumber, shortId } from "@/lib/format";
import { treePath } from "@/lib/paths";
import { useTreeContext } from "./context";
import { RevisionSelect } from "./history/RevisionSelect";
import { AuthorAvatar, EmptyState, LoadingRows, QueryError, RelativeTime, SnapshotBadges } from "./history/shared";

function Summary({ result }: { result: CompareResult }) {
  const n = result.stats.files;
  return (
    <p className="flex flex-wrap items-center gap-x-3 gap-y-1 text-sm">
      <span>
        <span className="font-medium">{formatNumber(result.snapshots.length)}</span> {result.snapshots.length === 1 ? "snapshot" : "snapshots"}
      </span>
      <span>
        <span className="font-medium">{formatNumber(n)}</span> {n === 1 ? "file" : "files"} changed
      </span>
      <ChangeCounts additions={result.stats.additions} deletions={result.stats.deletions} className="text-sm" />
      {result.merge_base && (
        <span className="text-muted-foreground">
          merge base <span className="font-mono text-foreground">{shortId(result.merge_base)}</span>
        </span>
      )}
    </p>
  );
}

export function CompareView() {
  const { tenant, tree, summary } = useTreeContext();
  const [params, setParams] = useSearchParams();
  const defaultBranch = summary.tree.default_branch;
  const to = params.get("to") ?? defaultBranch;
  const from = params.get("from") ?? "";
  const result = useCompare(tenant, tree, to, from);

  const update = (next: { from: string; to: string }) => {
    const q = new URLSearchParams();
    if (next.from) q.set("from", next.from);
    if (next.to) q.set("to", next.to);
    setParams(q);
  };

  const snapshotPath = (id: string) => treePath(tenant, tree, `snapshots/${encodeURIComponent(id)}`);

  return (
    <div className="flex flex-col gap-4">
      <Card>
        <CardHeader>
          <CardTitle>Compare revisions</CardTitle>
          <CardDescription>
            Pick a base and a target: a branch, a tag or a snapshot. Without a base, the target is compared with its first parent.
          </CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-4">
          <div className="flex flex-col gap-3 sm:flex-row sm:items-end">
            <RevisionSelect
              tenant={tenant}
              tree={tree}
              branch={defaultBranch}
              label="Base (from)"
              emptyLabel="Parent of target"
              value={from}
              onChange={(v) => update({ from: v, to })}
            />
            <Button
              variant="outline"
              size="icon"
              aria-label="Swap base and target"
              disabled={!from || !to}
              onClick={() => update({ from: to, to: from })}
              className="self-start sm:self-auto"
            >
              <ArrowLeftRight aria-hidden="true" />
            </Button>
            <RevisionSelect
              tenant={tenant}
              tree={tree}
              branch={defaultBranch}
              label="Target (to)"
              value={to}
              onChange={(v) => update({ from, to: v })}
            />
          </div>
          {result.data && <Summary result={result.data} />}
        </CardContent>
      </Card>

      {!to ? (
        <EmptyState title="Choose a target revision" />
      ) : result.isPending ? (
        <LoadingRows label="Loading comparison" rows={6} />
      ) : result.isError ? (
        <QueryError error={result.error} onRetry={() => void result.refetch()} />
      ) : (
        <>
          <Card className="gap-0 py-0">
            <CardHeader className="border-b py-3">
              <CardTitle className="text-sm">
                <h2>Snapshots</h2>
              </CardTitle>
            </CardHeader>
            {result.data.snapshots.length === 0 ? (
              <p className="px-6 py-4 text-sm text-muted-foreground">No snapshots between these revisions.</p>
            ) : (
              <ol aria-label="Snapshots in comparison" className="max-h-80 divide-y overflow-y-auto">
                {result.data.snapshots.map((s) => (
                  <li key={s.id} className="flex items-center gap-3 px-4 py-2 text-sm">
                    <GitCommitHorizontal aria-hidden="true" className="size-4 shrink-0 text-muted-foreground" />
                    <AuthorAvatar name={s.author} className="hidden sm:flex" />
                    <span className="min-w-0 flex-1">
                      <span className="flex flex-wrap items-center gap-2">
                        <Link to={snapshotPath(s.id)} className="truncate font-medium hover:underline">
                          {firstLine(s.message) || "no message"}
                        </Link>
                        <SnapshotBadges snapshot={s} />
                      </span>
                      <span className="text-xs text-muted-foreground">
                        {s.author} · <RelativeTime value={s.created_at} />
                      </span>
                    </span>
                    <span title={s.id} className="shrink-0 rounded border px-1.5 py-0.5 font-mono text-xs">
                      {shortId(s.id)}
                    </span>
                  </li>
                ))}
              </ol>
            )}
          </Card>
          <section aria-label="File changes">
            <DiffViewer files={result.data.files} />
          </section>
        </>
      )}
    </div>
  );
}
