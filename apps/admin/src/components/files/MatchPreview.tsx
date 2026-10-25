import { CircleSlash } from "lucide-react";
import type { ReactNode } from "react";
import { formatNumber } from "@/lib/format";

interface MatchPreviewProps {
  /** Singular and plural noun, e.g. ["branch", "branches"]. */
  noun: readonly [one: string, other: string];
  matches: readonly string[];
  /** Total number of candidates checked. */
  total: number;
  /** Shown when nothing matches. */
  empty: ReactNode;
  pending?: boolean;
  limit?: number;
}

/** Live list of names a glob currently matches (announced politely). */
export function MatchPreview({ noun, matches, total, empty, pending = false, limit = 12 }: MatchPreviewProps) {
  const count = (n: number) => `${formatNumber(n)} ${n === 1 ? noun[0] : noun[1]}`;
  return (
    <div aria-live="polite" className="rounded-md border bg-muted/40 p-3 text-xs">
      {pending ? (
        <p className="text-muted-foreground">Loading {noun[1]}…</p>
      ) : matches.length === 0 ? (
        <p className="flex items-start gap-1.5 text-muted-foreground">
          <CircleSlash aria-hidden="true" className="mt-px size-3.5 shrink-0" />
          <span>{empty}</span>
        </p>
      ) : (
        <>
          <p className="mb-2">
            Matches <span className="font-bold">{count(matches.length)}</span> of {formatNumber(total)}
          </p>
          <ul aria-label={`Matching ${noun[1]}`} className="flex flex-wrap gap-1">
            {matches.slice(0, limit).map((m) => (
              <li key={m} className="max-w-full truncate rounded-sm border bg-background px-1.5 py-0.5 font-mono">
                {m}
              </li>
            ))}
            {matches.length > limit && (
              <li className="px-1.5 py-0.5 text-muted-foreground">+{formatNumber(matches.length - limit)} more</li>
            )}
          </ul>
        </>
      )}
    </div>
  );
}
