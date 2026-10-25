import { EyeOff, FolderGit2, Pin } from "lucide-react";
import { Fragment, useCallback, useMemo, useState } from "react";
import { Link, useSearchParams } from "react-router";
import { useBranches, useSnapshot } from "@/api/queries";
import type { SnapshotDetailV05 } from "@/api/v05tree";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { DirectoryView } from "@/components/files/DirectoryView";
import { FileTree } from "@/components/files/FileTree";
import { FileViewer } from "@/components/files/FileViewer";
import { resolveRelative } from "@/components/files/markdown";
import { SplitPane } from "@/components/files/SplitPane";
import { EmptyState, ErrorState, LoadingRows, RelativeTime } from "@/components/files/states";
import { ancestorDirs } from "@/components/files/tree";
import { Badge } from "@/components/ui/badge";
import {
  Breadcrumb,
  BreadcrumbItem,
  BreadcrumbLink,
  BreadcrumbList,
  BreadcrumbPage,
  BreadcrumbSeparator,
} from "@/components/ui/breadcrumb";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { normalizeDir, parentDir } from "@/lib/files";
import { firstLine, formatNumber, shortId } from "@/lib/format";
import { treePath } from "@/lib/paths";
import { BranchSelect } from "./BranchSelect";
import { useTreeContext } from "./context";

/** Files tab location (`branch` empty = default branch, `snapshot` empty = branch tip). */
function filesHref(tenant: string, tree: string, opts: { branch?: string; snapshot?: string; path?: string }): string {
  const q = new URLSearchParams();
  if (opts.branch) q.set("branch", opts.branch);
  if (opts.snapshot) q.set("snapshot", opts.snapshot);
  if (opts.path) q.set("path", opts.path);
  const s = q.toString();
  return `${treePath(tenant, tree, "files")}${s ? `?${s}` : ""}`;
}

/** Directories to open so that `path` is visible (its ancestors and itself). */
function autoExpand(path: string): string[] {
  return path ? [...ancestorDirs(path), path] : [];
}

function PathBreadcrumb({ root, path, hrefFor }: { root: string; path: string; hrefFor: (p: string) => string }) {
  const parts = path ? path.split("/") : [];
  return (
    <Breadcrumb aria-label="File path">
      <BreadcrumbList className="font-mono text-[13px]">
        <BreadcrumbItem>
          {parts.length ? (
            <BreadcrumbLink asChild>
              <Link to={hrefFor("")}>{root}</Link>
            </BreadcrumbLink>
          ) : (
            <BreadcrumbPage>{root}</BreadcrumbPage>
          )}
        </BreadcrumbItem>
        {parts.map((part, i) => {
          const p = parts.slice(0, i + 1).join("/");
          return (
            <Fragment key={p}>
              <BreadcrumbSeparator />
              <BreadcrumbItem>
                {i === parts.length - 1 ? (
                  <BreadcrumbPage className="font-semibold">{part}</BreadcrumbPage>
                ) : (
                  <BreadcrumbLink asChild>
                    <Link to={hrefFor(p)}>{part}</Link>
                  </BreadcrumbLink>
                )}
              </BreadcrumbItem>
            </Fragment>
          );
        })}
      </BreadcrumbList>
    </Breadcrumb>
  );
}

export function FilesTab() {
  const { tenant, tree, summary } = useTreeContext();
  const [params, setParams] = useSearchParams();
  const pinned = params.get("snapshot") ?? "";
  const branchParam = params.get("branch") ?? "";
  const branch = branchParam || summary.tree.default_branch;
  // Legacy links carry the folder in `path` and the file in `file`.
  const target = normalizeDir(params.get("file") || params.get("path") || "");
  const branches = useBranches(tenant, tree);
  const tip = branches.data?.find((b) => b.name === branch)?.tip ?? "";
  const snapshotId = pinned || tip;
  const detail = useSnapshot(tenant, tree, snapshotId);
  const files = useMemo(() => detail.data?.files ?? [], [detail.data]);
  const selectedFile = files.find((f) => f.path === target);

  const [expanded, setExpanded] = useState<ReadonlySet<string>>(() => new Set(autoExpand(target)));
  const [revealed, setRevealed] = useState(target);
  if (revealed !== target) {
    setRevealed(target);
    setExpanded((prev) => new Set([...prev, ...autoExpand(target)]));
  }
  const toggle = useCallback((dir: string) => {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (!next.delete(dir)) next.add(dir);
      return next;
    });
  }, []);

  const hrefFor = useCallback(
    (path: string) => filesHref(tenant, tree, { branch: branchParam, snapshot: pinned, path }),
    [tenant, tree, branchParam, pinned],
  );
  const baseDir = selectedFile ? parentDir(selectedFile.path) : target;
  const resolveLink = useCallback((link: string) => hrefFor(resolveRelative(baseDir, link)), [hrefFor, baseDir]);

  const onBranchChange = (b: string) => {
    const next = new URLSearchParams();
    if (b !== summary.tree.default_branch) next.set("branch", b);
    if (target) next.set("path", target);
    setParams(next);
  };

  const snap = detail.data?.snapshot;
  const toolbar = (
    <div className="mb-3 flex flex-wrap items-center gap-x-3 gap-y-2">
      <BranchSelect tenant={tenant} tree={tree} value={pinned && snap ? snap.branch_name : branch} onChange={onBranchChange} />
      {pinned && (
        <Badge variant="outline" className="gap-1.5 py-1">
          <Pin aria-hidden="true" />
          Pinned to snapshot <span className="font-mono">{shortId(pinned)}</span>
        </Badge>
      )}
      {pinned && (
        <Button variant="link" size="sm" asChild className="h-auto px-0">
          <Link
            to={filesHref(tenant, tree, {
              branch: snap && snap.branch_name !== summary.tree.default_branch ? snap.branch_name : branchParam,
              path: target,
            })}
          >
            Show branch tip
          </Link>
        </Button>
      )}
      {snap ? (
        <p className="min-w-0 flex-1 truncate text-[13px] text-muted-foreground">
          <Link
            to={treePath(tenant, tree, `snapshots/${encodeURIComponent(snap.id)}`)}
            className="font-mono font-medium text-foreground hover:underline"
            title={snap.id}
          >
            {shortId(snap.id)}
          </Link>{" "}
          {firstLine(snap.message)} · <span className="text-foreground">{snap.author}</span> · <RelativeTime value={snap.created_at} />
        </p>
      ) : (
        detail.isPending && snapshotId !== "" && <Skeleton className="h-4 w-64" />
      )}
    </div>
  );

  if (!snapshotId) {
    return (
      <>
        {toolbar}
        <div className="rounded-xl border bg-card">
          {branches.isPending ? (
            <LoadingRows label="Loading branches" />
          ) : branches.isError ? (
            <ErrorState error={branches.error} onRetry={() => void branches.refetch()} />
          ) : (
            <EmptyState icon={FolderGit2} title="Nothing to browse">
              {branches.data.some((b) => b.name === branch) ? (
                <>
                  <span className="font-mono">{branch}</span> has no snapshots yet. Push one with <code className="font-mono">wt push</code>
                  .
                </>
              ) : (
                <>
                  Branch <span className="font-mono">{branch}</span> does not exist.
                </>
              )}
            </EmptyState>
          )}
        </div>
      </>
    );
  }

  if (detail.isPending) {
    return (
      <>
        {toolbar}
        <div className="rounded-xl border bg-card">
          <LoadingRows rows={8} label="Loading files" />
        </div>
      </>
    );
  }

  if (detail.isError) {
    return (
      <>
        {toolbar}
        <div className="rounded-xl border bg-card">
          <ErrorState error={detail.error} onRetry={() => void detail.refetch()} />
        </div>
      </>
    );
  }

  const restricted = (detail.data as SnapshotDetailV05).restricted_count ?? 0;

  return (
    <>
      {toolbar}
      {restricted > 0 && (
        <Alert className="mb-3" aria-label="Restricted paths">
          <EyeOff aria-hidden="true" />
          <AlertTitle>
            {formatNumber(restricted)} {restricted === 1 ? "path is" : "paths are"} hidden
          </AlertTitle>
          <AlertDescription>
            You may not read {restricted === 1 ? "it" : "them"} at this snapshot: an access policy denies the path or a license rule
            restricts it for your tenant. Clones and pulls leave such files out too.
          </AlertDescription>
        </Alert>
      )}
      <SplitPane
        label="Resize file tree"
        start={
          <FileTree
            files={files}
            selected={target}
            expanded={expanded}
            onToggle={toggle}
            hrefFor={hrefFor}
            className="max-h-[45vh] rounded-xl border bg-card md:sticky md:top-4 md:max-h-[calc(100dvh-10rem)]"
          />
        }
        end={
          <div className="flex min-w-0 flex-col gap-3 md:pl-1">
            <PathBreadcrumb root={tree} path={target} hrefFor={hrefFor} />
            {selectedFile ? (
              <FileViewer
                key={`${snapshotId}:${selectedFile.path}`}
                tenant={tenant}
                tree={tree}
                snapshot={snapshotId}
                branch={pinned ? (snap?.branch_name ?? "") : branch}
                file={selectedFile}
                resolveLink={resolveLink}
              />
            ) : (
              <DirectoryView
                tenant={tenant}
                tree={tree}
                snapshot={snapshotId}
                files={files}
                dir={target}
                hrefFor={hrefFor}
                resolveLink={resolveLink}
              />
            )}
          </div>
        }
      />
    </>
  );
}
