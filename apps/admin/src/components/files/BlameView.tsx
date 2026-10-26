import { useMemo } from "react";
import { Link } from "react-router";
import { isUnavailable, useBlame } from "@/api/v06";
import { UnavailableNotice } from "@/components/v06/UnavailableNotice";
import { tokenize, type Language } from "@/lib/code";
import { absoluteTime, relativeTime, shortId } from "@/lib/format";
import { treePath } from "@/lib/paths";
import { cn } from "@/lib/utils";
import { blameRunStarts, compactAge } from "./blame";
import { CodeTokens } from "./CodeView";
import { EmptyState, ErrorState, LoadingRows } from "./states";

/** Gutter (snapshot, author, age) and line-number column widths; the line column is pinned right after the gutter. */
const GUTTER_WIDTH = "w-[7.5rem] sm:w-[15rem]";
const LINE_WIDTH = "w-[3.5rem] sm:w-[4rem]";
/** Pinned right after the gutter. */
const LINE_LEFT = "left-[7.5rem] sm:left-[15rem]";
/**
 * Right edge of a pinned cell: an inset shadow instead of `border-r`, which
 * a collapsed-border table paints in place (it would not move with the
 * sticky cell and code would show through the gap).
 */
const PINNED_EDGE = "shadow-[inset_-1px_0_0_var(--color-border)]";

interface BlameViewProps {
  tenant: string;
  tree: string;
  /** Snapshot (revision) to blame at. */
  snapshot: string;
  path: string;
  language: Language | null;
}

/** Blame: a gutter with snapshot short id, author and age per run of lines; click a snapshot to open it. */
export function BlameView({ tenant, tree, snapshot, path, language }: BlameViewProps) {
  const blame = useBlame(tenant, tree, snapshot, path);
  const lines = useMemo(() => blame.data?.lines ?? [], [blame.data]);
  const tokens = useMemo(() => tokenize(lines.map((l) => l.text), language), [lines, language]);
  const starts = useMemo(() => blameRunStarts(lines), [lines]);

  if (blame.isPending) return <LoadingRows rows={8} label="Loading blame" />;
  if (blame.isError) {
    return isUnavailable(blame.error) ? (
      <div className="p-4">
        <UnavailableNotice feature="Blame" />
      </div>
    ) : (
      <ErrorState error={blame.error} onRetry={() => void blame.refetch()} />
    );
  }
  if (lines.length === 0) return <EmptyState title="Empty file" />;

  // hardening (§15.10): on narrow screens the gutter has a fixed width
  // (short id + compact age; author and full age from `sm` up), metadata
  // truncates instead of squeezing the code, and the gutter and line
  // numbers stay pinned while long lines scroll horizontally.
  return (
    <div
      role="region"
      aria-label={`Blame of ${path}`}
      tabIndex={0}
      className="max-w-full overflow-auto overscroll-x-contain font-mono text-[12.5px] leading-5 outline-none focus-visible:ring-2 focus-visible:ring-ring/50"
    >
      <table className="w-full border-collapse">
        <tbody>
          {lines.map((l, i) => {
            const start = starts.has(i);
            return (
              <tr key={l.line} data-run-start={start ? "" : undefined} className={cn("group", start && i > 0 && "border-t")}>
                <td data-blame-gutter="" className={cn("sticky left-0 z-10 bg-muted p-0 align-top group-hover:bg-accent", PINNED_EDGE)}>
                  {/* A fixed-width box: the table cannot grow it, long metadata truncates. */}
                  <div className={cn("overflow-hidden px-2 whitespace-nowrap", GUTTER_WIDTH)}>
                    {start ? (
                      <span className="flex min-w-0 items-baseline gap-2 text-[11.5px]" title={`${l.message}\n${l.author} · ${absoluteTime(l.at)}`}>
                        <Link
                          to={treePath(tenant, tree, `snapshots/${encodeURIComponent(l.snapshot_id)}`)}
                          className="shrink-0 font-semibold text-foreground hover:underline"
                          aria-label={`Snapshot ${shortId(l.snapshot_id)}: ${l.message}`}
                        >
                          {shortId(l.snapshot_id)}
                        </Link>
                        <span data-blame-author="" className="hidden min-w-0 flex-1 truncate font-sans text-foreground sm:block">
                          {l.author}
                        </span>
                        <time dateTime={l.at} className="ml-auto min-w-0 truncate font-sans text-muted-foreground">
                          <span className="sm:hidden">{compactAge(l.at)}</span>
                          <span className="hidden sm:inline">{relativeTime(l.at)}</span>
                        </time>
                      </span>
                    ) : (
                      <span aria-hidden="true">&nbsp;</span>
                    )}
                  </div>
                </td>
                <td
                  className={cn(
                    "sticky z-10 bg-background p-0 align-top text-muted-foreground tabular-nums select-none group-hover:bg-accent",
                    PINNED_EDGE,
                    LINE_LEFT,
                  )}
                >
                  <div className={cn("overflow-hidden px-2 text-right sm:px-3", LINE_WIDTH)}>{l.line}</div>
                </td>
                <td className="px-3 whitespace-pre group-hover:bg-muted/60">
                  {l.text.length === 0 ? " " : <CodeTokens tokens={tokens[i] ?? []} />}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}
