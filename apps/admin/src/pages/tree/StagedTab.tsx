import { useQueryClient } from "@tanstack/react-query";
import { ChevronDown, FileStack, GitBranch, TriangleAlert, Users } from "lucide-react";
import { useEffect, useId, useMemo, useRef, useState } from "react";
import { Link, useSearchParams } from "react-router";
import { keys, useStaged, useStagedSummary } from "@/api/queries";
import type { StagedBranchActivity, StagedFileOverlap, StagedSnapshot } from "@/api/types";
import { EmptyState, ErrorState, LoadingRows, RelativeTime } from "@/components/files/states";
import { LiveIndicator } from "@/components/LiveIndicator";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Avatar, AvatarFallback } from "@/components/ui/avatar";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { firstLine, formatNumber, shortId } from "@/lib/format";
import { treePath } from "@/lib/paths";
import { cn } from "@/lib/utils";
import { BranchSelect } from "./BranchSelect";
import { useTreeContext } from "./context";

const PREVIEW_FILES = 5;

function initials(name: string): string {
  const parts = name.split(/[\s._-]+/).filter(Boolean);
  return ((parts[0]?.[0] ?? "?") + (parts[1]?.[0] ?? "")).toUpperCase();
}

function AuthorAvatar({ name, className }: { name: string; className?: string }) {
  return (
    <Avatar size="sm" className={className}>
      <AvatarFallback className="border bg-background font-semibold text-foreground">{initials(name)}</AvatarFallback>
    </Avatar>
  );
}

/** Files staged by more than one author, merged across branches. */
function collectOverlaps(summary: readonly StagedBranchActivity[]): Map<string, StagedFileOverlap> {
  const byPath = new Map<string, StagedFileOverlap>();
  for (const b of summary) {
    for (const o of b.overlaps ?? []) {
      const prev = byPath.get(o.path);
      byPath.set(o.path, {
        path: o.path,
        authors: [...new Set([...(prev?.authors ?? []), ...(o.authors ?? [])])].sort(),
        branches: [...new Set([...(prev?.branches ?? []), ...(o.branches ?? []), b.branch])].sort(),
      });
    }
  }
  return byPath;
}

function OverlapWarning({ overlaps }: { overlaps: StagedFileOverlap[] }) {
  return (
    <Alert className="border-2 border-foreground">
      <TriangleAlert aria-hidden="true" />
      <AlertTitle className="font-bold">
        {overlaps.length === 1 ? "1 file is" : `${overlaps.length} files are`} being changed by more than one person
      </AlertTitle>
      <AlertDescription>
        <p>These may conflict when the work is pushed. Coordinate before merging.</p>
        <ul aria-label="Overlapping files" className="mt-2 flex w-full flex-col gap-1">
          {overlaps.map((o) => (
            <li key={o.path} className="flex flex-wrap items-center gap-x-2 gap-y-0.5 text-foreground">
              <span className="font-mono font-bold break-all">{o.path}</span>
              <span className="text-muted-foreground">
                {o.authors.join(", ")} · on {o.branches.join(", ")}
              </span>
            </li>
          ))}
        </ul>
      </AlertDescription>
    </Alert>
  );
}

function BranchCard({ activity, overlaps }: { activity: StagedBranchActivity; overlaps: Map<string, StagedFileOverlap> }) {
  const { tenant, tree } = useTreeContext();
  const authors = [...(activity.authors ?? [])].sort((a, b) => b.staged_at.localeCompare(a.staged_at));
  const conflicts = (activity.overlaps ?? []).filter((o) => overlaps.has(o.path)).length;
  return (
    <Card className="gap-4 py-4">
      <CardHeader className="px-4">
        <CardTitle className="flex min-w-0 items-center gap-2 font-mono text-sm">
          <GitBranch aria-hidden="true" className="size-4 shrink-0" />
          <Link
            to={`${treePath(tenant, tree, "files")}?branch=${encodeURIComponent(activity.branch)}`}
            className="truncate hover:underline"
          >
            {activity.branch}
          </Link>
        </CardTitle>
        <CardDescription className="flex flex-wrap gap-x-3 text-xs">
          <span>
            <span className="font-semibold text-foreground tabular-nums">{formatNumber(activity.files_changed)}</span> files
          </span>
          <span>
            <span className="font-semibold text-foreground tabular-nums">{authors.length}</span>{" "}
            {authors.length === 1 ? "author" : "authors"}
          </span>
          <span>
            latest <RelativeTime value={activity.latest_at} />
          </span>
        </CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-3 px-4">
        <ul aria-label={`Authors on ${activity.branch}`} className="flex flex-col gap-2">
          {authors.map((a) => (
            <li key={a.account_id || a.author} className="flex items-center gap-2 text-sm">
              <AuthorAvatar name={a.author} />
              <span className="min-w-0 flex-1 truncate font-medium">{a.author}</span>
              <span className="text-xs text-muted-foreground tabular-nums">
                {a.files} {a.files === 1 ? "file" : "files"} · <RelativeTime value={a.staged_at} />
              </span>
            </li>
          ))}
        </ul>
        {conflicts > 0 && (
          <p className="flex items-center gap-1.5 border-t pt-3 text-xs font-bold">
            <TriangleAlert aria-hidden="true" className="size-3.5" />
            {conflicts} overlapping {conflicts === 1 ? "file" : "files"}
          </p>
        )}
      </CardContent>
    </Card>
  );
}

function StagedItem({ s, overlaps }: { s: StagedSnapshot; overlaps: Map<string, StagedFileOverlap> }) {
  const { tenant, tree } = useTreeContext();
  const files = s.files_changed ?? [];
  const [open, setOpen] = useState(false);
  const shown = open ? files : files.slice(0, PREVIEW_FILES);
  const contested = files.filter((f) => overlaps.has(f)).length;
  const listId = useId();

  return (
    <li className="px-4 py-3">
      <div className="flex flex-wrap items-start gap-3">
        <AuthorAvatar name={s.author} className="mt-0.5" />
        <div className="min-w-0 flex-1">
          <p className="flex flex-wrap items-center gap-2 text-sm">
            <span className="font-semibold">{s.author}</span>
            <Badge variant="outline" className="font-mono">
              <GitBranch aria-hidden="true" />
              {s.branch_name}
            </Badge>
            {contested > 0 && (
              <Badge className="font-semibold">
                <TriangleAlert aria-hidden="true" />
                {contested} overlapping
              </Badge>
            )}
          </p>
          {s.message && <p className="mt-0.5 truncate text-sm text-muted-foreground">{firstLine(s.message)}</p>}
          <p className="mt-1 flex flex-wrap gap-x-3 font-mono text-xs" aria-label="Change summary">
            <span className="font-semibold">+{s.files_added} added</span>
            <span>~{s.files_modified} modified</span>
            <span className="text-muted-foreground line-through decoration-1">-{s.files_deleted} deleted</span>
          </p>
        </div>
        <div className="text-right text-xs text-muted-foreground">
          <p>
            staged <RelativeTime value={s.staged_at} />
          </p>
          <Link
            to={treePath(tenant, tree, `snapshots/${encodeURIComponent(s.snapshot_id)}`)}
            className="font-mono text-foreground hover:underline"
            title={s.snapshot_id}
          >
            {shortId(s.snapshot_id)}
          </Link>
        </div>
      </div>
      {files.length > 0 && (
        <div className="mt-2 pl-9">
          <ul
            id={listId}
            aria-label={`Files staged by ${s.author} on ${s.branch_name}`}
            className="flex flex-col gap-0.5 font-mono text-xs"
          >
            {shown.map((f) => {
              const o = overlaps.get(f);
              return (
                <li
                  key={f}
                  className={cn("flex items-center gap-1.5 break-all", o ? "font-bold text-foreground" : "text-muted-foreground")}
                >
                  {o && <TriangleAlert aria-hidden="true" className="size-3 shrink-0" />}
                  {f}
                  {o && <span className="sr-only"> (also staged by {o.authors.filter((a) => a !== s.author).join(", ")})</span>}
                </li>
              );
            })}
          </ul>
          {files.length > PREVIEW_FILES && (
            <Button
              variant="link"
              size="sm"
              className="h-auto px-0 text-xs"
              aria-expanded={open}
              aria-controls={listId}
              onClick={() => setOpen((v) => !v)}
            >
              <ChevronDown aria-hidden="true" className={cn("transition-transform", open && "rotate-180")} />
              {open ? "Show fewer" : `Show all ${files.length} files`}
            </Button>
          )}
        </div>
      )}
    </li>
  );
}

export function StagedTab() {
  const { tenant, tree, live } = useTreeContext();
  const [params, setParams] = useSearchParams();
  const branch = params.get("branch") ?? "";
  const staged = useStaged(tenant, tree, branch);
  const summary = useStagedSummary(tenant, tree);
  const queryClient = useQueryClient();
  const overlaps = useMemo(() => collectOverlaps(summary.data ?? []), [summary.data]);

  // The tree layout refreshes the staged list on live events; keep the summary in step.
  const seen = useRef({ branch, at: staged.dataUpdatedAt });
  useEffect(() => {
    const prev = seen.current;
    seen.current = { branch, at: staged.dataUpdatedAt };
    if (prev.branch === branch && prev.at !== 0 && prev.at !== staged.dataUpdatedAt) {
      void queryClient.invalidateQueries({ queryKey: keys.stagedSummary(tenant, tree) });
    }
  }, [branch, staged.dataUpdatedAt, queryClient, tenant, tree]);

  const branchesSorted = useMemo(() => [...(summary.data ?? [])].sort((a, b) => b.latest_at.localeCompare(a.latest_at)), [summary.data]);
  const list = useMemo(() => [...(staged.data ?? [])].sort((a, b) => b.staged_at.localeCompare(a.staged_at)), [staged.data]);

  return (
    <div className="flex flex-col gap-6">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <h2 className="text-lg font-semibold">Staged work in progress</h2>
          <p className="text-sm text-muted-foreground">Who is working on what. Updates live as teammates stage or push.</p>
        </div>
        <LiveIndicator status={live} />
      </div>

      {overlaps.size > 0 && <OverlapWarning overlaps={[...overlaps.values()]} />}

      <section aria-labelledby="staged-by-branch" className="flex flex-col gap-3">
        <h3 id="staged-by-branch" className="flex items-center gap-2 text-sm font-semibold">
          <Users aria-hidden="true" className="size-4" />
          Activity by branch
        </h3>
        {summary.isPending ? (
          <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
            <LoadingRows rows={3} label="Loading staged summary" className="rounded-xl border" />
          </div>
        ) : summary.isError ? (
          <div className="rounded-xl border">
            <ErrorState error={summary.error} onRetry={() => void summary.refetch()} />
          </div>
        ) : branchesSorted.length === 0 ? (
          <p className="rounded-xl border border-dashed px-4 py-6 text-center text-sm text-muted-foreground">No branch has staged work.</p>
        ) : (
          <ul aria-label="Staged activity by branch" className="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
            {branchesSorted.map((a) => (
              <li key={a.branch}>
                <BranchCard activity={a} overlaps={overlaps} />
              </li>
            ))}
          </ul>
        )}
      </section>

      <section aria-labelledby="staged-all" className="flex flex-col gap-3">
        <div className="flex flex-wrap items-center justify-between gap-2">
          <h3 id="staged-all" className="flex items-center gap-2 text-sm font-semibold">
            <FileStack aria-hidden="true" className="size-4" />
            All staged snapshots
          </h3>
          <BranchSelect
            tenant={tenant}
            tree={tree}
            value={branch}
            allowAll
            label="Filter by branch"
            onChange={(b) => setParams(b ? { branch: b } : {}, { replace: true })}
          />
        </div>
        <div className="overflow-hidden rounded-xl border bg-card">
          {staged.isPending ? (
            <LoadingRows label="Loading staged work" />
          ) : staged.isError ? (
            <ErrorState error={staged.error} onRetry={() => void staged.refetch()} />
          ) : list.length === 0 ? (
            <EmptyState icon={FileStack} title={branch ? `Nobody has staged work on ${branch}` : "Nobody has staged work"}>
              Teammates share WIP with <code className="font-mono">wt stage</code>, or automatically via the background daemon.
            </EmptyState>
          ) : (
            <ul aria-label="Staged snapshots" className="divide-y">
              {list.map((s) => (
                <StagedItem key={`${s.account_id}-${s.branch_name}`} s={s} overlaps={overlaps} />
              ))}
            </ul>
          )}
        </div>
      </section>
    </div>
  );
}
