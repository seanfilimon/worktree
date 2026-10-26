import { useMutation, useQueryClient } from "@tanstack/react-query";
import { ChevronDown, FileStack, FolderOpen, GitBranch, History, Tag, TriangleAlert, Undo2, Users } from "lucide-react";
import { useEffect, useId, useMemo, useRef, useState } from "react";
import { Link, useSearchParams } from "react-router";
import { keys, useStaged, useStagedSummary } from "@/api/queries";
import type { StagedBranchActivity, StagedFileOverlap } from "@/api/types";
import { isUnavailable, type StagedSnapshotV06 } from "@/api/v06";
import { useAuth } from "@/auth/context";
import { StagedFilesSheet } from "./v06/StagedFilesSheet";
import { EmptyState, ErrorState, LoadingRows, RelativeTime } from "@/components/files/states";
import { LiveIndicator } from "@/components/LiveIndicator";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Avatar, AvatarFallback } from "@/components/ui/avatar";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { UnavailableNotice } from "@/components/v06/UnavailableNotice";
import { firstLine, formatNumber, shortId } from "@/lib/format";
import { treePath } from "@/lib/paths";
import { cn } from "@/lib/utils";
import { ConfirmAction } from "@/pages/iam/shared";
import { useToast } from "@/toast/store";
import { BranchSelect } from "./BranchSelect";
import { useTreeContext } from "./context";
import {
  isStagedHistoryStatus,
  labelOf,
  slotName,
  stagedV2Keys,
  STAGED_HISTORY_STATUSES,
  useStagedHistory,
  useStagedV2Api,
  type StagedHistoryStatus,
} from "./v08/stagedApi";

const PREVIEW_FILES = 5;
const HISTORY_LIMIT = 100;

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

function LabelBadge({ label }: { label: string }) {
  return (
    <Badge variant="outline" className={cn("font-mono", !label && "text-muted-foreground")}>
      <Tag aria-hidden="true" />
      {label || "default"}
    </Badge>
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

interface AuthorGroupData {
  key: string;
  author: string;
  accountId: string;
  latest: string;
  items: StagedSnapshotV06[];
}

/** Live entries grouped by account, newest author first; rows by branch, then label (default first). */
function groupByAuthor(list: readonly StagedSnapshotV06[]): AuthorGroupData[] {
  const groups = new Map<string, AuthorGroupData>();
  for (const s of list) {
    const key = s.account_id || s.author;
    const g = groups.get(key) ?? { key, author: s.author, accountId: s.account_id, latest: s.staged_at, items: [] };
    g.items.push(s);
    if (s.staged_at > g.latest) g.latest = s.staged_at;
    groups.set(key, g);
  }
  for (const g of groups.values()) {
    g.items.sort((a, b) => a.branch_name.localeCompare(b.branch_name) || labelOf(a).localeCompare(labelOf(b)));
  }
  return [...groups.values()].sort((a, b) => b.latest.localeCompare(a.latest) || a.author.localeCompare(b.author));
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

function StagedItem({
  s,
  overlaps,
  canUnstage,
  onBrowse,
  onUnstage,
}: {
  s: StagedSnapshotV06;
  overlaps: Map<string, StagedFileOverlap>;
  canUnstage: boolean;
  onBrowse: (s: StagedSnapshotV06) => void;
  onUnstage: (s: StagedSnapshotV06) => void;
}) {
  const { tenant, tree } = useTreeContext();
  const files = s.files_changed ?? [];
  const [open, setOpen] = useState(false);
  const shown = open ? files : files.slice(0, PREVIEW_FILES);
  const contested = files.filter((f) => overlaps.has(f)).length;
  const listId = useId();
  const label = labelOf(s);
  const slot = slotName(s);

  return (
    <li aria-label={`${s.branch_name} · ${label || "default"}`} className="px-4 py-3">
      <div className="flex flex-wrap items-start gap-3">
        <div className="min-w-0 flex-1">
          <p className="flex flex-wrap items-center gap-2 text-sm">
            <Badge variant="outline" className="font-mono">
              <GitBranch aria-hidden="true" />
              {s.branch_name}
            </Badge>
            <LabelBadge label={label} />
            {contested > 0 && (
              <Badge className="font-semibold">
                <TriangleAlert aria-hidden="true" />
                {contested} overlapping
              </Badge>
            )}
          </p>
          {s.message && <p className="mt-1 truncate text-sm text-muted-foreground">{firstLine(s.message)}</p>}
          <p className="mt-1 flex flex-wrap gap-x-3 font-mono text-xs" aria-label="Change summary">
            <span className="font-semibold">+{s.files_added} added</span>
            <span>~{s.files_modified} modified</span>
            <span className="text-muted-foreground line-through decoration-1">-{s.files_deleted} deleted</span>
          </p>
        </div>
        <div className="flex flex-col items-end gap-1.5 text-right text-xs text-muted-foreground">
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
          <div className="flex flex-wrap justify-end gap-1.5">
            {s.manifest_hash && (
              <Button
                variant="outline"
                size="sm"
                className="h-7 text-xs"
                aria-label={`Browse files staged by ${s.author} on ${slot}`}
                onClick={() => onBrowse(s)}
              >
                <FolderOpen aria-hidden="true" />
                Browse files
              </Button>
            )}
            {canUnstage && (
              <Button
                variant="outline"
                size="sm"
                className="h-7 text-xs"
                aria-label={`Unstage ${slot} by ${s.author}`}
                onClick={() => onUnstage(s)}
              >
                <Undo2 aria-hidden="true" />
                Unstage
              </Button>
            )}
          </div>
        </div>
      </div>
      {files.length > 0 && (
        <div className="mt-2">
          <ul id={listId} aria-label={`Files staged by ${s.author} on ${slot}`} className="flex flex-col gap-0.5 font-mono text-xs">
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

function AuthorGroup({
  group,
  overlaps,
  isSelf,
  canUnstage,
  onBrowse,
  onUnstage,
}: {
  group: AuthorGroupData;
  overlaps: Map<string, StagedFileOverlap>;
  isSelf: boolean;
  canUnstage: boolean;
  onBrowse: (s: StagedSnapshotV06) => void;
  onUnstage: (s: StagedSnapshotV06) => void;
}) {
  const branches = new Set(group.items.map((s) => s.branch_name)).size;
  return (
    <li aria-label={`Staged by ${group.author}`}>
      <div className="flex flex-wrap items-center gap-2 border-b bg-muted/40 px-4 py-2.5">
        <AuthorAvatar name={group.author} />
        <span className="font-semibold">{group.author}</span>
        {isSelf && <Badge variant="outline">you</Badge>}
        <span className="ml-auto text-xs text-muted-foreground tabular-nums">
          {group.items.length} {group.items.length === 1 ? "snapshot" : "snapshots"} · {branches} {branches === 1 ? "branch" : "branches"} · latest{" "}
          <RelativeTime value={group.latest} />
        </span>
      </div>
      <ul aria-label={`Snapshots staged by ${group.author}`} className="divide-y">
        {group.items.map((s) => (
          <StagedItem
            key={`${s.branch_name}\u0000${labelOf(s)}`}
            s={s}
            overlaps={overlaps}
            canUnstage={canUnstage}
            onBrowse={onBrowse}
            onUnstage={onUnstage}
          />
        ))}
      </ul>
    </li>
  );
}

type HistoryFilter = "all" | StagedHistoryStatus;

const STATUS_LABEL: Record<HistoryFilter, string> = { all: "All", pushed: "Pushed", cleared: "Cleared", expired: "Expired" };

function StagedHistory({ branch }: { branch: string }) {
  const { tenant, tree } = useTreeContext();
  const [status, setStatus] = useState<HistoryFilter>("all");
  const history = useStagedHistory(tenant, tree, {
    branch: branch || undefined,
    status: status === "all" ? undefined : status,
    limit: HISTORY_LIMIT,
  });
  const entries = history.data ?? [];

  return (
    <section aria-labelledby="staged-history" className="flex flex-col gap-3">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h3 id="staged-history" className="flex items-center gap-2 text-sm font-semibold">
          <History aria-hidden="true" className="size-4" />
          History
          <span className="font-normal text-muted-foreground">last 30 days</span>
        </h3>
        <ToggleGroup
          type="single"
          variant="outline"
          size="sm"
          value={status}
          onValueChange={(v) => {
            if (v === "all" || isStagedHistoryStatus(v)) setStatus(v);
          }}
          aria-label="Filter history by status"
        >
          {(["all", ...STAGED_HISTORY_STATUSES] as const).map((f) => (
            <ToggleGroupItem key={f} value={f} aria-label={STATUS_LABEL[f]}>
              {STATUS_LABEL[f]}
            </ToggleGroupItem>
          ))}
        </ToggleGroup>
      </div>
      {history.isError && isUnavailable(history.error) ? (
        <UnavailableNotice feature="Staged history" />
      ) : (
        <div className="overflow-hidden rounded-xl border bg-card">
          {history.isPending ? (
            <LoadingRows rows={3} label="Loading staged history" />
          ) : history.isError ? (
            <ErrorState error={history.error} onRetry={() => void history.refetch()} />
          ) : entries.length === 0 ? (
            <EmptyState icon={History} title={status === "all" ? "No staged history" : `No ${status} staged snapshots`}>
              Staged snapshots that were pushed, cleared or expired{branch ? ` on ${branch}` : ""} in the last 30 days appear here.
            </EmptyState>
          ) : (
            <Table aria-label="Staged history">
              <TableHeader>
                <TableRow>
                  <TableHead>Author</TableHead>
                  <TableHead>Branch</TableHead>
                  <TableHead>Label</TableHead>
                  <TableHead>Status</TableHead>
                  <TableHead>Ended</TableHead>
                  <TableHead>Ended by</TableHead>
                  <TableHead className="text-right">Files</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {entries.map((e) => (
                  <TableRow key={e.id}>
                    <TableCell className="font-medium">{e.author}</TableCell>
                    <TableCell className="font-mono text-xs">{e.branch_name}</TableCell>
                    <TableCell>
                      <LabelBadge label={labelOf(e)} />
                    </TableCell>
                    <TableCell>
                      <span className="flex items-center gap-2">
                        <Badge variant="outline">{e.status}</Badge>
                        {e.pushed_tip && (
                          <Link
                            to={treePath(tenant, tree, `snapshots/${encodeURIComponent(e.pushed_tip)}`)}
                            className="font-mono text-xs hover:underline"
                            title={e.pushed_tip}
                          >
                            {shortId(e.pushed_tip)}
                          </Link>
                        )}
                      </span>
                    </TableCell>
                    <TableCell className="text-xs text-muted-foreground">
                      <RelativeTime value={e.ended_at} />
                    </TableCell>
                    <TableCell className="text-xs">{e.ended_by || "—"}</TableCell>
                    <TableCell className="text-right font-mono text-xs tabular-nums" title={(e.files_changed ?? []).join("\n")}>
                      {formatNumber((e.files_changed ?? []).length)}
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          )}
        </div>
      )}
    </section>
  );
}

export function StagedTab() {
  const { tenant, tree, live, canManage } = useTreeContext();
  const { account } = useAuth();
  const [params, setParams] = useSearchParams();
  const branch = params.get("branch") ?? "";
  const staged = useStaged(tenant, tree, branch);
  const summary = useStagedSummary(tenant, tree);
  const queryClient = useQueryClient();
  const toast = useToast();
  const api = useStagedV2Api();
  const overlaps = useMemo(() => collectOverlaps(summary.data ?? []), [summary.data]);
  const [browsing, setBrowsing] = useState<StagedSnapshotV06 | null>(null);
  const [unstaging, setUnstaging] = useState<StagedSnapshotV06 | null>(null);

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
  const groups = useMemo(() => groupByAuthor(staged.data ?? []), [staged.data]);

  const isOwn = (s: StagedSnapshotV06) => account !== undefined && s.account_id === account.id;

  const unstage = useMutation({
    mutationFn: (s: StagedSnapshotV06) =>
      api.unstage(tenant, tree, { branch: s.branch_name, label: labelOf(s), account: isOwn(s) ? undefined : s.account_id }),
    meta: { inlineError: true },
    onSuccess: (_removed, s) => {
      void queryClient.invalidateQueries({ queryKey: stagedV2Keys.staged(tenant, tree) });
      void queryClient.invalidateQueries({ queryKey: keys.stagedSummary(tenant, tree) });
      void queryClient.invalidateQueries({ queryKey: keys.tree(tenant, tree), exact: true });
      toast.success("Unstaged", `${slotName(s)} by ${s.author}`);
    },
  });

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
          <GitBranch aria-hidden="true" className="size-4" />
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
            <Users aria-hidden="true" className="size-4" />
            Staged snapshots by person
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
          ) : groups.length === 0 ? (
            <EmptyState icon={FileStack} title={branch ? `Nobody has staged work on ${branch}` : "Nobody has staged work"}>
              Teammates share WIP with <code className="font-mono">wt stage</code> (or <code className="font-mono">wt stage --label</code> for
              a named slot), or automatically via the background daemon.
            </EmptyState>
          ) : (
            <ul aria-label="Staged snapshots" className="divide-y">
              {groups.map((g) => {
                const self = account !== undefined && g.accountId === account.id;
                return (
                  <AuthorGroup
                    key={g.key}
                    group={g}
                    overlaps={overlaps}
                    isSelf={self}
                    canUnstage={self || canManage}
                    onBrowse={setBrowsing}
                    onUnstage={setUnstaging}
                  />
                );
              })}
            </ul>
          )}
        </div>
      </section>

      <StagedHistory branch={branch} />

      <StagedFilesSheet staged={browsing} onClose={() => setBrowsing(null)} />
      <ConfirmAction
        open={unstaging !== null}
        onOpenChange={(o) => {
          if (!o) setUnstaging(null);
        }}
        title={unstaging ? `Unstage ${slotName(unstaging)}?` : "Unstage"}
        description={
          unstaging && (
            <>
              Removes the staged snapshot {isOwn(unstaging) ? "you" : <span className="font-medium text-foreground">{unstaging.author}</span>} shared on{" "}
              <span className="font-mono">{unstaging.branch_name}</span> in the{" "}
              <span className="font-mono">{labelOf(unstaging) || "default"}</span> slot. Other labels stay staged and local work is not touched.
            </>
          )
        }
        confirmLabel="Unstage"
        onConfirm={() => (unstaging ? unstage.mutateAsync(unstaging) : Promise.resolve())}
      />
    </div>
  );
}
