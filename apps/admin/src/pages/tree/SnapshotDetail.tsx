import { ArrowLeft, FolderOpen, GitBranch, GitCompareArrows } from "lucide-react";
import type { ReactNode } from "react";
import { Link, useParams } from "react-router";
import { useCompare, useSnapshot } from "@/api/queries";
import { DiffViewer } from "@/components/diff/DiffViewer";
import { ChangeCounts } from "@/components/diff/parts";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { absoluteTime, formatNumber, shortId } from "@/lib/format";
import { filesPath, treePath } from "@/lib/paths";
import { useTreeContext } from "./context";
import { AuthorAvatar, CopyButton, LoadingRows, QueryError, RelativeTime, SnapshotBadges } from "./history/shared";

function Meta({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="flex flex-col gap-1">
      <dt className="text-xs text-muted-foreground">{label}</dt>
      <dd className="flex min-w-0 flex-wrap items-center gap-1.5 text-sm">{children}</dd>
    </div>
  );
}

export function SnapshotDetail() {
  const { tenant, tree } = useTreeContext();
  const { id = "" } = useParams();
  const detail = useSnapshot(tenant, tree, id);
  const diff = useCompare(tenant, tree, id);
  const snapshotPath = (sid: string) => treePath(tenant, tree, `snapshots/${encodeURIComponent(sid)}`);

  const back = (
    <Button variant="ghost" size="sm" asChild className="self-start">
      <Link to={treePath(tenant, tree, "history")}>
        <ArrowLeft aria-hidden="true" />
        History
      </Link>
    </Button>
  );

  if (detail.isPending) {
    return (
      <div className="flex flex-col gap-4">
        {back}
        <LoadingRows label="Loading snapshot" rows={4} />
      </div>
    );
  }
  if (detail.isError) {
    return (
      <div className="flex flex-col gap-4">
        {back}
        <QueryError error={detail.error} onRetry={() => void detail.refetch()} />
      </div>
    );
  }

  const s = detail.data.snapshot;
  const parents = s.parents ?? [];
  const [title = "", ...rest] = s.message.split(/\r?\n/);
  const body = rest.join("\n").trim();
  const fileCount = detail.data.files?.length ?? 0;

  return (
    <div className="flex flex-col gap-4">
      {back}
      <Card>
        <CardHeader className="gap-2">
          <CardTitle className="flex flex-wrap items-center gap-2 text-lg">
            <h2>{title.trim() || <span className="text-muted-foreground italic">no message</span>}</h2>
            <SnapshotBadges snapshot={s} />
          </CardTitle>
          {body && <CardDescription className="whitespace-pre-wrap">{body}</CardDescription>}
          <div className="flex flex-wrap gap-2 pt-1">
            <Button variant="outline" size="sm" asChild>
              <Link to={filesPath(tenant, tree, { snapshot: s.id })}>
                <FolderOpen aria-hidden="true" />
                Browse files at this snapshot
              </Link>
            </Button>
            <Button variant="outline" size="sm" asChild>
              <Link to={`${treePath(tenant, tree, "compare")}?${new URLSearchParams({ ...(parents[0] ? { from: parents[0] } : {}), to: s.id }).toString()}`}>
                <GitCompareArrows aria-hidden="true" />
                Open in compare
              </Link>
            </Button>
          </div>
        </CardHeader>
        <CardContent>
          <dl className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
            <Meta label="Snapshot">
              <code className="font-mono text-xs break-all">{s.id}</code>
              <CopyButton value={s.id} label="Copy snapshot id" />
            </Meta>
            <Meta label={parents.length > 1 ? "Parents" : "Parent"}>
              {parents.length === 0 ? (
                <span className="text-muted-foreground">none (root snapshot)</span>
              ) : (
                parents.map((p) => (
                  <Link key={p} to={snapshotPath(p)} title={p} className="rounded border px-1.5 py-0.5 font-mono text-xs hover:bg-accent">
                    {shortId(p)}
                  </Link>
                ))
              )}
            </Meta>
            <Meta label="Author">
              <AuthorAvatar name={s.author} />
              <span className="font-medium">{s.author}</span>
            </Meta>
            <Meta label="Snapshotted">
              <RelativeTime value={s.created_at} />
              <span className="text-xs text-muted-foreground">{absoluteTime(s.created_at)}</span>
            </Meta>
            <Meta label="Branch">
              <GitBranch aria-hidden="true" className="size-3.5 text-muted-foreground" />
              <Link
                to={`${treePath(tenant, tree, "history")}?${new URLSearchParams({ branch: s.branch_name }).toString()}`}
                className="font-mono hover:underline"
              >
                {s.branch_name}
              </Link>
            </Meta>
            <Meta label="Files">
              {formatNumber(fileCount)} in manifest · pushed <RelativeTime value={s.pushed_at} />
            </Meta>
          </dl>
        </CardContent>
      </Card>

      <section aria-labelledby="snapshot-changes" className="flex flex-col gap-3">
        <div className="flex flex-wrap items-baseline gap-x-3 gap-y-1">
          <h2 id="snapshot-changes" className="text-base font-semibold">
            Changes
          </h2>
          {diff.data && (
            <span className="text-sm text-muted-foreground">
              {diff.data.from ? (
                <>
                  against parent{" "}
                  <Link to={snapshotPath(diff.data.from.id)} className="font-mono text-foreground hover:underline">
                    {shortId(diff.data.from.id)}
                  </Link>
                </>
              ) : (
                "root snapshot: every file is new"
              )}
              {" · "}
              <ChangeCounts additions={diff.data.stats.additions} deletions={diff.data.stats.deletions} />
            </span>
          )}
        </div>
        {diff.isPending ? (
          <LoadingRows label="Loading changes" rows={6} />
        ) : diff.isError ? (
          <QueryError error={diff.error} onRetry={() => void diff.refetch()} />
        ) : (
          <DiffViewer files={diff.data.files} />
        )}
      </section>
    </div>
  );
}
