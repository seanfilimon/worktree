import { ChevronDown, Database, Loader2, RefreshCw } from "lucide-react";
import { useState } from "react";
import { isUnavailable } from "@/api/v06";
import { ErrorState, LoadingRows } from "@/components/files/states";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { formatBytes, formatNumber, relativeTime } from "@/lib/format";
import { useToast } from "@/toast/store";
import { useCodeIndexStatus, useRebuildCodeIndex, type CodeIndexStatus } from "./searchApi";

function summary(s: CodeIndexStatus): string {
  const limit = s.memory_limit > 0 ? formatBytes(s.memory_limit) : "unlimited";
  return [
    `${formatBytes(s.memory_bytes)} of ${limit} in memory (${formatNumber(s.loaded_segments)} segments)`,
    `${formatBytes(s.disk_bytes)} on disk`,
    `${formatNumber(s.builds)} built, ${formatNumber(s.disk_loads)} loaded from disk since start`,
    `up to ${s.max_branches} branches per tree`,
  ].join(" · ");
}

/** Server admins: the persistent code index (§15.9) with per-tree rebuilds. Loads when opened. */
export function CodeIndexPanel() {
  const [open, setOpen] = useState(false);
  const status = useCodeIndexStatus(open);
  const rebuild = useRebuildCodeIndex();
  const toast = useToast();
  const [busy, setBusy] = useState<string | null>(null);

  const run = (tree?: string) => {
    setBusy(tree ?? "*");
    rebuild.mutate(tree, {
      onSuccess: (r) => toast.success("Code index rebuilt", `${r.rebuilt} ${r.rebuilt === 1 ? "tree" : "trees"}`),
      onSettled: () => setBusy(null),
    });
  };

  return (
    <Collapsible open={open} onOpenChange={setOpen} className="mt-4 rounded-xl border bg-card">
      <div className="flex items-center gap-2 px-3 py-2">
        <CollapsibleTrigger asChild>
          <Button variant="ghost" size="sm" className="gap-2" aria-label="Index status">
            <Database aria-hidden="true" />
            Index status
            <ChevronDown aria-hidden="true" className={open ? "rotate-180 transition-transform" : "transition-transform"} />
          </Button>
        </CollapsibleTrigger>
        {open && status.data && (
          <Button size="sm" variant="outline" className="ml-auto" disabled={busy !== null} onClick={() => run()}>
            {busy === "*" ? <Loader2 aria-hidden="true" className="animate-spin motion-reduce:animate-none" /> : <RefreshCw aria-hidden="true" />}
            Rebuild all
          </Button>
        )}
      </div>
      <CollapsibleContent className="border-t">
        {status.isPending ? (
          <LoadingRows rows={3} label="Loading index status" />
        ) : status.isError ? (
          isUnavailable(status.error) ? (
            <p className="p-4 text-sm text-muted-foreground">Index status is not available on this server.</p>
          ) : (
            <ErrorState error={status.error} onRetry={() => void status.refetch()} />
          )
        ) : (
          <div className="flex flex-col gap-3 p-3">
            <p role="status" className="text-xs text-muted-foreground">
              Format {status.data.format_version} · {status.data.persistent ? "on disk" : "in memory only"} · {summary(status.data)}
            </p>
            {status.data.trees.length === 0 ? (
              <p className="text-sm text-muted-foreground">No tree has been indexed yet. Trees are indexed by their first search.</p>
            ) : (
              <Table aria-label="Indexed trees">
                <TableHeader>
                  <TableRow>
                    <TableHead>Tree</TableHead>
                    <TableHead>State</TableHead>
                    <TableHead className="text-right">Branches</TableHead>
                    <TableHead className="text-right">Files</TableHead>
                    <TableHead className="text-right">Symbols</TableHead>
                    <TableHead className="text-right">Memory</TableHead>
                    <TableHead className="text-right">Disk</TableHead>
                    <TableHead>Built</TableHead>
                    <TableHead className="sr-only">Actions</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {status.data.trees.map((t) => {
                    const ref = `${t.tenant}/${t.tree}`;
                    return (
                      <TableRow key={t.tree_id}>
                        <TableCell className="font-mono text-xs font-semibold">{ref}</TableCell>
                        <TableCell>
                          <Badge variant={t.state === "ready" ? "outline" : "secondary"} title={t.error || undefined}>
                            {t.state}
                          </Badge>
                        </TableCell>
                        <TableCell className="text-right tabular-nums" title={t.branches.map((b) => b.branch).join(", ")}>
                          {t.branch_count}
                          {t.segments !== t.branch_count && <span className="text-muted-foreground"> ({t.segments} segments)</span>}
                        </TableCell>
                        <TableCell className="text-right tabular-nums">{formatNumber(t.files)}</TableCell>
                        <TableCell className="text-right tabular-nums">{formatNumber(t.symbols)}</TableCell>
                        <TableCell className="text-right tabular-nums">{formatBytes(t.memory_bytes)}</TableCell>
                        <TableCell className="text-right tabular-nums">{formatBytes(t.disk_bytes)}</TableCell>
                        <TableCell className="text-xs text-muted-foreground">{t.built_at ? relativeTime(t.built_at) : "—"}</TableCell>
                        <TableCell className="text-right">
                          <Button size="sm" variant="ghost" aria-label={`Rebuild ${ref}`} disabled={busy !== null} onClick={() => run(ref)}>
                            {busy === ref ? (
                              <Loader2 aria-hidden="true" className="animate-spin motion-reduce:animate-none" />
                            ) : (
                              <RefreshCw aria-hidden="true" />
                            )}
                          </Button>
                        </TableCell>
                      </TableRow>
                    );
                  })}
                </TableBody>
              </Table>
            )}
          </div>
        )}
      </CollapsibleContent>
    </Collapsible>
  );
}
