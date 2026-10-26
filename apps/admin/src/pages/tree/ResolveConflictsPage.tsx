/**
 * v0.8 conflict-ui (docs/ENGINEERING_PLAN.md §16.9): resolve the conflicts
 * of a merge request in the browser. Per conflicting path a three-pane view
 * (ours = source branch, editable result, theirs = target branch), take /
 * delete choices and marker navigation; the submit writes a
 * merge-resolution snapshot on the source branch.
 */
import { useMutation, useQueryClient } from "@tanstack/react-query";
import {
  ArrowLeft,
  ChevronDown,
  ChevronUp,
  CircleCheck,
  CircleDashed,
  CircleX,
  FileWarning,
  LoaderCircle,
  RotateCw,
  Trash2,
} from "lucide-react";
import { useId, useMemo, useRef, useState, type ReactNode } from "react";
import { Link, useNavigate, useParams } from "react-router";
import { errorMessage, isApiError } from "@/api/client";
import {
  conflictKeys,
  hasConflictMarkers,
  isEditable,
  markerOffsets,
  useConflicts,
  useConflictsApi,
  type ConflictFile,
  type ConflictReport,
  type ConflictSide,
  type FileResolution,
  type ResolveTake,
} from "@/api/conflicts";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Skeleton } from "@/components/ui/skeleton";
import { Textarea } from "@/components/ui/textarea";
import { shortId } from "@/lib/format";
import { mergeRequestPath } from "@/lib/paths";
import { useToast } from "@/toast/store";
import { useTreeContext } from "./context";

/** Resolve the conflicts of a merge request in the browser (§16.9). */
export function ResolveConflictsPage() {
  const { tenant, tree } = useTreeContext();
  const { number = "" } = useParams();
  const n = Number(number);
  const valid = Number.isInteger(n) && n > 0;
  const report = useConflicts(tenant, tree, valid ? n : 0, valid);
  const back = mergeRequestPath(tenant, tree, n);
  // Bumped on an explicit reload: the edits restart from the fresh report.
  const [generation, setGeneration] = useState(0);

  if (!valid) {
    return <Notice title="Invalid merge request number" message={`“${number}” is not a merge request number.`} />;
  }

  return (
    <div className="flex flex-col gap-5">
      <header className="flex flex-col gap-2">
        <Link
          to={back}
          className="inline-flex w-fit items-center gap-1 text-sm text-muted-foreground underline-offset-4 hover:text-foreground hover:underline"
        >
          <ArrowLeft aria-hidden className="size-3.5" />
          Back to !{n}
        </Link>
        <h2 className="text-xl font-semibold tracking-tight">
          Resolve conflicts <span className="font-mono font-normal text-muted-foreground">!{n}</span>
        </h2>
        {report.data && (
          <p className="text-sm text-muted-foreground">
            The resolution is written as a snapshot on <span className="font-mono text-foreground">{report.data.source_branch}</span>{" "}
            with the tips of <span className="font-mono text-foreground">{report.data.source_branch}</span> and{" "}
            <span className="font-mono text-foreground">{report.data.target_branch}</span> as parents.
          </p>
        )}
      </header>

      {report.isPending ? (
        <div aria-label="Loading conflicts" className="flex flex-col gap-3">
          <Skeleton className="h-4 w-1/3" />
          <div className="grid gap-3 lg:grid-cols-3">
            <Skeleton className="h-64 w-full" />
            <Skeleton className="h-64 w-full" />
            <Skeleton className="h-64 w-full" />
          </div>
        </div>
      ) : report.isError ? (
        <Notice title="Could not load conflicts" message={errorMessage(report.error)}>
          <Button size="sm" variant="outline" onClick={() => void report.refetch()}>
            Retry
          </Button>
        </Notice>
      ) : report.data.files.length === 0 && report.data.total === 0 ? (
        <div className="flex flex-col items-center gap-3 rounded-lg border border-dashed py-12 text-center">
          <CircleCheck aria-hidden className="size-6 text-muted-foreground" />
          <p className="text-sm font-medium">No conflicts</p>
          <p className="text-sm text-muted-foreground">
            {report.data.reason || "The source and target branches merge cleanly."}
          </p>
          <Button asChild size="sm" variant="outline">
            <Link to={back}>Back to the merge request</Link>
          </Button>
        </div>
      ) : !report.data.resolvable ? (
        <Notice title="These conflicts cannot be resolved in the browser" message={report.data.reason || "The server refused to resolve them here."}>
          <Button asChild size="sm" variant="outline">
            <Link to={back}>Back to the merge request</Link>
          </Button>
        </Notice>
      ) : (
        <Resolver
          key={`${report.data.source_tip}:${report.data.target_tip}:${generation}`}
          report={report.data}
          tenant={tenant}
          tree={tree}
          n={n}
          onReload={() => {
            void report.refetch().then(() => setGeneration((g) => g + 1));
          }}
          reloading={report.isFetching}
        />
      )}
    </div>
  );
}

function Notice({ title, message, children }: { title: string; message: string; children?: ReactNode }) {
  return (
    <Alert>
      <CircleX aria-hidden />
      <AlertTitle className="line-clamp-none">{title}</AlertTitle>
      <AlertDescription>
        <p>{message}</p>
        {children}
      </AlertDescription>
    </Alert>
  );
}

/** The user's choice for one path. Edited content is kept while a take / delete is picked. */
interface Choice {
  mode: "edit" | "take" | "delete";
  take?: ResolveTake;
  content: string;
}

function initialChoice(f: ConflictFile): Choice | undefined {
  if (!isEditable(f)) return undefined;
  return { mode: "edit", content: f.markers ?? f.ours.text ?? f.theirs.text ?? "" };
}

function isResolved(c: Choice | undefined): boolean {
  if (!c) return false;
  if (c.mode !== "edit") return true;
  return !hasConflictMarkers(c.content);
}

function toResolution(path: string, c: Choice): FileResolution {
  if (c.mode === "delete") return { path, delete: true };
  if (c.mode === "take") return { path, take: c.take ?? "ours" };
  return { path, content: c.content };
}

interface ResolverProps {
  report: ConflictReport;
  tenant: string;
  tree: string;
  n: number;
  onReload: () => void;
  reloading: boolean;
}

function Resolver({ report, tenant, tree, n, onReload, reloading }: ResolverProps) {
  const api = useConflictsApi();
  const queryClient = useQueryClient();
  const toast = useToast();
  const navigate = useNavigate();
  const messageId = useId();
  const [choices, setChoices] = useState<Record<string, Choice | undefined>>(() =>
    Object.fromEntries(report.files.map((f) => [f.path, initialChoice(f)])),
  );
  const [selected, setSelected] = useState(report.files[0]?.path ?? "");
  const [message, setMessage] = useState("");

  const file = report.files.find((f) => f.path === selected) ?? report.files[0];
  const resolvedCount = report.files.filter((f) => isResolved(choices[f.path])).length;
  const allResolved = resolvedCount === report.files.length && !report.truncated;

  const submit = useMutation({
    mutationFn: () => {
      const files = report.files.map((f) => toResolution(f.path, choices[f.path]!));
      return api.resolve(tenant, tree, n, {
        files,
        ...(message.trim() ? { message: message.trim() } : {}),
        source_tip: report.source_tip,
        target_tip: report.target_tip,
      });
    },
    meta: { inlineError: true },
    onSuccess: async (res) => {
      const count = res.paths.length || report.files.length;
      toast.success(
        `Resolved ${count} ${count === 1 ? "conflict" : "conflicts"} of !${n}`,
        res.snapshot?.id ? `Snapshot ${shortId(res.snapshot.id)} on ${report.source_branch}` : undefined,
      );
      queryClient.removeQueries({ queryKey: conflictKeys.report(tenant, tree, n) });
      await queryClient.invalidateQueries({ queryKey: ["tree", tenant, tree] });
      void navigate(mergeRequestPath(tenant, tree, n));
    },
  });

  const update = (path: string, c: Choice) => setChoices((prev) => ({ ...prev, [path]: c }));

  return (
    <div className="flex flex-col gap-4">
      {report.truncated && (
        <Alert>
          <FileWarning aria-hidden />
          <AlertTitle className="line-clamp-none">Too many conflicting paths</AlertTitle>
          <AlertDescription>
            Only {report.files.length} of {report.total} conflicting paths are listed. Resolve them locally with{" "}
            <code className="font-mono">wt mr resolve {n}</code>.
          </AlertDescription>
        </Alert>
      )}

      {submit.isError && <ResolveError error={submit.error} onReload={onReload} reloading={reloading} />}

      <div className="grid gap-4 lg:grid-cols-[16rem_minmax(0,1fr)]">
        <nav aria-label="Conflicting files" className="flex flex-col gap-2">
          <p className="text-sm" data-resolved-count>
            <span className="font-semibold tabular-nums">{resolvedCount}</span> of{" "}
            <span className="tabular-nums">{report.files.length}</span> resolved
          </p>
          <ul className="flex flex-col gap-1">
            {report.files.map((f) => {
              const done = isResolved(choices[f.path]);
              const current = f.path === file?.path;
              return (
                <li key={f.path}>
                  <button
                    type="button"
                    aria-current={current ? "true" : undefined}
                    data-resolved={done ? "true" : "false"}
                    onClick={() => setSelected(f.path)}
                    className={
                      "flex w-full items-center gap-2 rounded-md border px-2 py-1.5 text-left text-sm hover:bg-muted " +
                      (current ? "border-foreground bg-muted" : "border-transparent")
                    }
                  >
                    {done ? (
                      <CircleCheck aria-hidden className="size-4 shrink-0" />
                    ) : (
                      <CircleDashed aria-hidden className="size-4 shrink-0 text-muted-foreground" />
                    )}
                    <span className="min-w-0 flex-1 truncate font-mono text-xs">{f.path}</span>
                    <span className="sr-only">{done ? "(resolved)" : "(unresolved)"}</span>
                  </button>
                </li>
              );
            })}
          </ul>
        </nav>

        {file && (
          <FileResolver
            key={file.path}
            file={file}
            report={report}
            choice={choices[file.path]}
            onChange={(c) => update(file.path, c)}
          />
        )}
      </div>

      <Card className="gap-3 py-4">
        <CardContent className="flex flex-col gap-3 px-4">
          <div className="grid gap-2">
            <Label htmlFor={messageId}>Snapshot message</Label>
            <Input
              id={messageId}
              value={message}
              onChange={(e) => setMessage(e.target.value)}
              placeholder={`Resolve conflicts of !${n} (${report.target_branch} into ${report.source_branch})`}
            />
            <p className="text-xs text-muted-foreground">Optional. Empty = the server default.</p>
          </div>
          <div className="flex flex-wrap items-center gap-3">
            <Button disabled={!allResolved || submit.isPending} onClick={() => submit.mutate()}>
              {submit.isPending ? <LoaderCircle aria-hidden className="animate-spin" /> : <CircleCheck aria-hidden />}
              Commit resolution
            </Button>
            {!allResolved && (
              <p className="text-xs text-muted-foreground">
                {report.truncated
                  ? "Not every conflicting path is listed: resolve locally."
                  : `Resolve every file first (${report.files.length - resolvedCount} left).`}
              </p>
            )}
          </div>
        </CardContent>
      </Card>
    </div>
  );
}

function ResolveError({ error, onReload, reloading }: { error: unknown; onReload: () => void; reloading: boolean }) {
  const code = isApiError(error) ? error.code : "";
  const status = isApiError(error) ? error.status : 0;
  let title = "Could not resolve the conflicts";
  if (code === "resolve_outdated") title = "The branches moved";
  else if (code === "quota_exceeded") title = "Storage quota exceeded";
  else if (status === 403) title = "Not allowed";
  else if (status === 400) title = "Resolution refused";
  else if (status === 409) title = "Conflict";
  return (
    <Alert data-error-code={code || undefined} className="border-foreground/60">
      <CircleX aria-hidden />
      <AlertTitle className="line-clamp-none">{title}</AlertTitle>
      <AlertDescription>
        {code === "resolve_outdated" && (
          <p>The source or target branch changed since the conflicts were loaded. Reload them and resolve again.</p>
        )}
        <p className="font-mono text-xs break-all">{errorMessage(error)}</p>
        {status === 409 && (
          <Button size="sm" variant="outline" className="mt-1" disabled={reloading} onClick={onReload}>
            <RotateCw aria-hidden className={reloading ? "animate-spin" : undefined} />
            Reload conflicts
          </Button>
        )}
      </AlertDescription>
    </Alert>
  );
}

const TAKE_LABEL: Record<ResolveTake, string> = { ours: "ours", theirs: "theirs", base: "base" };

function FileResolver({
  file: f,
  report,
  choice,
  onChange,
}: {
  file: ConflictFile;
  report: ConflictReport;
  choice: Choice | undefined;
  onChange: (c: Choice) => void;
}) {
  const editable = isEditable(f);
  const textarea = useRef<HTMLTextAreaElement>(null);
  const [current, setCurrent] = useState(-1);
  const content = choice?.content ?? "";
  const offsets = useMemo(() => markerOffsets(content), [content]);
  const editing = editable && (choice?.mode ?? "edit") === "edit";
  const branchOf: Record<ResolveTake, string> = { ours: report.source_branch, theirs: report.target_branch, base: "merge base" };

  const take = (t: ResolveTake) => onChange({ mode: "take", take: t, content });
  const remove = () => onChange({ mode: "delete", content });
  const edit = () => onChange({ mode: "edit", content });

  const jump = (dir: 1 | -1) => {
    const el = textarea.current;
    if (!el || offsets.length === 0) return;
    const len = offsets.length;
    const at = current >= 0 && current < len ? current : -1;
    const idx = dir === 1 ? (at + 1) % len : at <= 0 ? len - 1 : at - 1;
    const start = offsets[idx]!;
    const lineEnd = content.indexOf("\n", start);
    el.focus();
    el.setSelectionRange(start, lineEnd < 0 ? content.length : lineEnd);
    const line = content.slice(0, start).split("\n").length - 1;
    const lineHeight = parseFloat(getComputedStyle(el).lineHeight) || 16;
    el.scrollTop = Math.max(0, line * lineHeight - el.clientHeight / 3);
    setCurrent(idx);
  };

  const unavailable = f.restricted
    ? "You may not read this path, so its contents are not shown. Take one side or delete the file."
    : f.binary
      ? "Binary file: it cannot be edited in the browser. Take one side or delete the file."
      : f.too_large
        ? "Too large to edit in the browser. Take one side or delete the file, or resolve it locally."
        : !editable
          ? "No text to edit. Take one side or delete the file."
          : null;

  return (
    <section aria-label={`Resolve ${f.path}`} className="flex min-w-0 flex-col gap-3">
      <div className="flex flex-wrap items-center gap-2">
        <h3 className="font-mono text-sm font-semibold break-all">{f.path}</h3>
        <Badge variant="outline" className="font-mono">
          {f.kind}
        </Badge>
        {f.binary && <Badge variant="secondary">binary</Badge>}
        {f.too_large && <Badge variant="secondary">too large</Badge>}
        {f.restricted && <Badge variant="secondary">restricted</Badge>}
      </div>

      <div role="group" aria-label="Resolution" className="flex flex-wrap items-center gap-2">
        {(["ours", "theirs", "base"] as const).map((t) =>
          t === "base" && !f.base.exists ? null : (
            <Button
              key={t}
              size="sm"
              variant={choice?.mode === "take" && choice.take === t ? "default" : "outline"}
              aria-pressed={choice?.mode === "take" && choice.take === t}
              disabled={!f[t].exists}
              title={f[t].exists ? `Use the ${branchOf[t]} version` : `Does not exist on ${branchOf[t]}`}
              onClick={() => take(t)}
            >
              Take {TAKE_LABEL[t]}
            </Button>
          ),
        )}
        <Button
          size="sm"
          variant={choice?.mode === "delete" ? "default" : "outline"}
          aria-pressed={choice?.mode === "delete"}
          onClick={remove}
        >
          <Trash2 aria-hidden />
          Delete file
        </Button>
        {editable && (
          <Button size="sm" variant={editing ? "default" : "outline"} aria-pressed={editing} onClick={edit}>
            Edit
          </Button>
        )}
      </div>

      {unavailable && <p className="rounded-md border border-dashed p-3 text-sm text-muted-foreground">{unavailable}</p>}

      {choice && choice.mode !== "edit" && (
        <p data-choice={choice.mode} className="rounded-md border bg-muted p-3 text-sm">
          {choice.mode === "delete" ? (
            <>The file is deleted on {report.source_branch}.</>
          ) : (
            <>
              Resolved with the <span className="font-semibold">{TAKE_LABEL[choice.take ?? "ours"]}</span> version (
              <span className="font-mono">{branchOf[choice.take ?? "ours"]}</span>).
            </>
          )}
          {editable && <> Choose Edit to go back to the edited content.</>}
        </p>
      )}

      {editable && (
        <div className="grid gap-3 xl:grid-cols-3">
          <SidePane title="Ours" branch={report.source_branch} side={f.ours} />
          <div className="flex min-w-0 flex-col gap-2">
            <div className="flex flex-wrap items-center justify-between gap-2">
              <span className="text-sm font-medium">Result</span>
              <span data-marker-count className="text-xs text-muted-foreground tabular-nums">
                {offsets.length === 0 ? "No conflict markers" : `${offsets.length} ${offsets.length === 1 ? "conflict" : "conflicts"} left`}
              </span>
            </div>
            <div className="flex items-center gap-1">
              <Button size="sm" variant="outline" disabled={!editing || offsets.length === 0} onClick={() => jump(-1)}>
                <ChevronUp aria-hidden />
                Previous conflict
              </Button>
              <Button size="sm" variant="outline" disabled={!editing || offsets.length === 0} onClick={() => jump(1)}>
                <ChevronDown aria-hidden />
                Next conflict
              </Button>
            </div>
            <Textarea
              ref={textarea}
              aria-label={`Result for ${f.path}`}
              value={content}
              readOnly={!editing}
              spellCheck={false}
              wrap="off"
              onChange={(e) => {
                setCurrent(-1);
                onChange({ mode: "edit", content: e.target.value });
              }}
              className={"h-96 resize-y font-mono text-xs [field-sizing:fixed] md:text-xs" + (editing ? "" : " opacity-60")}
            />
          </div>
          <SidePane title="Theirs" branch={report.target_branch} side={f.theirs} />
        </div>
      )}

      {editable && f.base.exists && typeof f.base.text === "string" && <BasePane text={f.base.text} />}
    </section>
  );
}

function SidePane({ title, branch, side }: { title: string; branch: string; side: ConflictSide }) {
  return (
    <Card role="region" aria-label={`${title} (${branch})`} className="min-w-0 gap-2 py-3">
      <CardHeader className="px-3">
        <CardTitle className="flex items-center gap-1.5 text-sm">
          {title}
          <span className="font-mono font-normal text-muted-foreground">{branch}</span>
        </CardTitle>
      </CardHeader>
      <CardContent className="px-3">
        {!side.exists ? (
          <p className="text-sm text-muted-foreground italic">Does not exist on {branch}.</p>
        ) : typeof side.text === "string" ? (
          <pre
            tabIndex={0}
            className="h-96 overflow-auto rounded-md border bg-muted p-2 font-mono text-xs whitespace-pre"
          >
            {side.text}
          </pre>
        ) : (
          <p className="text-sm text-muted-foreground italic">Contents not available.</p>
        )}
      </CardContent>
    </Card>
  );
}

function BasePane({ text }: { text: string }) {
  const [open, setOpen] = useState(false);
  return (
    <Collapsible open={open} onOpenChange={setOpen} className="flex flex-col gap-2">
      <CollapsibleTrigger asChild>
        <Button size="sm" variant="ghost" className="w-fit">
          {open ? <ChevronUp aria-hidden /> : <ChevronDown aria-hidden />}
          {open ? "Hide base" : "Show base"}
        </Button>
      </CollapsibleTrigger>
      <CollapsibleContent>
        <pre aria-label="Base" className="max-h-72 overflow-auto rounded-md border bg-muted p-2 font-mono text-xs whitespace-pre">
          {text}
        </pre>
      </CollapsibleContent>
    </Collapsible>
  );
}
