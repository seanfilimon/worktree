/** Suggested-change rendering and "Apply suggestion" (§15.7). */
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Check, LoaderCircle, WandSparkles } from "lucide-react";
import { useMemo } from "react";
import { keys, useBlob } from "@/api/queries";
import { v05Keys } from "@/api/v05tree";
import { v06Keys } from "@/api/v06";
import { isSuggestionOutdated, useV07ReviewApi, type ReviewCommentV07, type Suggestion } from "@/api/v07review";
import { Button } from "@/components/ui/button";
import { shortId } from "@/lib/format";
import { cn } from "@/lib/utils";
import { useToast } from "@/toast/store";
import { splitLines } from "./lines";

export interface SuggestionContext {
  tenant: string;
  tree: string;
  mrNumber: number;
  /** The merge request is open: suggestions can be applied. */
  open: boolean;
  /** Text of a line of the current source tip known from the diff (fallback for the replaced lines). */
  knownLine?: (path: string, line: number) => string | undefined;
}

/** Old (replaced) lines of a suggestion: the file at its snapshot, else lines known from the diff. */
function useReplacedLines(ctx: SuggestionContext, s: Suggestion): string[] | null {
  const blob = useBlob(ctx.tenant, ctx.tree, s.snapshot_id, s.path, s.snapshot_id !== "");
  return useMemo(() => {
    if (blob.data) {
      const lines = splitLines(new TextDecoder().decode(blob.data));
      if (s.end_line <= lines.length) return lines.slice(s.start_line - 1, s.end_line);
    }
    if (!ctx.knownLine) return null;
    const known: string[] = [];
    for (let i = s.start_line; i <= s.end_line; i++) {
      const t = ctx.knownLine(s.path, i);
      if (t === undefined) return null;
      known.push(t);
    }
    return known;
  }, [blob.data, ctx, s.path, s.start_line, s.end_line]);
}

/** Invalidates everything an applied suggestion changes (MR, comments, checks, diff, branches). */
function useApplySuggestion(ctx: SuggestionContext) {
  const api = useV07ReviewApi();
  const queryClient = useQueryClient();
  const toast = useToast();
  const { tenant, tree, mrNumber } = ctx;
  return useMutation({
    mutationFn: (id: string) => api.comments.apply(tenant, tree, mrNumber, id),
    meta: { inlineError: true },
    onSuccess: async (r) => {
      toast.success("Suggestion applied", r?.snapshot?.id ? `New snapshot ${shortId(r.snapshot.id)} on the source branch.` : undefined);
      await Promise.all(
        [
          keys.mergeRequest(tenant, tree, mrNumber),
          v05Keys.mrComments(tenant, tree, mrNumber),
          v05Keys.mrChecks(tenant, tree, mrNumber),
          v06Keys.mergeability(tenant, tree, mrNumber),
          ["tree", tenant, tree, "compare"],
          keys.branches(tenant, tree),
          ["tree", tenant, tree, "ci"],
          ["tree", tenant, tree, "mrs"],
        ].map((queryKey) => queryClient.invalidateQueries({ queryKey })),
      );
    },
    onError: (err) => {
      if (isSuggestionOutdated(err)) {
        toast.push({
          kind: "error",
          title: "Suggestion is outdated",
          message: "The source branch changed these lines since the suggestion was made. Ask for a new suggestion.",
        });
      } else {
        toast.error(err, "Could not apply the suggestion");
      }
    },
  });
}

function rangeLabel(s: Suggestion): string {
  return s.start_line === s.end_line ? `line ${s.start_line}` : `lines ${s.start_line}–${s.end_line}`;
}

/** The replaced range and the replacement as a monochrome diff block, with "Apply suggestion". */
export function SuggestionBlock({ comment: c, ctx }: { comment: ReviewCommentV07 & { suggestion: Suggestion }; ctx: SuggestionContext }) {
  const s = c.suggestion;
  const old = useReplacedLines(ctx, s);
  const apply = useApplySuggestion(ctx);
  const applied = s.applied_snapshot;
  const added = s.replacement === "" ? [] : splitLines(s.replacement);

  return (
    <figure
      aria-label={`Suggested change to ${s.path} ${rangeLabel(s)}`}
      data-suggestion={c.id}
      data-applied={applied ? true : undefined}
      className="mt-2 overflow-hidden rounded-md border"
    >
      <figcaption className="flex flex-wrap items-center justify-between gap-2 border-b bg-muted/50 px-2 py-1 text-xs">
        <span className="flex min-w-0 items-center gap-1.5">
          <WandSparkles aria-hidden="true" className="size-3.5 shrink-0" />
          <span className="font-medium">Suggested change</span>
          <span className="truncate font-mono text-muted-foreground">
            {s.path} · {rangeLabel(s)}
          </span>
        </span>
        {applied && s.applied_by && <span className="text-muted-foreground">applied by {s.applied_by}</span>}
      </figcaption>
      <div className="overflow-x-auto font-mono text-xs leading-5">
        {old ? (
          old.map((l, i) => (
            <div key={`d${i}`} data-kind="del" className="flex bg-diff-remove">
              <span aria-hidden="true" className="w-5 shrink-0 text-center font-semibold line-through select-none">
                -
              </span>
              <span className="pr-3 whitespace-pre text-muted-foreground line-through decoration-foreground/40">
                <span className="sr-only">removed: </span>
                {l}
              </span>
            </div>
          ))
        ) : (
          <p className="px-2 py-1 font-sans text-muted-foreground">Replaces {rangeLabel(s)}.</p>
        )}
        {added.length === 0 ? (
          <p className="px-2 py-1 font-sans text-muted-foreground">Deletes {rangeLabel(s)}.</p>
        ) : (
          added.map((l, i) => (
            <div key={`a${i}`} data-kind="add" className="flex bg-diff-add">
              <span aria-hidden="true" className="w-5 shrink-0 text-center font-semibold select-none">
                +
              </span>
              <span className="pr-3 font-semibold whitespace-pre">
                <span className="sr-only">added: </span>
                {l}
              </span>
            </div>
          ))
        )}
      </div>
      {(applied || ctx.open) && (
        <div className="flex items-center gap-2 border-t px-2 py-1.5">
          <Button
            size="xs"
            variant={applied ? "ghost" : "outline"}
            disabled={!!applied || apply.isPending}
            onClick={() => apply.mutate(c.id)}
            className={cn(applied && "disabled:opacity-100")}
          >
            {apply.isPending ? <LoaderCircle aria-hidden="true" className="animate-spin" /> : <Check aria-hidden="true" />}
            {applied ? `Applied as ${shortId(applied)}` : "Apply suggestion"}
          </Button>
        </div>
      )}
    </figure>
  );
}

