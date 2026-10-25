import { useMemo } from "react";
import { Link } from "react-router";
import { isUnavailable, useBlame } from "@/api/v06";
import { UnavailableNotice } from "@/components/v06/UnavailableNotice";
import { tokenize, type Language } from "@/lib/code";
import { absoluteTime, relativeTime, shortId } from "@/lib/format";
import { treePath } from "@/lib/paths";
import { cn } from "@/lib/utils";
import { blameRunStarts } from "./blame";
import { CodeTokens } from "./CodeView";
import { EmptyState, ErrorState, LoadingRows } from "./states";

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

  return (
    <div
      role="region"
      aria-label={`Blame of ${path}`}
      tabIndex={0}
      className="overflow-auto font-mono text-[12.5px] leading-5 outline-none focus-visible:ring-2 focus-visible:ring-ring/50"
    >
      <table className="w-full border-collapse">
        <tbody>
          {lines.map((l, i) => {
            const start = starts.has(i);
            return (
              <tr key={l.line} data-run-start={start ? "" : undefined} className={cn("hover:bg-muted/60", start && i > 0 && "border-t")}>
                <td className="w-px border-r bg-muted/30 px-2 align-top whitespace-nowrap">
                  {start ? (
                    <span className="flex items-baseline gap-2 text-[11.5px]" title={`${l.message}\n${l.author} · ${absoluteTime(l.at)}`}>
                      <Link
                        to={treePath(tenant, tree, `snapshots/${encodeURIComponent(l.snapshot_id)}`)}
                        className="font-semibold text-foreground hover:underline"
                        aria-label={`Snapshot ${shortId(l.snapshot_id)}: ${l.message}`}
                      >
                        {shortId(l.snapshot_id)}
                      </Link>
                      <span className="max-w-28 truncate font-sans text-foreground">{l.author}</span>
                      <time dateTime={l.at} className="font-sans text-muted-foreground">
                        {relativeTime(l.at)}
                      </time>
                    </span>
                  ) : (
                    <span aria-hidden="true">&nbsp;</span>
                  )}
                </td>
                <td className="w-px border-r px-3 text-right align-top text-muted-foreground tabular-nums select-none">{l.line}</td>
                <td className="px-3 whitespace-pre">{l.text.length === 0 ? " " : <CodeTokens tokens={tokens[i] ?? []} />}</td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}
