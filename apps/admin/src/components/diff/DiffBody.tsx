import { FileWarning, FileX } from "lucide-react";
import { memo, useMemo, type ReactNode } from "react";
import type { FileDiff } from "@/api/types";
import { parsePatch, toSplitRows, type DiffHunk, type DiffLine } from "@/lib/diff";
import { formatBytes } from "@/lib/format";
import { cn } from "@/lib/utils";

export type DiffView = "unified" | "split";

const ROW_BG = { add: "bg-diff-add", del: "bg-diff-remove", context: "" } as const;
const SIGN = { add: "+", del: "-", context: " " } as const;

const numCell = "w-[1%] min-w-10 px-2 text-right align-top whitespace-nowrap text-muted-foreground select-none tabular-nums";

function Content({ line, wrap, className }: { line: DiffLine; wrap: boolean; className?: string }) {
  return (
    <td className={cn("pr-3 align-top", wrap ? "break-all whitespace-pre-wrap" : "whitespace-pre", className)}>
      <span className={cn(line.kind === "del" && "text-muted-foreground line-through decoration-foreground/40")}>{line.content}</span>
      {line.noNewline && <span className="ml-2 text-[10px] text-muted-foreground italic select-none">no newline at end of file</span>}
    </td>
  );
}

/** "+" / struck-through "-" gutter marker (hidden from assistive tech; the row's data-kind and text carry it). */
function Sign({ kind, className }: { kind: DiffLine["kind"]; className?: string }) {
  return (
    <td
      aria-hidden="true"
      className={cn("w-4 pl-1 align-top select-none", kind === "del" && "line-through", kind !== "context" && "font-semibold", className)}
    >
      {SIGN[kind]}
    </td>
  );
}

function HunkHeader({ hunk, colSpan }: { hunk: DiffHunk; colSpan: number }) {
  return (
    <tr className="bg-diff-hunk">
      <td colSpan={colSpan} className="px-3 py-1 whitespace-pre text-muted-foreground">
        {hunk.header}
      </td>
    </tr>
  );
}

function UnifiedRows({ hunks }: { hunks: DiffHunk[] }) {
  return hunks.map((h, hi) => (
    <tbody key={hi}>
      <HunkHeader hunk={h} colSpan={4} />
      {h.lines.map((l, li) => (
        <tr key={li} data-kind={l.kind} className={ROW_BG[l.kind]}>
          <td className={numCell}>{l.oldNo ?? ""}</td>
          <td className={numCell}>{l.newNo ?? ""}</td>
          <Sign kind={l.kind} />
          <Content line={l} wrap={false} />
        </tr>
      ))}
    </tbody>
  ));
}

function SplitSide({ line, side }: { line: DiffLine | null; side: "old" | "new" }) {
  const edge = side === "new" && "border-l";
  if (!line) return <td colSpan={3} className={cn("bg-muted/40", edge)} />;
  const bg = ROW_BG[line.kind];
  return (
    <>
      <td className={cn(numCell, bg, edge)}>{side === "old" ? line.oldNo : line.newNo}</td>
      <Sign kind={line.kind} className={bg} />
      <Content line={line} wrap className={cn("w-1/2", bg)} />
    </>
  );
}

function SplitRows({ hunks }: { hunks: DiffHunk[] }) {
  return hunks.map((h, hi) => (
    <tbody key={hi}>
      <HunkHeader hunk={h} colSpan={6} />
      {toSplitRows(h.lines).map((r, ri) => (
        <tr key={ri}>
          <SplitSide line={r.left} side="old" />
          <SplitSide line={r.right} side="new" />
        </tr>
      ))}
    </tbody>
  ));
}

function Placeholder({ icon, children }: { icon?: ReactNode; children: ReactNode }) {
  return (
    <div className="flex items-center justify-center gap-2 px-4 py-8 text-center text-sm text-muted-foreground">
      {icon}
      {children}
    </div>
  );
}

function sizes(f: FileDiff): string {
  if (f.status === "added") return formatBytes(f.new_size);
  if (f.status === "deleted") return formatBytes(f.old_size);
  return `${formatBytes(f.old_size)} → ${formatBytes(f.new_size)}`;
}

/** The rendered patch of one file (parsed once per patch; re-rendered only on layout change). */
export const DiffBody = memo(function DiffBody({ file, view }: { file: FileDiff; view: DiffView }) {
  const parsed = useMemo(() => parsePatch(file.patch ?? ""), [file.patch]);

  if (file.binary) {
    return <Placeholder icon={<FileX aria-hidden="true" className="size-4" />}>Binary file not shown · {sizes(file)}</Placeholder>;
  }
  if (file.too_large) {
    return (
      <Placeholder icon={<FileWarning aria-hidden="true" className="size-4" />}>File too large to display · {sizes(file)}</Placeholder>
    );
  }
  if (parsed.hunks.length === 0) {
    return <Placeholder>{file.status === "modified" ? "No textual changes" : "Empty file"}</Placeholder>;
  }

  return (
    <div className="overflow-x-auto">
      <table aria-label={`Changes to ${file.path}`} className="w-full border-collapse font-mono text-xs leading-5 [tab-size:4]">
        {view === "split" ? <SplitRows hunks={parsed.hunks} /> : <UnifiedRows hunks={parsed.hunks} />}
      </table>
    </div>
  );
});
