import { useEffect, useMemo, useState, type MouseEvent } from "react";
import { Link, useLocation, useNavigate } from "react-router";
import { Button } from "@/components/ui/button";
import { tokenize, type Language, type Token } from "@/lib/code";
import { formatNumber } from "@/lib/format";
import { cn } from "@/lib/utils";
import { lineHash, parseLineHash } from "./lines";

/** Lines rendered before a "show all" button, to keep huge files responsive. */
const INITIAL_LINES = 2000;

const TOKEN_CLASS: Record<Token["kind"], string | undefined> = {
  plain: undefined,
  keyword: "font-bold",
  comment: "italic text-muted-foreground",
  string: undefined,
};

/** One line of tokens with monochrome emphasis. */
export function CodeTokens({ tokens }: { tokens: readonly Token[] }) {
  return tokens.map((t, j) =>
    TOKEN_CLASS[t.kind] ? (
      <span key={j} className={TOKEN_CLASS[t.kind]}>
        {t.text}
      </span>
    ) : (
      t.text
    ),
  );
}

interface CodeViewProps {
  lines: readonly string[];
  language: Language | null;
  wrap: boolean;
  /** Accessible name of the scrollable code region. */
  label: string;
  /** Line numbers link to `#L<n>`; shift-click selects a range. */
  anchors?: boolean;
  className?: string;
}

/** Monochrome code listing with line numbers and `#L10` / `#L10-L20` anchors. */
export function CodeView({ lines, language, wrap, label, anchors = true, className }: CodeViewProps) {
  const location = useLocation();
  const navigate = useNavigate();
  const selection = anchors ? parseLineHash(location.hash) : null;
  const [showAll, setShowAll] = useState(false);
  const shown = showAll || lines.length <= INITIAL_LINES || (selection?.end ?? 0) > INITIAL_LINES ? lines.length : INITIAL_LINES;
  const tokens = useMemo(() => tokenize(lines.slice(0, shown), language), [lines, shown, language]);
  const start = selection?.start;

  useEffect(() => {
    if (start === undefined) return;
    document.getElementById(`L${start}`)?.scrollIntoView?.({ block: "center" });
  }, [start]);

  const onLineClick = (e: MouseEvent<HTMLAnchorElement>, n: number) => {
    if (!e.shiftKey || !selection) return;
    e.preventDefault();
    const range = { start: Math.min(selection.start, n), end: Math.max(selection.start, n) };
    void navigate({ search: location.search, hash: lineHash(range) }, { replace: true, preventScrollReset: true });
  };

  return (
    <div className={className}>
      <div
        role="region"
        aria-label={label}
        tabIndex={0}
        className="overflow-auto font-mono text-[12.5px] leading-5 outline-none focus-visible:ring-2 focus-visible:ring-ring/50"
      >
        <table className="w-full border-collapse">
          <tbody>
            {tokens.map((line, i) => {
              const n = i + 1;
              const selected = selection !== null && n >= selection.start && n <= selection.end;
              return (
                <tr
                  key={n}
                  id={`L${n}`}
                  data-highlighted={selected ? "" : undefined}
                  className={cn("scroll-mt-24", selected ? "bg-accent" : "hover:bg-muted/60")}
                >
                  <td
                    className={cn(
                      "sticky left-0 w-px border-r bg-background px-3 text-right align-top text-muted-foreground tabular-nums select-none",
                      selected && "bg-accent font-bold text-foreground shadow-[inset_2px_0_0_var(--foreground)]",
                    )}
                  >
                    {anchors ? (
                      <Link
                        to={{ search: location.search, hash: `L${n}` }}
                        replace
                        preventScrollReset
                        onClick={(e) => onLineClick(e, n)}
                        aria-label={`Line ${n}`}
                        className="hover:text-foreground hover:underline"
                      >
                        {n}
                      </Link>
                    ) : (
                      n
                    )}
                  </td>
                  <td className={cn("px-3", wrap ? "break-all whitespace-pre-wrap" : "whitespace-pre")}>
                    {line.length === 0 ? " " : <CodeTokens tokens={line} />}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
      {shown < lines.length && (
        <div className="flex items-center justify-center gap-3 border-t p-3 text-sm text-muted-foreground">
          Showing {formatNumber(shown)} of {formatNumber(lines.length)} lines.
          <Button variant="outline" size="sm" onClick={() => setShowAll(true)}>
            Show all lines
          </Button>
        </div>
      )}
    </div>
  );
}
