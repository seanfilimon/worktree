/** Merge request review form and comment threads (§13.4). */
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { CircleCheck, FileCode, MessageSquare, Reply, RotateCcw, Send } from "lucide-react";
import { useId, useMemo, useState, type FormEvent } from "react";
import { errorMessage } from "@/api/client";
import { keys } from "@/api/queries";
import type { MergeRequest } from "@/api/types";
import {
  isUnavailable,
  useMrComments,
  useTreeV05Api,
  type ReviewDecision,
} from "@/api/v05tree";
import type { ReviewCommentV07 } from "@/api/v07review";
import { useAuth } from "@/auth/context";
import { ConfirmDelete } from "@/components/files/ConfirmDelete";
import { UserAvatar } from "@/components/mr/UserAvatar";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import { Skeleton } from "@/components/ui/skeleton";
import { Textarea } from "@/components/ui/textarea";
import { shortId } from "@/lib/format";
import { cn } from "@/lib/utils";
import { useToast } from "@/toast/store";
import { RelativeTime } from "../history/shared";
import { InlineUnavailable } from "./shared";
import { SuggestionBlock, type SuggestionContext } from "../v07/Suggestion";
import { groupThreads, threadLabel, useCommentMutations, type CommentMutations, type Thread } from "./threads";

const DECISIONS: { value: ReviewDecision; label: string; hint: string }[] = [
  { value: "commented", label: "Comment", hint: "General feedback without a verdict." },
  { value: "approved", label: "Approve", hint: "Counts toward the required approvals for the current tip." },
  { value: "changes_requested", label: "Request changes", hint: "Blocks merging until you approve the current tip." },
];

/** Review decisions for reviewers who are not the author (§13.4 `POST .../review`). */
export function ReviewForm({ tenant, tree, mr }: { tenant: string; tree: string; mr: MergeRequest }) {
  const api = useTreeV05Api();
  const queryClient = useQueryClient();
  const toast = useToast();
  const id = useId();
  const [decision, setDecision] = useState<ReviewDecision>("commented");
  const [body, setBody] = useState("");
  const [touched, setTouched] = useState(false);
  const needsBody = decision !== "approved";
  const bodyError = touched && needsBody && !body.trim() ? "Add a comment explaining your review." : null;

  const review = useMutation({
    mutationFn: () => api.reviews.review(tenant, tree, mr.number, { decision, body: body.trim() || undefined }),
    meta: { inlineError: true },
    onSuccess: async (updated) => {
      if (updated && typeof updated === "object" && "number" in updated) {
        queryClient.setQueryData(keys.mergeRequest(tenant, tree, mr.number), updated);
      }
      const verb = decision === "approved" ? "Approved" : decision === "changes_requested" ? "Requested changes on" : "Reviewed";
      toast.success(`${verb} !${mr.number}`, mr.title);
      setBody("");
      setTouched(false);
      await queryClient.invalidateQueries({ queryKey: ["tree", tenant, tree] });
    },
  });

  const submit = (e: FormEvent) => {
    e.preventDefault();
    setTouched(true);
    if (needsBody && !body.trim()) return;
    review.mutate();
  };

  return (
    <Card className="gap-3 py-4">
      <CardHeader className="px-4">
        <CardTitle className="text-sm">Review</CardTitle>
      </CardHeader>
      <CardContent className="px-4">
        <form onSubmit={submit} aria-label="Submit review" className="grid gap-3" noValidate>
          <div className="grid gap-2">
            <Label htmlFor={`${id}-body`}>Review comment</Label>
            <Textarea
              id={`${id}-body`}
              value={body}
              rows={3}
              placeholder="Leave feedback for the author"
              aria-invalid={bodyError ? true : undefined}
              aria-describedby={bodyError ? `${id}-err` : undefined}
              onChange={(e) => setBody(e.target.value)}
            />
            {bodyError && (
              <p id={`${id}-err`} className="text-xs font-medium">
                {bodyError}
              </p>
            )}
          </div>
          <RadioGroup
            aria-label="Review decision"
            value={decision}
            onValueChange={(v) => setDecision(v as ReviewDecision)}
            className="grid gap-2 sm:grid-cols-3"
          >
            {DECISIONS.map((d) => (
              <label
                key={d.value}
                htmlFor={`${id}-${d.value}`}
                className={cn(
                  "flex cursor-pointer items-start gap-2 rounded-md border p-2.5 text-sm",
                  decision === d.value && "border-foreground",
                )}
              >
                <RadioGroupItem id={`${id}-${d.value}`} value={d.value} className="mt-0.5" aria-describedby={`${id}-${d.value}-hint`} />
                <span className="grid gap-0.5">
                  <span className="font-medium">{d.label}</span>
                  <span id={`${id}-${d.value}-hint`} className="text-xs text-muted-foreground">
                    {d.hint}
                  </span>
                </span>
              </label>
            ))}
          </RadioGroup>
          {review.isError && (
            <p role="alert" className="text-xs font-medium">
              {isUnavailable(review.error) ? "Review decisions are not available on this server." : errorMessage(review.error)}
            </p>
          )}
          <div>
            <Button type="submit" size="sm" disabled={review.isPending}>
              <Send aria-hidden="true" />
              Submit review
            </Button>
          </div>
        </form>
      </CardContent>
    </Card>
  );
}

function CommentItem({
  c,
  canModerate,
  m,
  sourceTip,
  ctx,
}: {
  c: ReviewCommentV07;
  canModerate: boolean;
  m: CommentMutations;
  sourceTip?: string;
  ctx?: SuggestionContext;
}) {
  const { account } = useAuth();
  const mine = account?.id === c.author_id;
  const outdated = !!(c.path && c.snapshot_id && sourceTip && c.snapshot_id !== sourceTip);
  return (
    <li className="flex gap-2.5 py-2.5" data-comment={c.id}>
      <UserAvatar name={c.author} />
      <div className="min-w-0 flex-1">
        <p className="flex flex-wrap items-center gap-x-2 gap-y-1 text-xs text-muted-foreground">
          <span className="font-medium text-foreground">{c.author}</span>
          <RelativeTime value={c.created_at} />
          {outdated && (
            <Badge variant="outline" className="text-muted-foreground" title={`Written against ${c.snapshot_id}`}>
              outdated · {shortId(c.snapshot_id)}
            </Badge>
          )}
        </p>
        {c.body && <p className="mt-0.5 text-sm break-words whitespace-pre-wrap">{c.body}</p>}
        {c.suggestion && ctx && <SuggestionBlock comment={{ ...c, suggestion: c.suggestion }} ctx={ctx} />}
      </div>
      {(mine || canModerate) && (
        <ConfirmDelete label={`Delete comment by ${c.author}`} title="Delete this comment?" onConfirm={() => m.remove.mutate(c.id)} pending={m.remove.isPending}>
          The comment is removed for everyone.
        </ConfirmDelete>
      )}
    </li>
  );
}

export function ThreadView({
  thread: t,
  sourceTip,
  canModerate,
  canComment,
  m,
  ctx,
}: {
  thread: Thread;
  sourceTip?: string;
  canModerate: boolean;
  canComment: boolean;
  m: CommentMutations;
  /** v0.7: renders suggestions (with Apply). */
  ctx?: SuggestionContext;
}) {
  const { account } = useAuth();
  const id = useId();
  const root = t.comments[0]!;
  const resolved = !!root.resolved;
  const [replying, setReplying] = useState(false);
  const [reply, setReply] = useState("");
  const label = threadLabel(t);
  const canResolve = canModerate || account?.id === root.author_id;

  const sendReply = (e: FormEvent) => {
    e.preventDefault();
    if (!reply.trim()) return;
    m.add.mutate(
      { body: reply.trim(), path: t.path, line: t.line, snapshot_id: sourceTip },
      {
        onSuccess: () => {
          setReply("");
          setReplying(false);
        },
      },
    );
  };

  return (
    <article aria-label={`Thread ${label}`} data-resolved={resolved} className={cn("rounded-lg border", resolved && "bg-muted/40")}>
      <header className="flex flex-wrap items-center justify-between gap-2 border-b px-3 py-2">
        <p className={cn("flex min-w-0 items-center gap-1.5 text-xs", resolved && "text-muted-foreground")}>
          {t.path ? <FileCode aria-hidden="true" className="size-3.5 shrink-0" /> : <MessageSquare aria-hidden="true" className="size-3.5 shrink-0" />}
          <span className={cn("truncate", t.path && "font-mono font-medium")}>{label}</span>
          {resolved && (
            <Badge variant="secondary">
              <CircleCheck aria-hidden="true" />
              resolved
            </Badge>
          )}
        </p>
        <span className="flex gap-1">
          {canComment && !resolved && (
            <Button variant="ghost" size="xs" onClick={() => setReplying((r) => !r)} aria-expanded={replying}>
              <Reply aria-hidden="true" />
              Reply
            </Button>
          )}
          {canResolve && (
            <Button
              variant="ghost"
              size="xs"
              disabled={m.resolve.isPending}
              onClick={() => m.resolve.mutate({ id: root.id, resolved: !resolved })}
            >
              {resolved ? <RotateCcw aria-hidden="true" /> : <CircleCheck aria-hidden="true" />}
              {resolved ? "Reopen" : "Resolve"}
            </Button>
          )}
        </span>
      </header>
      <ol className={cn("divide-y px-3", resolved && "opacity-70")}>
        {t.comments.map((c) => (
          <CommentItem key={c.id} c={c} canModerate={canModerate} m={m} sourceTip={sourceTip} ctx={ctx} />
        ))}
      </ol>
      {replying && (
        <form onSubmit={sendReply} aria-label={`Reply to ${label}`} className="flex flex-col gap-2 border-t px-3 py-2">
          <Label htmlFor={`${id}-reply`} className="sr-only">
            Reply
          </Label>
          <Textarea id={`${id}-reply`} rows={2} value={reply} autoFocus onChange={(e) => setReply(e.target.value)} placeholder="Write a reply" />
          <div className="flex gap-2">
            <Button type="submit" size="xs" disabled={m.add.isPending || !reply.trim()}>
              Send reply
            </Button>
            <Button type="button" variant="ghost" size="xs" onClick={() => setReplying(false)}>
              Discard
            </Button>
          </div>
        </form>
      )}
    </article>
  );
}

/** Comment threads, general or anchored to a file line, with a form to start one. */
export function CommentThreads({
  tenant,
  tree,
  mr,
  sourceTip,
  files,
  canModerate,
  knownLine,
}: {
  tenant: string;
  tree: string;
  mr: MergeRequest;
  /** Current source tip: new comments are written against it; older file comments show as outdated. */
  sourceTip?: string;
  /** Changed paths offered for file comments. */
  files: readonly string[];
  /** tree:admin: may resolve / delete any comment. */
  canModerate: boolean;
  /** v0.7: source-tip line texts known from the diff (suggestion fallback). */
  knownLine?: SuggestionContext["knownLine"];
}) {
  const comments = useMrComments(tenant, tree, mr.number);
  const ctx = useMemo<SuggestionContext>(
    () => ({ tenant, tree, mrNumber: mr.number, open: mr.state === "open", knownLine }),
    [tenant, tree, mr.number, mr.state, knownLine],
  );
  const m = useCommentMutations(tenant, tree, mr.number);
  const id = useId();
  const [body, setBody] = useState("");
  const [path, setPath] = useState("");
  const [line, setLine] = useState("");
  const [touched, setTouched] = useState(false);
  const threads = useMemo(() => groupThreads(comments.data ?? []), [comments.data]);
  const open = threads.filter((t) => !t.comments[0]!.resolved).length;

  const lineNumber = line.trim() === "" ? undefined : Number(line);
  const lineError = lineNumber !== undefined && (!Number.isInteger(lineNumber) || lineNumber < 1) ? "A line number from 1." : null;
  const lineWithoutPath = lineNumber !== undefined && !path.trim() ? "Choose a file for a line comment." : null;
  const bodyError = touched && !body.trim() ? "Write a comment." : null;

  const submit = (e: FormEvent) => {
    e.preventDefault();
    setTouched(true);
    if (!body.trim() || lineError || lineWithoutPath) return;
    m.add.mutate(
      { body: body.trim(), path: path.trim() || undefined, line: path.trim() ? lineNumber : undefined, snapshot_id: sourceTip },
      {
        onSuccess: () => {
          setBody("");
          setPath("");
          setLine("");
          setTouched(false);
        },
      },
    );
  };

  return (
    <section aria-labelledby={`${id}-title`} className="flex flex-col gap-3">
      <h3 id={`${id}-title`} className="flex items-center gap-2 text-sm font-semibold">
        <MessageSquare aria-hidden="true" className="size-4" />
        Discussion
        {comments.data && (
          <span className="font-normal text-muted-foreground">
            {threads.length} {threads.length === 1 ? "thread" : "threads"}
            {threads.length > 0 && ` · ${open} open`}
          </span>
        )}
      </h3>
      {comments.isPending ? (
        <Skeleton className="h-16 w-full" />
      ) : comments.isError ? (
        isUnavailable(comments.error) ? (
          <InlineUnavailable feature="Comments" />
        ) : (
          <p className="text-sm text-muted-foreground">Could not load comments: {errorMessage(comments.error)}</p>
        )
      ) : (
        <>
          {threads.length === 0 ? (
            <p className="text-sm text-muted-foreground">No comments yet.</p>
          ) : (
            <div aria-label="Comment threads" role="list" className="flex flex-col gap-3">
              {threads.map((t) => (
                <div role="listitem" key={t.key}>
                  <ThreadView thread={t} sourceTip={sourceTip} canModerate={canModerate} canComment m={m} ctx={ctx} />
                </div>
              ))}
            </div>
          )}
          <form onSubmit={submit} aria-label="Add comment" className="grid gap-3 rounded-lg border p-3" noValidate>
            <div className="grid gap-2">
              <Label htmlFor={`${id}-body`}>New comment</Label>
              <Textarea
                id={`${id}-body`}
                rows={3}
                value={body}
                onChange={(e) => setBody(e.target.value)}
                aria-invalid={bodyError ? true : undefined}
              />
              {bodyError && <p className="text-xs font-medium">{bodyError}</p>}
            </div>
            <div className="flex flex-wrap items-end gap-3">
              <div className="grid min-w-0 flex-1 gap-1.5">
                <Label htmlFor={`${id}-path`} className="text-xs text-muted-foreground">
                  File (optional)
                </Label>
                <Input
                  id={`${id}-path`}
                  value={path}
                  list={`${id}-files`}
                  autoComplete="off"
                  spellCheck={false}
                  className="font-mono text-xs"
                  placeholder="src/app.ts"
                  onChange={(e) => setPath(e.target.value)}
                />
                <datalist id={`${id}-files`}>
                  {files.map((f) => (
                    <option key={f} value={f} />
                  ))}
                </datalist>
              </div>
              <div className="grid w-24 gap-1.5">
                <Label htmlFor={`${id}-line`} className="text-xs text-muted-foreground">
                  Line
                </Label>
                <Input
                  id={`${id}-line`}
                  value={line}
                  type="number"
                  min={1}
                  inputMode="numeric"
                  className="tabular-nums"
                  aria-invalid={lineError || lineWithoutPath ? true : undefined}
                  onChange={(e) => setLine(e.target.value)}
                />
              </div>
            </div>
            {(lineError || lineWithoutPath) && <p className="text-xs font-medium">{lineError ?? lineWithoutPath}</p>}
            {m.add.isError && (
              <p role="alert" className="text-xs font-medium">
                {errorMessage(m.add.error)}
              </p>
            )}
            <div>
              <Button type="submit" size="sm" disabled={m.add.isPending}>
                <MessageSquare aria-hidden="true" />
                Add comment
              </Button>
            </div>
          </form>
        </>
      )}
    </section>
  );
}
