import { CircleCheck, FileDiff, GitCommitHorizontal, TriangleAlert } from "lucide-react";
import { errorMessage } from "@/api/client";
import type { CompareResult } from "@/api/types";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Skeleton } from "@/components/ui/skeleton";
import { formatNumber, shortId } from "@/lib/format";
import { cn } from "@/lib/utils";
import { branchRelation } from "./model";

const plural = (n: number, one: string, many = `${one}s`) => `${formatNumber(n)} ${n === 1 ? one : many}`;

/** "N files +A −D", with additions/deletions as text (no colour). */
export function DiffStat({ stats, className }: { stats: CompareResult["stats"]; className?: string }) {
  return (
    <span className={cn("inline-flex flex-wrap items-center gap-x-2 tabular-nums", className)}>
      <span>{plural(stats.files, "file")} changed</span>
      <span className="font-semibold" aria-label={`${stats.additions} additions`}>
        +{formatNumber(stats.additions)}
      </span>
      <span className="text-muted-foreground" aria-label={`${stats.deletions} deletions`}>
        −{formatNumber(stats.deletions)}
      </span>
    </span>
  );
}

/**
 * Live preview of what merging `source` into `target` would bring in, from
 * `compare?from=<target>&to=<source>`.
 */
export function ChangeSummary({
  compare,
  pending,
  error,
  stale = false,
}: {
  compare: CompareResult | undefined;
  pending: boolean;
  error: unknown;
  /** Showing the previous comparison while a new one loads. */
  stale?: boolean;
}) {
  if (error) {
    return <p className="text-sm text-muted-foreground">Could not compare branches: {errorMessage(error)}</p>;
  }
  if (pending || !compare) {
    return (
      <div aria-label="Comparing branches" className="flex flex-col gap-2">
        <Skeleton className="h-4 w-48" />
        <Skeleton className="h-4 w-32" />
      </div>
    );
  }
  const relation = branchRelation(compare);
  return (
    <div aria-busy={stale} className={cn("flex flex-col gap-3 transition-opacity", stale && "opacity-60")}>
      <div className="flex flex-wrap items-center gap-x-4 gap-y-1 text-sm">
        <span className="inline-flex items-center gap-1.5">
          <FileDiff aria-hidden className="size-4 text-muted-foreground" />
          <DiffStat stats={compare.stats} />
        </span>
        <span className="inline-flex items-center gap-1.5 tabular-nums">
          <GitCommitHorizontal aria-hidden className="size-4 text-muted-foreground" />
          {plural(compare.snapshots.length, "snapshot")}
        </span>
      </div>
      {relation === "up_to_date" && (
        <Alert>
          <CircleCheck aria-hidden />
          <AlertTitle>Already merged</AlertTitle>
          <AlertDescription>The target branch already contains every snapshot of the source branch.</AlertDescription>
        </Alert>
      )}
      {relation === "diverged" && (
        <Alert className="border-foreground/60">
          <TriangleAlert aria-hidden />
          <AlertTitle>Branches have diverged</AlertTitle>
          <AlertDescription>
            The target moved since the merge base {shortId(compare.merge_base)}. Merge the target into the source locally (wt merge)
            and push before merging.
          </AlertDescription>
        </Alert>
      )}
    </div>
  );
}
