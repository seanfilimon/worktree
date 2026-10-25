import { useMutation } from "@tanstack/react-query";
import { Binary, Download, EyeOff, FileStack, FileWarning } from "lucide-react";
import { useMemo, useState } from "react";
import type { SnapshotFile } from "@/api/types";
import { isUnavailable, useStagedFile, useStagedFiles, useV06Api, type StagedSnapshotV06 } from "@/api/v06";
import { CodeView } from "@/components/files/CodeView";
import { EmptyState, ErrorState, LoadingRows, RelativeTime } from "@/components/files/states";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from "@/components/ui/sheet";
import { UnavailableNotice } from "@/components/v06/UnavailableNotice";
import { detectLanguage } from "@/lib/code";
import { baseName, decodeText, downloadBytes, splitLines } from "@/lib/files";
import { formatBytes, formatNumber, shortId } from "@/lib/format";
import { cn } from "@/lib/utils";
import { useTreeContext } from "../context";

const PREVIEW_LIMIT = 1024 * 1024;

function StagedFileView({ staged, file }: { staged: StagedSnapshotV06; file: SnapshotFile }) {
  const { tenant, tree } = useTreeContext();
  const api = useV06Api();
  const previewable = file.size <= PREVIEW_LIMIT;
  const blob = useStagedFile(tenant, tree, staged.snapshot_id, file.path, previewable);
  const text = useMemo(() => (blob.data ? decodeText(blob.data) : null), [blob.data]);
  const lines = useMemo(() => (text === null ? [] : splitLines(text)), [text]);
  const changed = (staged.files_changed ?? []).includes(file.path);

  const download = useMutation({
    mutationFn: async () => blob.data ?? (await api.staged.file(tenant, tree, staged.snapshot_id, file.path)),
    onSuccess: (bytes) => downloadBytes(bytes, baseName(file.path)),
  });

  let body;
  if (!previewable) {
    body = (
      <EmptyState icon={FileWarning} title="File too large to preview">
        Files over {formatBytes(PREVIEW_LIMIT)} are not loaded in the browser. Download it instead.
      </EmptyState>
    );
  } else if (blob.isPending) {
    body = <LoadingRows rows={8} label="Loading file" />;
  } else if (blob.isError) {
    body = <ErrorState error={blob.error} onRetry={() => void blob.refetch()} />;
  } else if (text === null) {
    body = (
      <EmptyState icon={Binary} title="Binary file">
        {formatBytes(file.size)} of binary data cannot be shown inline.
      </EmptyState>
    );
  } else {
    body = <CodeView lines={lines} language={detectLanguage(file.path)} wrap={false} anchors={false} label={`Staged contents of ${file.path}`} />;
  }

  return (
    <section aria-label={`Staged file ${file.path}`} className="flex min-w-0 flex-col overflow-hidden rounded-lg border">
      <div className="flex flex-wrap items-center gap-2 border-b px-3 py-2">
        <span className="min-w-0 flex-1 truncate font-mono text-[13px] font-semibold">{file.path}</span>
        {changed && <Badge>changed</Badge>}
        <span className="text-xs text-muted-foreground tabular-nums">{formatBytes(file.size)}</span>
        <Button variant="outline" size="icon-sm" aria-label={`Download ${file.path}`} disabled={download.isPending} onClick={() => download.mutate()}>
          <Download aria-hidden="true" />
        </Button>
      </div>
      {body}
    </section>
  );
}

interface StagedFilesSheetProps {
  staged: StagedSnapshotV06 | null;
  onClose: () => void;
}

/** Browse the working manifest of a staged snapshot (v0.6 `{tree}/staged/{snapshot}/files`). */
export function StagedFilesSheet({ staged, onClose }: StagedFilesSheetProps) {
  const { tenant, tree } = useTreeContext();
  const listing = useStagedFiles(tenant, tree, staged?.snapshot_id ?? "", staged !== null);
  const [filter, setFilter] = useState("");
  const [onlyChanged, setOnlyChanged] = useState(true);
  const [selected, setSelected] = useState("");
  const changed = useMemo(() => new Set(staged?.files_changed ?? []), [staged]);
  const files = useMemo(() => {
    const all = listing.data?.files ?? [];
    const f = filter.trim().toLowerCase();
    return all.filter((x) => (!onlyChanged || changed.size === 0 || changed.has(x.path)) && (!f || x.path.toLowerCase().includes(f)));
  }, [listing.data, filter, onlyChanged, changed]);
  const current = listing.data?.files.find((f) => f.path === selected) ?? files[0];

  return (
    <Sheet
      open={staged !== null}
      onOpenChange={(o) => {
        if (!o) {
          setSelected("");
          setFilter("");
          onClose();
        }
      }}
    >
      <SheetContent className="w-full gap-0 overflow-y-auto sm:max-w-5xl">
        <SheetHeader className="border-b">
          <SheetTitle className="flex items-center gap-2">
            <FileStack aria-hidden="true" className="size-4" />
            Staged files
          </SheetTitle>
          {staged && (
            <SheetDescription>
              <span className="font-medium text-foreground">{staged.author}</span> on <span className="font-mono">{staged.branch_name}</span> · staged{" "}
              <RelativeTime value={staged.staged_at} /> · <span className="font-mono">{shortId(staged.snapshot_id)}</span>
            </SheetDescription>
          )}
        </SheetHeader>
        {staged &&
          (listing.isPending ? (
            <LoadingRows rows={8} label="Loading staged files" />
          ) : listing.isError ? (
            isUnavailable(listing.error) ? (
              <div className="p-4">
                <UnavailableNotice feature="Browsing staged files" />
              </div>
            ) : (
              <ErrorState error={listing.error} onRetry={() => void listing.refetch()} />
            )
          ) : (
            <div className="flex flex-col gap-3 p-4">
              {listing.data.restricted_count > 0 && (
                <Alert aria-label="Restricted paths">
                  <EyeOff aria-hidden="true" />
                  <AlertTitle>
                    {formatNumber(listing.data.restricted_count)} {listing.data.restricted_count === 1 ? "path is" : "paths are"} hidden
                  </AlertTitle>
                  <AlertDescription>An access policy or license rule keeps you from reading them.</AlertDescription>
                </Alert>
              )}
              <div className="grid gap-3 md:grid-cols-[18rem_minmax(0,1fr)]">
                <div className="flex min-w-0 flex-col gap-2">
                  <Input
                    type="search"
                    value={filter}
                    onChange={(e) => setFilter(e.target.value)}
                    placeholder="Filter files"
                    aria-label="Filter staged files"
                    className="h-8 font-mono text-xs"
                  />
                  {changed.size > 0 && (
                    <div className="flex gap-1 text-xs" role="group" aria-label="Show">
                      <Button size="sm" variant={onlyChanged ? "default" : "outline"} className="h-7" aria-pressed={onlyChanged} onClick={() => setOnlyChanged(true)}>
                        Changed ({changed.size})
                      </Button>
                      <Button size="sm" variant={onlyChanged ? "outline" : "default"} className="h-7" aria-pressed={!onlyChanged} onClick={() => setOnlyChanged(false)}>
                        All ({listing.data.files.length})
                      </Button>
                    </div>
                  )}
                  {files.length === 0 ? (
                    <p className="rounded-md border border-dashed px-3 py-4 text-center text-xs text-muted-foreground">No files match.</p>
                  ) : (
                    <ul aria-label="Staged files" className="max-h-[60vh] overflow-y-auto rounded-md border font-mono text-xs">
                      {files.map((f) => (
                        <li key={f.path}>
                          <button
                            type="button"
                            aria-current={current?.path === f.path ? "true" : undefined}
                            onClick={() => setSelected(f.path)}
                            className={cn(
                              "block w-full truncate px-2 py-1.5 text-left hover:bg-muted/60",
                              current?.path === f.path && "bg-accent font-bold",
                              changed.has(f.path) ? "text-foreground" : "text-muted-foreground",
                            )}
                            title={f.path}
                          >
                            {f.path}
                          </button>
                        </li>
                      ))}
                    </ul>
                  )}
                </div>
                {current ? (
                  <StagedFileView key={current.path} staged={staged} file={current} />
                ) : (
                  <EmptyState icon={FileStack} title="No file selected" />
                )}
              </div>
            </div>
          ))}
      </SheetContent>
    </Sheet>
  );
}
