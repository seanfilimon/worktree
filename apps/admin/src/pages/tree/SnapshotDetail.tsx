import { ArrowLeft, BadgeCheck, FileSignature, FolderOpen, GitBranch, GitCompareArrows, Link2, ShieldOff, Undo2 } from "lucide-react";
import type { ReactNode } from "react";
import { Link, useParams } from "react-router";
import { useCompare, useSnapshot } from "@/api/queries";
import { useDependencies, type SnapshotDetailV05, type SnapshotV05 } from "@/api/v05tree";
import { DiffViewer } from "@/components/diff/DiffViewer";
import { ChangeCounts } from "@/components/diff/parts";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { absoluteTime, formatNumber, shortId } from "@/lib/format";
import { filesPath, treePath } from "@/lib/paths";
import { useTreeContext } from "./context";
import { AuthorAvatar, CopyButton, LoadingRows, QueryError, RelativeTime, SnapshotBadges } from "./history/shared";
import { BlockingBadge, PriorityBadge, TodoStatusBadge } from "./v05/shared";

function SignatureInfo({ snapshot: s }: { snapshot: SnapshotV05 }) {
  if (s.verified) {
    return (
      <>
        <Badge>
          <BadgeCheck aria-hidden="true" />
          Verified
        </Badge>
        {s.signing_key_id && <code className="font-mono text-xs text-muted-foreground">key {s.signing_key_id}</code>}
      </>
    );
  }
  if (s.signature) {
    return (
      <>
        <Badge variant="outline">
          <FileSignature aria-hidden="true" />
          Signed, not verified
        </Badge>
        {s.signing_key_id && <code className="font-mono text-xs text-muted-foreground">key {s.signing_key_id}</code>}
      </>
    );
  }
  return (
    <span className="inline-flex items-center gap-1.5 text-muted-foreground">
      <ShieldOff aria-hidden="true" className="size-3.5" />
      Unsigned
    </span>
  );
}

/** Dependencies declared on this snapshot and the TODOs they created (§13.5). */
function DependenciesCard({ snapshot: s }: { snapshot: SnapshotV05 }) {
  const { tenant, tree } = useTreeContext();
  const status = useDependencies(tenant, tree, s.branch_name);
  const deps = s.dependencies ?? [];
  const outgoing = (status.data?.outgoing ?? []).filter((o) => o.snapshot_id === s.id);

  return (
    <Card className="gap-3">
      <CardHeader>
        <CardTitle className="flex items-center gap-2 text-base">
          <Link2 aria-hidden="true" className="size-4" />
          Dependencies
        </CardTitle>
        <CardDescription>Work this snapshot requested from other trees. Each dependency opens a TODO in its target tree.</CardDescription>
      </CardHeader>
      <CardContent>
        <Table aria-label="Dependencies">
          <TableHeader>
            <TableRow>
              <TableHead>Target</TableHead>
              <TableHead>Request</TableHead>
              <TableHead>Priority</TableHead>
              <TableHead>TODO</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {deps.map((d, i) => {
              const target = `${d.tenant_slug || tenant}/${d.tree}`;
              const todo = outgoing.find((o) => o.dependency.tree === d.tree && o.dependency.title === d.title)?.todo;
              return (
                <TableRow key={`${target}:${d.title}:${i}`}>
                  <TableCell className="font-mono text-xs">
                    {target}
                    {d.branch && <span className="text-muted-foreground">@{d.branch}</span>}
                  </TableCell>
                  <TableCell className="max-w-sm whitespace-normal">
                    <span className="flex flex-wrap items-center gap-1.5">
                      <span className="font-medium">{d.title}</span>
                      {d.blocking && <BlockingBadge />}
                      {d.linked && <Badge variant="outline">linked</Badge>}
                    </span>
                    {d.details && <span className="line-clamp-2 text-xs text-muted-foreground">{d.details}</span>}
                  </TableCell>
                  <TableCell>
                    <PriorityBadge priority={d.priority} />
                  </TableCell>
                  <TableCell>
                    {todo ? (
                      <span className="flex flex-wrap items-center gap-1.5">
                        <Link to={treePath(todo.tenant, todo.tree, "todos")} className="font-mono text-xs hover:underline">
                          {todo.tenant}/{todo.tree}#{todo.number}
                        </Link>
                        <TodoStatusBadge status={todo.status} />
                      </span>
                    ) : (
                      <span className="text-xs text-muted-foreground">{status.isPending ? "…" : "not created"}</span>
                    )}
                  </TableCell>
                </TableRow>
              );
            })}
          </TableBody>
        </Table>
      </CardContent>
    </Card>
  );
}

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

  const data = detail.data as SnapshotDetailV05;
  const s = data.snapshot;
  const restricted = data.restricted_count ?? 0;
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
              {restricted > 0 && (
                <span className="text-xs text-muted-foreground">
                  · {formatNumber(restricted)} restricted {restricted === 1 ? "path" : "paths"} hidden
                </span>
              )}
            </Meta>
            <Meta label="Signature">
              <SignatureInfo snapshot={s} />
            </Meta>
            {s.reverts && (
              <Meta label="Reverts">
                <Undo2 aria-hidden="true" className="size-3.5 text-muted-foreground" />
                <Link to={snapshotPath(s.reverts)} title={s.reverts} className="rounded border px-1.5 py-0.5 font-mono text-xs hover:bg-accent">
                  {shortId(s.reverts)}
                </Link>
              </Meta>
            )}
          </dl>
        </CardContent>
      </Card>

      {(s.dependencies?.length ?? 0) > 0 && <DependenciesCard snapshot={s} />}

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
