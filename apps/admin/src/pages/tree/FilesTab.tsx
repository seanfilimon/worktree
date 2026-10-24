import { useMutation } from "@tanstack/react-query";
import { useMemo } from "react";
import { Link, useSearchParams } from "react-router";
import { useApi } from "@/api/context";
import { useBlob, useBranches, useSnapshot } from "@/api/queries";
import type { SnapshotFile } from "@/api/types";
import { Icon } from "@/components/icons";
import { Badge } from "@/components/ui/Badge";
import { Button } from "@/components/ui/Button";
import { Card } from "@/components/ui/Card";
import { EmptyState, ErrorState, SkeletonRows } from "@/components/ui/States";
import { Table, Td, Th, Tr } from "@/components/ui/Table";
import { RelativeTime } from "@/components/ui/Time";
import { baseName, decodeText, downloadBytes, listDirectory, normalizeDir, parentDir, splitLines } from "@/lib/files";
import { firstLine, formatBytes, shortId } from "@/lib/format";
import { filesPath } from "@/lib/paths";
import { BranchSelect } from "./BranchSelect";
import { useTreeContext } from "./context";

/** Larger files are offered as a download instead of an inline preview. */
const PREVIEW_LIMIT = 1024 * 1024;

function PathCrumbs({ path, link }: { path: string; link: (dir: string) => string }) {
  const parts = path ? path.split("/") : [];
  return (
    <nav aria-label="File path" className="min-w-0">
      <ol className="flex flex-wrap items-center gap-1 font-mono text-[13px]">
        <li>
          <Link to={link("")} className="text-accent hover:underline">
            root
          </Link>
        </li>
        {parts.map((p, i) => {
          const last = i === parts.length - 1;
          const dir = parts.slice(0, i + 1).join("/");
          return (
            <li key={dir} className="flex items-center gap-1">
              <span className="text-subtle">/</span>
              {last ? (
                <span aria-current="page">{p}</span>
              ) : (
                <Link to={link(dir)} className="text-accent hover:underline">
                  {p}
                </Link>
              )}
            </li>
          );
        })}
      </ol>
    </nav>
  );
}

function FileViewer({ snapshot, file }: { snapshot: string; file: SnapshotFile }) {
  const api = useApi();
  const { tenant, tree } = useTreeContext();
  const previewable = file.size <= PREVIEW_LIMIT;
  const blob = useBlob(tenant, tree, snapshot, file.path, previewable);
  const text = useMemo(() => (blob.data ? decodeText(blob.data) : null), [blob.data]);
  const lines = useMemo(() => (text === null ? [] : splitLines(text)), [text]);

  const download = useMutation({
    mutationFn: async () => blob.data ?? (await api.trees.blob(tenant, tree, snapshot, file.path)),
    onSuccess: (bytes) => downloadBytes(bytes, baseName(file.path)),
  });

  const downloadButton = (
    <Button size="sm" onClick={() => download.mutate()} loading={download.isPending}>
      <Icon name="download" className="size-3.5" />
      Download
    </Button>
  );

  let body;
  if (!previewable) {
    body = <EmptyState title="File too large to preview">{formatBytes(file.size)} — download it to view.</EmptyState>;
  } else if (blob.isPending) {
    body = <SkeletonRows rows={8} label="Loading file" />;
  } else if (blob.isError) {
    body = <ErrorState error={blob.error} onRetry={() => void blob.refetch()} />;
  } else if (text === null) {
    body = <EmptyState title="Binary file">This file is not valid UTF-8 text and cannot be shown inline.</EmptyState>;
  } else if (lines.length === 1 && lines[0] === "") {
    body = <EmptyState title="Empty file" />;
  } else {
    body = (
      <div role="region" aria-label={`Contents of ${file.path}`} tabIndex={0} className="overflow-auto font-mono text-[12.5px] leading-5">
        <table className="w-full border-collapse">
          <tbody>
            {lines.map((line, i) => (
              <tr key={i} className="hover:bg-surface-2/60">
                <td aria-hidden="true" className="w-px border-r border-border px-3 text-right text-subtle select-none tabular-nums">
                  {i + 1}
                </td>
                <td className="px-3 whitespace-pre">{line || " "}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    );
  }

  return (
    <Card>
      <div className="flex flex-wrap items-center justify-between gap-2 border-b border-border px-4 py-2.5">
        <div className="flex min-w-0 items-center gap-2">
          <Icon name="file" className="text-subtle" />
          <h2 className="truncate font-mono text-sm font-medium">{baseName(file.path)}</h2>
          <span className="shrink-0 text-xs whitespace-nowrap text-muted">{formatBytes(file.size)}</span>
          {text !== null && <span className="shrink-0 text-xs whitespace-nowrap text-muted">· {lines.length} lines</span>}
          {file.executable && <Badge tone="warning">executable</Badge>}
        </div>
        {downloadButton}
      </div>
      {body}
    </Card>
  );
}

export function FilesTab() {
  const { tenant, tree, summary } = useTreeContext();
  const [params, setParams] = useSearchParams();
  const pinned = params.get("snapshot") ?? "";
  const branchParam = params.get("branch") || summary.tree.default_branch;
  const branches = useBranches(tenant, tree);
  const tip = branches.data?.find((b) => b.name === branchParam)?.tip ?? "";
  const snapshotId = pinned || tip;
  const dir = normalizeDir(params.get("path") ?? "");
  const filePath = params.get("file") ?? "";
  const detail = useSnapshot(tenant, tree, snapshotId);

  const files = useMemo(() => detail.data?.files ?? [], [detail.data]);
  const listing = useMemo(() => listDirectory(files, dir), [files, dir]);
  const selected = filePath ? files.find((f) => f.path === filePath) : undefined;

  const link = (opts: { path?: string; file?: string }) => filesPath(tenant, tree, { snapshot: snapshotId, ...opts });

  const toolbar = (
    <div className="mb-3 flex flex-wrap items-center gap-3">
      <BranchSelect tenant={tenant} tree={tree} value={branchParam} onChange={(b) => setParams({ branch: b })} />
      {pinned && (
        <Link to={filesPath(tenant, tree)} className="text-[13px] text-accent hover:underline">
          Show branch tip
        </Link>
      )}
      {detail.data && (
        <p className="min-w-0 truncate text-[13px] text-muted">
          <span className="font-mono text-fg" title={detail.data.snapshot.id}>
            {shortId(detail.data.snapshot.id)}
          </span>{" "}
          {firstLine(detail.data.snapshot.message)} · {detail.data.snapshot.author} · <RelativeTime value={detail.data.snapshot.created_at} />
        </p>
      )}
    </div>
  );

  if (!snapshotId) {
    return (
      <>
        {toolbar}
        <Card>
          {branches.isPending ? (
            <SkeletonRows label="Loading branches" />
          ) : branches.isError ? (
            <ErrorState error={branches.error} onRetry={() => void branches.refetch()} />
          ) : (
            <EmptyState title="Nothing to browse">The branch has no snapshots yet.</EmptyState>
          )}
        </Card>
      </>
    );
  }

  return (
    <>
      {toolbar}
      <div className="mb-3">
        <PathCrumbs path={selected ? selected.path : dir} link={(d) => link({ path: d })} />
      </div>
      {detail.isPending ? (
        <Card>
          <SkeletonRows label="Loading files" />
        </Card>
      ) : detail.isError ? (
        <Card>
          <ErrorState error={detail.error} onRetry={() => void detail.refetch()} />
        </Card>
      ) : filePath ? (
        selected ? (
          <FileViewer snapshot={snapshotId} file={selected} />
        ) : (
          <Card>
            <EmptyState title="File not found in this snapshot" action={
                <Link to={link({ path: parentDir(filePath) })} className="text-accent hover:underline">
                  Back to folder
                </Link>
              } />
          </Card>
        )
      ) : listing.dirs.length === 0 && listing.files.length === 0 ? (
        <Card>
          <EmptyState title={files.length === 0 ? "This snapshot has no files" : "Folder not found"} />
        </Card>
      ) : (
        <Card>
          <Table label={`Files in ${dir || "root"}`}>
            <thead>
              <tr>
                <Th>Name</Th>
                <Th className="text-right">Size</Th>
                <Th className="hidden sm:table-cell">Hash</Th>
              </tr>
            </thead>
            <tbody>
              {dir && (
                <Tr>
                  <Td colSpan={3}>
                    <Link to={link({ path: parentDir(dir) })} className="font-mono text-muted hover:text-fg" aria-label="Parent folder">
                      ..
                    </Link>
                  </Td>
                </Tr>
              )}
              {listing.dirs.map((d) => (
                <Tr key={d.path}>
                  <Td>
                    <Link to={link({ path: d.path })} className="flex items-center gap-2 font-mono hover:underline">
                      <Icon name="folder" className="text-accent" />
                      {d.name}/
                    </Link>
                  </Td>
                  <Td className="text-right text-muted tabular-nums">
                    {d.fileCount} {d.fileCount === 1 ? "file" : "files"}
                  </Td>
                  <Td className="hidden sm:table-cell" />
                </Tr>
              ))}
              {listing.files.map((f) => (
                <Tr key={f.path}>
                  <Td>
                    <Link to={link({ path: dir, file: f.path })} className="flex items-center gap-2 font-mono hover:underline">
                      <Icon name="file" className="text-subtle" />
                      {baseName(f.path)}
                      {f.executable && <Badge tone="warning">x</Badge>}
                    </Link>
                  </Td>
                  <Td className="text-right text-muted tabular-nums">{formatBytes(f.size)}</Td>
                  <Td className="hidden font-mono text-xs text-subtle sm:table-cell" title={f.hash}>
                    {f.hash.slice(0, 12)}
                  </Td>
                </Tr>
              ))}
            </tbody>
          </Table>
        </Card>
      )}
    </>
  );
}
