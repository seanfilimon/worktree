import { FileDiff as FileDiffIcon, FileMinus, FilePlus } from "lucide-react";
import type { FileChangeStatus } from "@/api/types";
import { cn } from "@/lib/utils";

const STATUS_ICON = { added: FilePlus, deleted: FileMinus, modified: FileDiffIcon } as const;

/** Lucide icon for a file's change status, with a screen-reader label. */
export function FileStatusIcon({ status, className }: { status: FileChangeStatus; className?: string }) {
  const Icon = STATUS_ICON[status];
  return (
    <>
      <Icon aria-hidden="true" className={cn("size-4 shrink-0", status === "modified" ? "text-muted-foreground" : "text-foreground", className)} />
      <span className="sr-only">{status}</span>
    </>
  );
}

const BLOCKS = 5;

/** Five-block monochrome bar: solid = additions, mid-grey = deletions. */
export function ChangeBar({ additions, deletions }: { additions: number; deletions: number }) {
  const total = additions + deletions;
  let add = total === 0 ? 0 : Math.round((additions / total) * BLOCKS);
  if (additions > 0 && add === 0) add = 1;
  if (deletions > 0 && add === BLOCKS) add = BLOCKS - 1;
  const del = total === 0 ? 0 : BLOCKS - add;
  return (
    <span aria-hidden="true" className="flex shrink-0 gap-px">
      {Array.from({ length: BLOCKS }, (_, i) => (
        <span
          key={i}
          className={cn("size-2 rounded-[1px]", i < add ? "bg-foreground" : i < add + del ? "bg-muted-foreground/50" : "bg-border")}
        />
      ))}
    </span>
  );
}

/** "+12 −3" in monospace; zero sides are dimmed. */
export function ChangeCounts({ additions, deletions, className }: { additions: number; deletions: number; className?: string }) {
  return (
    <span className={cn("shrink-0 font-mono text-xs tabular-nums", className)}>
      <span aria-hidden="true">
        <span className={cn("font-semibold", additions === 0 && "font-normal text-muted-foreground")}>+{additions}</span>{" "}
        <span className="text-muted-foreground">−{deletions}</span>
      </span>
      <span className="sr-only">
        {additions} additions, {deletions} deletions
      </span>
    </span>
  );
}
