import { useMutation, useQueryClient } from "@tanstack/react-query";
import { CircleCheck, CircleSlash, Hand, ListTodo, Plus } from "lucide-react";
import { useId, useState, type FormEvent } from "react";
import { useSearchParams } from "react-router";
import { errorMessage } from "@/api/client";
import {
  isNotImplemented,
  TODO_PRIORITIES,
  TODO_STATUSES,
  useTodos,
  useTreeV05Api,
  type Todo,
  type TodoPriority,
  type TodoQuery,
  type TodoStatus,
} from "@/api/v05tree";
import { useAuth } from "@/auth/context";
import { Alert, AlertDescription } from "@/components/ui/alert";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
} from "@/components/ui/alert-dialog";
import { Button } from "@/components/ui/button";
import { Card, CardAction, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { Textarea } from "@/components/ui/textarea";
import { shortId } from "@/lib/format";
import { useToast } from "@/toast/store";
import { BranchSelect } from "./BranchSelect";
import { useTreeContext } from "./context";
import { EmptyState, LoadingRows, QueryError, RelativeTime } from "./history/shared";
import { BlockingBadge, FeatureUnavailable, PriorityBadge, TodoStatusBadge } from "./v05/shared";
import { TODO_STATUS_LABEL, useCanWrite } from "./v05/util";

const ALL = "all";
type StatusFilter = TodoStatus | typeof ALL;

function useInvalidateTodos() {
  const { tenant, tree } = useTreeContext();
  const queryClient = useQueryClient();
  return () =>
    Promise.all([
      queryClient.invalidateQueries({ queryKey: ["tree", tenant, tree, "todos"] }),
      queryClient.invalidateQueries({ queryKey: ["tree", tenant, tree, "dependencies"] }),
      queryClient.invalidateQueries({ queryKey: ["tree", tenant, tree, "mr-checks"] }),
    ]);
}

function CreateTodoDialog() {
  const { tenant, tree, summary } = useTreeContext();
  const api = useTreeV05Api();
  const toast = useToast();
  const invalidate = useInvalidateTodos();
  const id = useId();
  const [open, setOpen] = useState(false);
  const [title, setTitle] = useState("");
  const [details, setDetails] = useState("");
  const [priority, setPriority] = useState<TodoPriority>("medium");
  const [blocking, setBlocking] = useState(false);
  const [branch, setBranch] = useState(summary.tree.default_branch);
  const [touched, setTouched] = useState(false);

  const create = useMutation({
    mutationFn: () =>
      api.todos.create(tenant, tree, {
        title: title.trim(),
        details: details.trim() || undefined,
        priority,
        blocking,
        target_branch: branch,
      }),
    meta: { inlineError: true },
    onSuccess: async (t) => {
      toast.success("TODO created", `#${t.number} ${t.title}`);
      setOpen(false);
      await invalidate();
    },
  });

  const titleError = touched && !title.trim() ? "A title is required." : null;

  const submit = (e: FormEvent) => {
    e.preventDefault();
    setTouched(true);
    if (!title.trim()) return;
    create.mutate();
  };

  return (
    <Dialog
      open={open}
      onOpenChange={(o) => {
        setOpen(o);
        if (o) {
          setTitle("");
          setDetails("");
          setPriority("medium");
          setBlocking(false);
          setBranch(summary.tree.default_branch);
          setTouched(false);
          create.reset();
        }
      }}
    >
      <DialogTrigger asChild>
        <Button size="sm">
          <Plus aria-hidden="true" />
          New TODO
        </Button>
      </DialogTrigger>
      <DialogContent>
        <form onSubmit={submit} aria-label="Create TODO" className="grid gap-4" noValidate>
          <DialogHeader>
            <DialogTitle>New TODO</DialogTitle>
            <DialogDescription>
              Most TODOs are created by dependencies declared with <code className="font-mono">wt snapshot --depend</code>; add one
              by hand here.
            </DialogDescription>
          </DialogHeader>
          <div className="grid gap-2">
            <Label htmlFor={`${id}-title`}>Title</Label>
            <Input
              id={`${id}-title`}
              value={title}
              autoFocus
              autoComplete="off"
              aria-invalid={titleError ? true : undefined}
              aria-describedby={titleError ? `${id}-title-error` : undefined}
              onChange={(e) => setTitle(e.target.value)}
            />
            {titleError && (
              <p id={`${id}-title-error`} className="text-xs font-medium">
                {titleError}
              </p>
            )}
          </div>
          <div className="grid gap-2">
            <Label htmlFor={`${id}-details`}>
              Details <span className="font-normal text-muted-foreground">(optional)</span>
            </Label>
            <Textarea id={`${id}-details`} value={details} rows={3} onChange={(e) => setDetails(e.target.value)} />
          </div>
          <div className="flex flex-wrap items-end gap-4">
            <div className="grid gap-2">
              <Label htmlFor={`${id}-priority`}>Priority</Label>
              <Select value={priority} onValueChange={(v) => setPriority(v as TodoPriority)}>
                <SelectTrigger id={`${id}-priority`} className="w-36 capitalize">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {TODO_PRIORITIES.map((p) => (
                    <SelectItem key={p} value={p} className="capitalize">
                      {p}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <div className="grid gap-2">
              <span className="text-sm font-medium">Target branch</span>
              <BranchSelect tenant={tenant} tree={tree} value={branch} onChange={setBranch} label="Target branch" />
            </div>
          </div>
          <div className="flex items-start justify-between gap-4 rounded-md border p-3">
            <div className="grid gap-1">
              <Label htmlFor={`${id}-blocking`}>Blocking</Label>
              <p id={`${id}-blocking-desc`} className="text-xs text-muted-foreground">
                Open blocking TODOs stop merges into the target branch when its protection blocks on TODOs.
              </p>
            </div>
            <Switch id={`${id}-blocking`} checked={blocking} onCheckedChange={setBlocking} aria-describedby={`${id}-blocking-desc`} />
          </div>
          {create.isError && (
            <Alert>
              <AlertDescription className="text-foreground">{errorMessage(create.error)}</AlertDescription>
            </Alert>
          )}
          <DialogFooter>
            <Button type="button" variant="outline" onClick={() => setOpen(false)}>
              Cancel
            </Button>
            <Button type="submit" disabled={create.isPending}>
              {create.isPending ? "Creating…" : "Create TODO"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

function CompleteDialog({ todo, onDone }: { todo: Todo; onDone: (snapshot: string) => void }) {
  const id = useId();
  const [open, setOpen] = useState(false);
  const [snapshot, setSnapshot] = useState("");
  return (
    <Dialog
      open={open}
      onOpenChange={(o) => {
        setOpen(o);
        if (o) setSnapshot("");
      }}
    >
      <DialogTrigger asChild>
        <Button variant="outline" size="xs" aria-label={`Complete TODO #${todo.number}`}>
          <CircleCheck aria-hidden="true" />
          Complete
        </Button>
      </DialogTrigger>
      <DialogContent>
        <form
          aria-label="Complete TODO"
          className="grid gap-4"
          onSubmit={(e) => {
            e.preventDefault();
            setOpen(false);
            onDone(snapshot.trim());
          }}
        >
          <DialogHeader>
            <DialogTitle>
              Complete #{todo.number}: {todo.title}
            </DialogTitle>
            <DialogDescription>
              Optionally record the snapshot that did the work. Pushing a snapshot whose message contains{" "}
              <code className="font-mono">Completes: #{todo.number}</code> does this automatically.
            </DialogDescription>
          </DialogHeader>
          <div className="grid gap-2">
            <Label htmlFor={`${id}-snapshot`}>
              Completing snapshot <span className="font-normal text-muted-foreground">(optional)</span>
            </Label>
            <Input
              id={`${id}-snapshot`}
              value={snapshot}
              autoComplete="off"
              spellCheck={false}
              className="font-mono"
              placeholder="snapshot id"
              onChange={(e) => setSnapshot(e.target.value)}
            />
          </div>
          <DialogFooter>
            <Button type="button" variant="outline" onClick={() => setOpen(false)}>
              Back
            </Button>
            <Button type="submit">Mark done</Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

type TodoAction = { kind: "claim" } | { kind: "complete"; snapshot: string } | { kind: "cancel" };

function TodoActions({ todo }: { todo: Todo }) {
  const { tenant, tree } = useTreeContext();
  const api = useTreeV05Api();
  const toast = useToast();
  const invalidate = useInvalidateTodos();
  const { account } = useAuth();

  const act = useMutation({
    mutationFn: (a: TodoAction) => {
      switch (a.kind) {
        case "claim":
          return api.todos.claim(tenant, tree, todo.number);
        case "complete":
          return api.todos.complete(tenant, tree, todo.number, a.snapshot || undefined);
        case "cancel":
          return api.todos.cancel(tenant, tree, todo.number);
      }
    },
    onSuccess: async (t, a) => {
      const verb = a.kind === "claim" ? "Claimed" : a.kind === "complete" ? "Completed" : "Cancelled";
      toast.success(`${verb} #${todo.number}`, t?.title ?? todo.title);
      await invalidate();
    },
  });

  const closed = todo.status === "done" || todo.status === "cancelled";
  if (closed) return null;
  const mine = todo.assignee_id !== undefined && todo.assignee_id === account?.id;

  return (
    <span className="flex justify-end gap-1">
      {!mine && (
        <Button
          variant="outline"
          size="xs"
          aria-label={`Claim TODO #${todo.number}`}
          disabled={act.isPending}
          onClick={() => act.mutate({ kind: "claim" })}
        >
          <Hand aria-hidden="true" />
          Claim
        </Button>
      )}
      <CompleteDialog todo={todo} onDone={(snapshot) => act.mutate({ kind: "complete", snapshot })} />
      <AlertDialog>
        <AlertDialogTrigger asChild>
          <Button variant="ghost" size="xs" aria-label={`Cancel TODO #${todo.number}`} disabled={act.isPending}>
            <CircleSlash aria-hidden="true" />
            Cancel
          </Button>
        </AlertDialogTrigger>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Cancel #{todo.number}?</AlertDialogTitle>
            <AlertDialogDescription>
              “{todo.title}” is closed without being done. It no longer blocks merges.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Keep it</AlertDialogCancel>
            <AlertDialogAction onClick={() => act.mutate({ kind: "cancel" })}>Cancel TODO</AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </span>
  );
}

export function TodosTab() {
  const { tenant, tree } = useTreeContext();
  const { account } = useAuth();
  const canWrite = useCanWrite();
  const id = useId();
  const [params, setParams] = useSearchParams();
  const statusParam = params.get("status");
  const status: StatusFilter = statusParam === ALL || TODO_STATUSES.some((s) => s === statusParam) ? (statusParam as StatusFilter) : "open";
  const branch = params.get("branch") ?? "";
  const mine = params.get("mine") === "1";

  const q: TodoQuery = {
    status: status === ALL ? undefined : status,
    branch: branch || undefined,
    assignee: mine ? account?.username : undefined,
  };
  const todos = useTodos(tenant, tree, q);

  const setParam = (key: string, value: string | null) =>
    setParams(
      (prev) => {
        const p = new URLSearchParams(prev);
        if (value === null || value === "") p.delete(key);
        else p.set(key, value);
        return p;
      },
      { replace: true },
    );

  if (todos.isError && isNotImplemented(todos.error)) {
    return (
      <Card>
        <CardHeader>
          <CardTitle>TODOs</CardTitle>
        </CardHeader>
        <CardContent>
          <FeatureUnavailable feature="TODOs" />
        </CardContent>
      </Card>
    );
  }

  const list = todos.data ?? [];

  return (
    <Card className="gap-4">
      <CardHeader className="flex-wrap">
        <CardTitle>TODOs</CardTitle>
        <CardDescription>Work requested from this tree by dependencies declared in other trees’ snapshots.</CardDescription>
        {canWrite && (
          <CardAction>
            <CreateTodoDialog />
          </CardAction>
        )}
      </CardHeader>

      <div role="toolbar" aria-label="TODO filters" className="mx-6 flex flex-wrap items-center gap-3">
        <div className="flex items-center gap-2">
          <Label htmlFor={`${id}-status`} className="text-xs text-muted-foreground">
            Status
          </Label>
          <Select value={status} onValueChange={(v) => setParam("status", v === "open" ? null : v)}>
            <SelectTrigger id={`${id}-status`} size="sm" className="w-36">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {TODO_STATUSES.map((s) => (
                <SelectItem key={s} value={s}>
                  {TODO_STATUS_LABEL[s]}
                </SelectItem>
              ))}
              <SelectItem value={ALL}>All</SelectItem>
            </SelectContent>
          </Select>
        </div>
        <BranchSelect
          tenant={tenant}
          tree={tree}
          value={branch}
          onChange={(b) => setParam("branch", b)}
          label="Target branch filter"
          allowAll
          className="w-52"
        />
        <div className="flex items-center gap-2">
          <Switch id={`${id}-mine`} checked={mine} onCheckedChange={(on) => setParam("mine", on ? "1" : null)} />
          <Label htmlFor={`${id}-mine`} className="text-sm">
            Assigned to me
          </Label>
        </div>
      </div>

      <CardContent>
        {todos.isPending ? (
          <LoadingRows label="Loading TODOs" rows={4} />
        ) : todos.isError ? (
          <QueryError error={todos.error} onRetry={() => void todos.refetch()} />
        ) : list.length === 0 ? (
          <EmptyState title={status === "open" && !branch && !mine ? "No open TODOs" : "No TODOs match these filters"} icon={<ListTodo aria-hidden="true" />}>
            Another tree requests work here with <code className="font-mono">wt snapshot --depend {tree} --todo TITLE</code>.
          </EmptyState>
        ) : (
          <Table aria-label="TODOs">
            <TableHeader className="sticky top-0 bg-card">
              <TableRow>
                <TableHead className="w-12">#</TableHead>
                <TableHead>TODO</TableHead>
                <TableHead>Priority</TableHead>
                <TableHead className="hidden md:table-cell">Target</TableHead>
                <TableHead className="hidden lg:table-cell">Requested by</TableHead>
                <TableHead>Assignee</TableHead>
                <TableHead>Status</TableHead>
                {canWrite && (
                  <TableHead>
                    <span className="sr-only">Actions</span>
                  </TableHead>
                )}
              </TableRow>
            </TableHeader>
            <TableBody>
              {[...list]
                .sort((a, b) => b.number - a.number)
                .map((t) => (
                  <TableRow key={t.id || t.number} data-todo={t.number}>
                    <TableCell className="font-mono text-muted-foreground tabular-nums">#{t.number}</TableCell>
                    <TableCell className="max-w-sm whitespace-normal">
                      <span className="flex flex-wrap items-center gap-1.5">
                        <span className={t.status === "cancelled" ? "font-medium line-through" : "font-medium"}>{t.title}</span>
                        {t.blocking && <BlockingBadge />}
                      </span>
                      {t.details && <span className="line-clamp-2 text-xs text-muted-foreground">{t.details}</span>}
                    </TableCell>
                    <TableCell>
                      <PriorityBadge priority={t.priority} />
                    </TableCell>
                    <TableCell className="hidden font-mono text-xs md:table-cell">{t.target_branch || "—"}</TableCell>
                    <TableCell className="hidden text-xs text-muted-foreground lg:table-cell">
                      <span className="text-foreground">{t.author}</span>
                      {t.source_branch && (
                        <>
                          {" "}
                          on <span className="font-mono">{t.source_branch}</span>
                        </>
                      )}
                      {t.source_snapshot_id && (
                        <span className="block font-mono" title={t.source_snapshot_id}>
                          {shortId(t.source_snapshot_id)} · <RelativeTime value={t.created_at} />
                        </span>
                      )}
                    </TableCell>
                    <TableCell>{t.assignee || <span className="text-muted-foreground">—</span>}</TableCell>
                    <TableCell>
                      <TodoStatusBadge status={t.status} />
                      {t.resolved_by && (
                        <span className="mt-0.5 block font-mono text-xs text-muted-foreground" title={t.resolved_by}>
                          by {shortId(t.resolved_by)}
                        </span>
                      )}
                    </TableCell>
                    {canWrite && (
                      <TableCell>
                        <TodoActions todo={t} />
                      </TableCell>
                    )}
                  </TableRow>
                ))}
            </TableBody>
          </Table>
        )}
      </CardContent>
    </Card>
  );
}
