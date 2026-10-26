/**
 * The merge request's Changes view with inline review (§15.7): click a line
 * number (or its "+") to comment on that line of the source tip or suggest a
 * change; threads anchored to a path and line render under it.
 */
import { ChevronRight, ChevronsDownUp, ChevronsUpDown, FileWarning, FileX, MessageSquare, Plus } from "lucide-react";
import { Fragment, memo, useCallback, useId, useMemo, useState } from "react";
import type { FileDiff, MergeRequest } from "@/api/types";
import { useMrComments } from "@/api/v05tree";
import { ChangeBar, ChangeCounts, FileStatusIcon } from "@/components/diff/parts";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";
import { Label } from "@/components/ui/label";
import { parsePatch, type DiffLine } from "@/lib/diff";
import { formatBytes, formatNumber } from "@/lib/format";
import { cn } from "@/lib/utils";
import { ThreadView } from "../v05/MrDiscussion";
import { groupThreads, useCommentMutations, type CommentMutations, type Thread } from "../v05/threads";
import { LineComposer } from "./LineComposer";
import { newSideLines } from "./lines";
import type { SuggestionContext } from "./Suggestion";

const ROW_BG = { add: "bg-diff-add", del: "bg-diff-remove", context: "" } as const;
const SIGN = { add: "+", del: "-", context: " " } as const;
const NO_THREADS: readonly Thread[] = [];
const numCell = "w-[1%] min-w-10 px-2 text-right align-top whitespace-nowrap text-muted-foreground select-none tabular-nums";

interface Anchor {
  path: string;
  line: number;
}

interface FileCardProps {
  file: FileDiff;
  domId: string;
  open: boolean;
  viewed: boolean;
  threads: readonly Thread[];
  composer: Anchor | null;
  canComment: boolean;
  canModerate: boolean;
  sourceTip?: string;
  tenant: string;
  tree: string;
  diffLines?: ReadonlyMap<number, string>;
  m: CommentMutations;
  ctx: SuggestionContext;
  onOpenChange: (path: string, open: boolean) => void;
  onViewedChange: (path: string, viewed: boolean) => void;
  onCompose: (a: Anchor | null) => void;
}

function sizes(f: FileDiff): string {
  if (f.status === "added") return formatBytes(f.new_size);
  if (f.status === "deleted") return formatBytes(f.old_size);
  return `${formatBytes(f.old_size)} → ${formatBytes(f.new_size)}`;
}

function LineNumberButton({ path, line, active, onClick }: { path: string; line: number; active: boolean; onClick: () => void }) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-label={`Comment on ${path} line ${line}`}
      aria-expanded={active}
      title="Comment on this line"
      className={cn(
        "group/ln flex w-full items-center justify-end gap-1 rounded-sm px-2 tabular-nums outline-none hover:bg-foreground hover:text-background focus-visible:bg-foreground focus-visible:text-background",
        active && "bg-foreground text-background",
      )}
    >
      <Plus aria-hidden="true" className="size-3 opacity-0 group-hover/ln:opacity-100 group-focus-visible/ln:opacity-100" />
      {line}
    </button>
  );
}

const FileCard = memo(function FileCard(props: FileCardProps) {
  const { file, domId, open, viewed, threads, composer, canComment, canModerate, sourceTip, m, ctx, onOpenChange, onViewedChange, onCompose } = props;
  const viewedId = `${domId}-viewed`;
  const parsed = useMemo(() => parsePatch(file.patch ?? ""), [file.patch]);
  const byLine = useMemo(() => {
    const map = new Map<number, Thread[]>();
    for (const t of threads) if (t.line) map.set(t.line, [...(map.get(t.line) ?? []), t]);
    return map;
  }, [threads]);
  const visible = useMemo(() => {
    const s = new Set<number>();
    for (const h of parsed.hunks) for (const l of h.lines) if (l.newNo !== null) s.add(l.newNo);
    return s;
  }, [parsed]);
  // File-level threads and threads on lines the patch does not show.
  const elsewhere = threads.filter((t) => !t.line || !visible.has(t.line));
  const openThreads = threads.filter((t) => !t.comments[0]?.resolved).length;

  const renderThreads = (list: readonly Thread[]) =>
    list.map((t) => <ThreadView key={t.key} thread={t} sourceTip={sourceTip} canModerate={canModerate} canComment={canComment} m={m} ctx={ctx} />);

  const row = (l: DiffLine, key: string) => {
    const n = l.newNo;
    const here = n !== null ? (byLine.get(n) ?? []) : [];
    const composing = n !== null && composer?.path === file.path && composer.line === n;
    return (
      <Fragment key={key}>
        <tr data-kind={l.kind} data-line={n ?? undefined} className={ROW_BG[l.kind]}>
          <td className={numCell}>{l.oldNo ?? ""}</td>
          <td className={cn(numCell, canComment && n !== null && "px-0")}>
            {n !== null && canComment ? (
              <LineNumberButton path={file.path} line={n} active={composing} onClick={() => onCompose(composing ? null : { path: file.path, line: n })} />
            ) : (
              (n ?? "")
            )}
          </td>
          <td aria-hidden="true" className={cn("w-4 pl-1 align-top select-none", l.kind === "del" && "line-through", l.kind !== "context" && "font-semibold")}>
            {SIGN[l.kind]}
          </td>
          <td className="pr-3 align-top whitespace-pre">
            <span className={cn(l.kind === "del" && "text-muted-foreground line-through decoration-foreground/40")}>{l.content}</span>
            {l.noNewline && <span className="ml-2 text-[10px] text-muted-foreground italic select-none">no newline at end of file</span>}
          </td>
        </tr>
        {(here.length > 0 || composing) && (
          <tr data-thread-row={n}>
            <td colSpan={4} className="border-y bg-background p-0">
              <div
                role="group"
                aria-label={`Discussion on ${file.path} line ${n}`}
                className="sticky left-0 flex w-[min(48rem,calc(100vw-3rem))] max-w-full flex-col gap-2 p-2 font-sans text-sm whitespace-normal"
              >
                {renderThreads(here)}
                {composing && (
                  <LineComposer
                    tenant={props.tenant}
                    tree={props.tree}
                    path={file.path}
                    line={n}
                    sourceTip={sourceTip}
                    diffLines={props.diffLines}
                    m={m}
                    onClose={() => onCompose(null)}
                  />
                )}
              </div>
            </td>
          </tr>
        )}
      </Fragment>
    );
  };

  let body;
  if (file.binary) {
    body = (
      <p className="flex items-center justify-center gap-2 px-4 py-8 text-sm text-muted-foreground">
        <FileX aria-hidden="true" className="size-4" />
        Binary file not shown · {sizes(file)}
      </p>
    );
  } else if (file.too_large) {
    body = (
      <p className="flex items-center justify-center gap-2 px-4 py-8 text-sm text-muted-foreground">
        <FileWarning aria-hidden="true" className="size-4" />
        File too large to display · {sizes(file)}
      </p>
    );
  } else if (parsed.hunks.length === 0) {
    body = <p className="px-4 py-8 text-center text-sm text-muted-foreground">{file.status === "modified" ? "No textual changes" : "Empty file"}</p>;
  } else {
    body = (
      <div className="overflow-x-auto">
        <table aria-label={`Changes to ${file.path}`} className="w-full border-collapse font-mono text-xs leading-5 [tab-size:4]">
          {parsed.hunks.map((h, hi) => (
            <tbody key={hi}>
              <tr className="bg-diff-hunk">
                <td colSpan={4} className="px-3 py-1 whitespace-pre text-muted-foreground">
                  {h.header}
                </td>
              </tr>
              {h.lines.map((l, li) => row(l, `${hi}:${li}`))}
            </tbody>
          ))}
        </table>
      </div>
    );
  }

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
            {threads.length > 0 && (
              <Badge variant={openThreads > 0 ? "secondary" : "outline"} title={`${threads.length} threads, ${openThreads} open`}>
                <MessageSquare aria-hidden="true" />
                {threads.length}
                <span className="sr-only"> {threads.length === 1 ? "thread" : "threads"}</span>
              </Badge>
            )}
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
          {body}
          {elsewhere.length > 0 && (
            <div role="group" aria-label={`Other comments on ${file.path}`} className="flex flex-col gap-2 border-t p-2">
              <p className="text-xs text-muted-foreground">Comments on the file or on lines outside the diff</p>
              {renderThreads(elsewhere)}
            </div>
          )}
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

export interface ReviewDiffProps {
  tenant: string;
  tree: string;
  mr: MergeRequest;
  files: FileDiff[];
  /** Current source tip: new comments and suggestions are written against it. */
  sourceTip?: string;
  /** The merge request is open: lines can be commented on. */
  canComment: boolean;
  /** tree:admin: may resolve / delete any comment. */
  canModerate: boolean;
}

export function ReviewDiff({ tenant, tree, mr, files, sourceTip, canComment, canModerate }: ReviewDiffProps) {
  const uid = useId();
  const comments = useMrComments(tenant, tree, mr.number);
  const m = useCommentMutations(tenant, tree, mr.number);
  const [collapsed, setCollapsed] = useState<ReadonlySet<string>>(() => new Set());
  const [viewed, setViewed] = useState<ReadonlySet<string>>(() => new Set());
  const [composer, setComposer] = useState<Anchor | null>(null);

  const lines = useMemo(() => newSideLines(files), [files]);
  const knownLine = useCallback((path: string, line: number) => lines.get(path)?.get(line), [lines]);
  const ctx = useMemo<SuggestionContext>(
    () => ({ tenant, tree, mrNumber: mr.number, open: mr.state === "open", knownLine }),
    [tenant, tree, mr.number, mr.state, knownLine],
  );

  const byPath = useMemo(() => {
    const map = new Map<string, Thread[]>();
    for (const t of groupThreads(comments.data ?? [])) {
      if (!t.path) continue;
      map.set(t.path, [...(map.get(t.path) ?? []), t]);
    }
    return map;
  }, [comments.data]);

  const totals = useMemo(
    () => files.reduce((acc, f) => ({ additions: acc.additions + f.additions, deletions: acc.deletions + f.deletions }), { additions: 0, deletions: 0 }),
    [files],
  );

  const onOpenChange = useCallback((path: string, open: boolean) => setCollapsed((s) => toggled(s, path, !open)), []);
  const onViewedChange = useCallback((path: string, isViewed: boolean) => {
    setViewed((s) => toggled(s, path, isViewed));
    setCollapsed((s) => toggled(s, path, isViewed));
  }, []);
  const domId = (i: number) => `${uid}-file-${i}`;
  const viewedCount = files.filter((f) => viewed.has(f.path)).length;


  return (
    <div className="flex flex-col gap-3">
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
          {canComment && <span className="hidden text-xs text-muted-foreground md:inline">Click a line number to comment or suggest a change.</span>}
          <Button variant="outline" size="sm" onClick={() => setCollapsed(new Set())}>
            <ChevronsUpDown aria-hidden="true" />
            Expand all
          </Button>
          <Button variant="outline" size="sm" onClick={() => setCollapsed(new Set(files.map((f) => f.path)))}>
            <ChevronsDownUp aria-hidden="true" />
            Collapse all
          </Button>
        </div>
      </div>
      <div className="flex min-w-0 flex-col gap-3">
        {files.map((f, i) => (
          <FileCard
            key={f.path}
            file={f}
            domId={domId(i)}
            open={!collapsed.has(f.path)}
            viewed={viewed.has(f.path)}
            threads={byPath.get(f.path) ?? NO_THREADS}
            composer={composer?.path === f.path ? composer : null}
            canComment={canComment}
            canModerate={canModerate}
            sourceTip={sourceTip}
            tenant={tenant}
            tree={tree}
            diffLines={lines.get(f.path)}
            m={m}
            ctx={ctx}
            onOpenChange={onOpenChange}
            onViewedChange={onViewedChange}
            onCompose={setComposer}
          />
        ))}
      </div>
    </div>
  );
}
