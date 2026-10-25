import { BookOpen, CornerLeftUp, FolderX } from "lucide-react";
import { useMemo } from "react";
import { Link } from "react-router";
import { useBlob } from "@/api/queries";
import type { SnapshotFile } from "@/api/types";
import { Badge } from "@/components/ui/badge";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { baseName, decodeText, listDirectory, parentDir } from "@/lib/files";
import { formatBytes } from "@/lib/format";
import { FileIcon, FolderIcon } from "./FileIcon";
import { MarkdownView } from "./MarkdownView";
import { EmptyState, LoadingRows } from "./states";

const README = /^readme(\.(md|markdown|mdx|txt))?$/i;
const README_LIMIT = 256 * 1024;

interface DirectoryViewProps {
  tenant: string;
  tree: string;
  snapshot: string;
  files: readonly SnapshotFile[];
  dir: string;
  hrefFor: (path: string) => string;
  resolveLink: (target: string) => string | null;
}

function Readme({
  tenant,
  tree,
  snapshot,
  file,
  resolveLink,
}: {
  tenant: string;
  tree: string;
  snapshot: string;
  file: SnapshotFile;
  resolveLink: DirectoryViewProps["resolveLink"];
}) {
  const blob = useBlob(tenant, tree, snapshot, file.path, true);
  const text = useMemo(() => (blob.data ? decodeText(blob.data) : null), [blob.data]);
  return (
    <section aria-label={baseName(file.path)} className="overflow-hidden rounded-xl border bg-card">
      <h2 className="flex items-center gap-2 border-b px-4 py-2.5 text-sm font-semibold">
        <BookOpen aria-hidden="true" className="size-4" />
        {baseName(file.path)}
      </h2>
      {blob.isPending ? (
        <LoadingRows rows={4} label="Loading readme" />
      ) : text === null ? (
        <p className="p-4 text-sm text-muted-foreground">The readme could not be displayed.</p>
      ) : /\.txt$|^readme$/i.test(baseName(file.path)) ? (
        <pre className="overflow-x-auto p-4 font-mono text-[12.5px] whitespace-pre-wrap">{text}</pre>
      ) : (
        <MarkdownView source={text} resolveLink={resolveLink} />
      )}
    </section>
  );
}

/** Directory listing (folders first) plus its README, like a repository home page. */
export function DirectoryView({ tenant, tree, snapshot, files, dir, hrefFor, resolveLink }: DirectoryViewProps) {
  const listing = useMemo(() => listDirectory(files, dir), [files, dir]);
  const readme = listing.files.find((f) => README.test(baseName(f.path)) && f.size <= README_LIMIT);

  if (listing.dirs.length === 0 && listing.files.length === 0) {
    return (
      <div className="rounded-xl border bg-card">
        <EmptyState icon={FolderX} title={files.length === 0 ? "This snapshot has no files" : "Folder not found"}>
          {files.length > 0 && (
            <>
              <span className="font-mono">{dir}</span> does not exist in this snapshot.
            </>
          )}
        </EmptyState>
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-4">
      <div className="overflow-hidden rounded-xl border bg-card">
        <Table aria-label={`Files in ${dir || "root"}`}>
          <TableHeader className="sticky top-0 bg-card">
            <TableRow>
              <TableHead>Name</TableHead>
              <TableHead className="text-right">Size</TableHead>
              <TableHead className="hidden sm:table-cell">Hash</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {dir && (
              <TableRow>
                <TableCell colSpan={3}>
                  <Link
                    to={hrefFor(parentDir(dir))}
                    className="inline-flex items-center gap-2 font-mono text-muted-foreground hover:text-foreground"
                    aria-label="Parent folder"
                  >
                    <CornerLeftUp aria-hidden="true" className="size-4" />
                    ..
                  </Link>
                </TableCell>
              </TableRow>
            )}
            {listing.dirs.map((d) => (
              <TableRow key={d.path}>
                <TableCell>
                  <Link to={hrefFor(d.path)} className="inline-flex items-center gap-2 font-mono font-medium hover:underline">
                    <FolderIcon />
                    {d.name}/
                  </Link>
                </TableCell>
                <TableCell className="text-right text-muted-foreground tabular-nums">
                  {d.fileCount} {d.fileCount === 1 ? "file" : "files"}
                </TableCell>
                <TableCell className="hidden sm:table-cell" />
              </TableRow>
            ))}
            {listing.files.map((f) => (
              <TableRow key={f.path}>
                <TableCell>
                  <Link to={hrefFor(f.path)} className="inline-flex items-center gap-2 font-mono hover:underline">
                    <FileIcon path={f.path} executable={f.executable} />
                    {baseName(f.path)}
                    {f.executable && (
                      <Badge variant="outline" className="px-1.5 font-mono" title="Executable">
                        x
                      </Badge>
                    )}
                  </Link>
                </TableCell>
                <TableCell className="text-right text-muted-foreground tabular-nums">{formatBytes(f.size)}</TableCell>
                <TableCell className="hidden font-mono text-xs text-muted-foreground sm:table-cell" title={f.hash}>
                  {f.hash.slice(0, 12)}
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </div>
      {readme && <Readme key={readme.path} tenant={tenant} tree={tree} snapshot={snapshot} file={readme} resolveLink={resolveLink} />}
    </div>
  );
}
