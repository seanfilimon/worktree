import { Check, ChevronRight, ChevronsDownUp, ChevronsUpDown, Columns2, Rows2 } from "lucide-react";
import { memo, useCallback, useId, useMemo, useState } from "react";
import type { FileDiff } from "@/api/types";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";
import { Label } from "@/components/ui/label";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { baseName, parentDir } from "@/lib/files";
import { formatNumber } from "@/lib/format";
import { cn } from "@/lib/utils";
import { DiffBody, type DiffView } from "./DiffBody";
import { ChangeBar, ChangeCounts, FileStatusIcon } from "./parts";

const VIEW_KEY = "wt.diff.view";

function readView(): DiffView {
  try {
    return localStorage.getItem(VIEW_KEY) === "split" ? "split" : "unified";
  } catch {
    return "unified";
  }
}

function writeView(v: DiffView) {
  try {
    localStorage.setItem(VIEW_KEY, v);
  } catch {
    // Storage unavailable (private mode): the choice just isn't remembered.
  }
}

interface DiffFileCardProps {
  file: FileDiff;
  domId: string;
  view: DiffView;
  open: boolean;
  viewed: boolean;
  onOpenChange: (path: string, open: boolean) => void;
  onViewedChange: (path: string, viewed: boolean) => void;
}

const DiffFileCard = memo(function DiffFileCard({ file, domId, view, open, viewed, onOpenChange, onViewedChange }: DiffFileCardProps) {
  const viewedId = `${domId}-viewed`;
  return (
    <Collapsible open={open} onOpenChange={(o) => onOpenChange(file.path, o)} asChild>
      <section id={domId} aria-label={file.path} className="scroll-mt-4 overflow-hidden rounded-lg border bg-card">
        <header className={cn("flex flex-wrap items-center gap-x-3 gap-y-1 bg-muted/50 px-2 py-1.5", open && "border-b")}>
          <CollapsibleTrigger asChild>
            <Button variant="ghost" size="icon-xs" aria-label={`${open ? "Collapse" : "Expand"} ${file.path}`}>
              <ChevronRight aria-hidden="true" className={cn("transition-transform motion-reduce:transition-none", open && "rotate-90")} />
            </Button>
          </CollapsibleTrigger>
          <span className="flex min-w-0 flex-1 items-center gap-2">
            <FileStatusIcon status={file.status} />
            <span className={cn("truncate font-mono text-[13px]", file.status === "deleted" && "line-through decoration-foreground/40")} title={file.path}>
              {file.path}
            </span>
          </span>
          <span className="flex items-center gap-3">
            {file.binary ? (
              <span className="text-xs text-muted-foreground">binary</span>
            ) : (
              <>
                <ChangeCounts additions={file.additions} deletions={file.deletions} />
                <ChangeBar additions={file.additions} deletions={file.deletions} />
              </>
            )}
            <span className="flex items-center gap-1.5">
              <Checkbox id={viewedId} checked={viewed} onCheckedChange={(c) => onViewedChange(file.path, c === true)} />
              <Label htmlFor={viewedId} className="text-xs font-normal text-muted-foreground">
                Viewed
              </Label>
            </span>
          </span>
        </header>
        <CollapsibleContent>
          <DiffBody file={file} view={view} />
        </CollapsibleContent>
      </section>
    </Collapsible>
  );
});

function toggled(set: ReadonlySet<string>, key: string, present: boolean): Set<string> {
  const next = new Set(set);
  if (present) next.add(key);
  else next.delete(key);
  return next;
}

export interface DiffViewerProps {
  files: FileDiff[];
  className?: string;
}

/**
 * Multi-file diff viewer for CompareResult.files: file list with change
 * stats, per-file collapsible cards, unified / split layouts, line numbers,
 * hunk headers and per-file "viewed" tracking.
 */
export function DiffViewer({ files, className }: DiffViewerProps) {
  const uid = useId();
  const [view, setView] = useState<DiffView>(readView);
  const [collapsed, setCollapsed] = useState<ReadonlySet<string>>(() => new Set());
  const [viewed, setViewed] = useState<ReadonlySet<string>>(() => new Set());

  const totals = useMemo(
    () => files.reduce((acc, f) => ({ additions: acc.additions + f.additions, deletions: acc.deletions + f.deletions }), { additions: 0, deletions: 0 }),
    [files],
  );

  const onOpenChange = useCallback((path: string, open: boolean) => setCollapsed((s) => toggled(s, path, !open)), []);
  const onViewedChange = useCallback((path: string, isViewed: boolean) => {
    setViewed((s) => toggled(s, path, isViewed));
    // Marking a file viewed folds it away; un-marking brings it back.
    setCollapsed((s) => toggled(s, path, isViewed));
  }, []);

  const domId = (i: number) => `${uid}-file-${i}`;

  const jumpTo = (path: string, i: number) => {
    setCollapsed((s) => toggled(s, path, false));
    document.getElementById(domId(i))?.scrollIntoView?.({ block: "start", behavior: "smooth" });
  };

  if (files.length === 0) {
    return <p className={cn("rounded-lg border border-dashed px-4 py-10 text-center text-sm text-muted-foreground", className)}>No file changes.</p>;
  }

  const viewedCount = files.filter((f) => viewed.has(f.path)).length;

  return (
    <div className={cn("flex flex-col gap-3", className)}>
      <div role="toolbar" aria-label="Diff options" className="flex flex-wrap items-center justify-between gap-2">
        <p className="text-sm">
          <span className="font-medium">
            {formatNumber(files.length)} {files.length === 1 ? "file" : "files"} changed
          </span>{" "}
          <ChangeCounts additions={totals.additions} deletions={totals.deletions} className="text-sm" />
          <span className="ml-3 text-xs text-muted-foreground">
            {viewedCount} / {files.length} viewed
          </span>
        </p>
        <div className="flex flex-wrap items-center gap-2">
          <Button variant="outline" size="sm" onClick={() => setCollapsed(new Set())}>
            <ChevronsUpDown aria-hidden="true" />
            Expand all
          </Button>
          <Button variant="outline" size="sm" onClick={() => setCollapsed(new Set(files.map((f) => f.path)))}>
            <ChevronsDownUp aria-hidden="true" />
            Collapse all
          </Button>
          <ToggleGroup
            type="single"
            variant="outline"
            size="sm"
            aria-label="Diff layout"
            value={view}
            onValueChange={(v) => {
              if (v !== "unified" && v !== "split") return;
              setView(v);
              writeView(v);
            }}
          >
            <ToggleGroupItem value="unified" aria-label="Unified view">
              <Rows2 aria-hidden="true" />
              <span className="hidden sm:inline">Unified</span>
            </ToggleGroupItem>
            <ToggleGroupItem value="split" aria-label="Split view">
              <Columns2 aria-hidden="true" />
              <span className="hidden sm:inline">Split</span>
            </ToggleGroupItem>
          </ToggleGroup>
        </div>
      </div>

      <div className="grid gap-4 lg:grid-cols-[15rem_minmax(0,1fr)]">
        <nav aria-label="Changed files" className="hidden lg:block">
          <div className="sticky top-4 max-h-[calc(100vh-6rem)] overflow-y-auto rounded-lg border">
            <ul className="p-1">
              {files.map((f, i) => (
                <li key={f.path}>
                  <button
                    type="button"
                    onClick={() => jumpTo(f.path, i)}
                    title={f.path}
                    className="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-xs outline-none hover:bg-accent focus-visible:ring-[3px] focus-visible:ring-ring/50"
                  >
                    <FileStatusIcon status={f.status} className="size-3.5" />
                    <span className={cn("min-w-0 flex-1 truncate", viewed.has(f.path) && "text-muted-foreground")}>
                      <span className="font-mono">{baseName(f.path)}</span>
                      {parentDir(f.path) && <span className="ml-1 text-muted-foreground">{parentDir(f.path)}</span>}
                    </span>
                    {viewed.has(f.path) ? (
                      <>
                        <Check aria-hidden="true" className="size-3.5 shrink-0 text-muted-foreground" />
                        <span className="sr-only">viewed</span>
                      </>
                    ) : (
                      !f.binary && <ChangeBar additions={f.additions} deletions={f.deletions} />
                    )}
                  </button>
                </li>
              ))}
            </ul>
          </div>
        </nav>

        <div className="flex min-w-0 flex-col gap-3">
          {files.map((f, i) => (
            <DiffFileCard
              key={f.path}
              file={f}
              domId={domId(i)}
              view={view}
              open={!collapsed.has(f.path)}
              viewed={viewed.has(f.path)}
              onOpenChange={onOpenChange}
              onViewedChange={onViewedChange}
            />
          ))}
        </div>
      </div>
    </div>
  );
}
