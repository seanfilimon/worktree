/**
 * Git mirrors of a tree (docs/ENGINEERING_PLAN.md §15.6): list with status
 * and last error, add / edit, pause / resume, sync now and delete. The
 * endpoints and helpers live in ./mirrorsApi.
 */
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { CircleAlert, GitFork, KeyRound, Pencil, Plus, RefreshCw, Tag, Trash2, TriangleAlert } from "lucide-react";
import { useId, useMemo, useState, type FormEvent } from "react";
import { useApi } from "@/api/context";
import { isUnavailable, retryV06 } from "@/api/v06";
import { ChipsInput } from "@/components/files/ChipsInput";
import { EmptyState, FormError, QueryError, TableSkeleton } from "@/components/people/states";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { UnavailableNotice } from "@/components/v06/UnavailableNotice";
import { absoluteTime, relativeTime } from "@/lib/format";
import { ConfirmAction } from "@/pages/iam/shared";
import { useToast } from "@/toast/store";
import { useTreeContext } from "./context";
import { createMirrorsApi, mirrorFormErrors, mirrorsKey, syncSummary, type Mirror, type UpdateMirrorRequest } from "./mirrorsApi";

function useMirrorsApi() {
  const { client } = useApi();
  const { tenant, tree } = useTreeContext();
  return useMemo(() => createMirrorsApi(client, tenant, tree), [client, tenant, tree]);
}

// ---- pieces ----

function StatusBadge({ m }: { m: Mirror }) {
  if (!m.active) return <Badge variant="outline">Paused</Badge>;
  switch (m.last_status) {
    case "ok":
      return <Badge variant="secondary">Synced</Badge>;
    case "running":
      return (
        <Badge variant="outline">
          <RefreshCw aria-hidden="true" className="animate-spin" />
          Syncing
        </Badge>
      );
    case "error":
      return (
        <Badge variant="outline" className="font-semibold">
          <TriangleAlert aria-hidden="true" />
          Failing
        </Badge>
      );
    case "disabled":
      return (
        <Badge variant="outline" className="font-semibold">
          <CircleAlert aria-hidden="true" />
          Disabled
        </Badge>
      );
    default:
      return <Badge variant="outline">Pending</Badge>;
  }
}

function LastSync({ m }: { m: Mirror }) {
  if (!m.last_sync_at) return <span className="text-muted-foreground">never</span>;
  return (
    <time className="text-muted-foreground" dateTime={m.last_sync_at} title={absoluteTime(m.last_sync_at)}>
      {relativeTime(m.last_sync_at)}
    </time>
  );
}

// ---- dialog ----

interface MirrorDialogProps {
  /** null = create. */
  mirror: Mirror | null;
  onClose: () => void;
}

function MirrorDialog({ mirror, onClose }: MirrorDialogProps) {
  const api = useMirrorsApi();
  const { tenant, tree } = useTreeContext();
  const queryClient = useQueryClient();
  const toast = useToast();
  const id = useId();
  const editing = mirror !== null;
  const [url, setUrl] = useState(mirror?.url ?? "");
  const [branches, setBranches] = useState<string[]>(mirror?.branches ?? []);
  const [tags, setTags] = useState(mirror?.tags ?? true);
  const [active, setActive] = useState(mirror?.active ?? true);
  const [credential, setCredential] = useState("");
  const [clearCredential, setClearCredential] = useState(false);
  const [touched, setTouched] = useState(false);
  const errors = mirrorFormErrors(url, branches, credential);

  const save = useMutation({
    mutationFn: async () => {
      if (mirror) {
        const body: UpdateMirrorRequest = { branches, tags, active };
        if (url.trim() !== mirror.url) body.url = url.trim();
        if (credential.trim()) body.credential = credential.trim();
        else if (clearCredential) body.credential = "";
        return api.update(mirror.id, body);
      }
      return api.create({ url: url.trim(), branches, tags, active, ...(credential.trim() && { credential: credential.trim() }) });
    },
    meta: { inlineError: true },
    onSuccess: async (m) => {
      toast.success(editing ? "Mirror updated" : "Mirror added", m.active ? `${m.url} syncs in a few seconds` : m.url);
      await queryClient.invalidateQueries({ queryKey: mirrorsKey(tenant, tree) });
      onClose();
    },
  });

  const submit = (e: FormEvent) => {
    e.preventDefault();
    setTouched(true);
    if (errors.url || errors.branches || errors.credential) return;
    save.mutate();
  };

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="max-h-[90dvh] overflow-y-auto sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>{editing ? "Edit mirror" : "Add mirror"}</DialogTitle>
          <DialogDescription>
            After every push to <span className="font-mono text-foreground">{`${tenant}/${tree}`}</span> the server exports the new snapshots as Git commits and pushes
            them to the remote with <span className="font-mono">--force-with-lease</span>.
          </DialogDescription>
        </DialogHeader>
        <form id={`${id}-form`} onSubmit={submit} aria-label={editing ? "Edit mirror" : "Add mirror"} className="grid gap-4" noValidate>
          <div className="grid gap-2">
            <Label htmlFor={`${id}-url`}>Remote URL</Label>
            <Input
              id={`${id}-url`}
              value={url}
              onChange={(e) => setUrl(e.target.value)}
              placeholder="https://github.com/acme/web.git"
              autoComplete="off"
              spellCheck={false}
              className="font-mono"
              aria-invalid={touched && errors.url ? true : undefined}
              aria-describedby={`${id}-url-hint`}
            />
            <p id={`${id}-url-hint`} className={touched && errors.url ? "text-xs font-semibold" : "text-xs text-muted-foreground"}>
              {(touched && errors.url) || "https://, ssh:// or user@host:path. Private hosts and local paths need --mirror-allow-local on the server."}
            </p>
          </div>
          <div className="grid gap-2">
            <Label htmlFor={`${id}-branches`}>Branches</Label>
            <ChipsInput id={`${id}-branches`} values={branches} onChange={setBranches} placeholder="main, release/*" describedBy={`${id}-branches-hint`} />
            <p id={`${id}-branches-hint`} className={touched && errors.branches ? "text-xs font-semibold" : "text-xs text-muted-foreground"}>
              {(touched && errors.branches) || "Glob patterns (* within a segment, ** across segments). Empty mirrors every branch."}
            </p>
          </div>
          <div className="grid gap-2">
            <Label htmlFor={`${id}-credential`}>{editing ? "New credential" : "Credential"}</Label>
            <Input
              id={`${id}-credential`}
              type="password"
              value={credential}
              onChange={(e) => setCredential(e.target.value)}
              autoComplete="new-password"
              spellCheck={false}
              className="font-mono"
              aria-invalid={touched && errors.credential ? true : undefined}
              aria-describedby={`${id}-credential-hint`}
            />
            <p id={`${id}-credential-hint`} className={touched && errors.credential ? "text-xs font-semibold" : "text-xs text-muted-foreground"}>
              {(touched && errors.credential) ||
                (editing && mirror?.has_credential
                  ? "Leave empty to keep the current credential."
                  : "Optional, for https remotes: a token or user:password. It is stored on the server and never shown again.")}
            </p>
            {editing && mirror?.has_credential && (
              <div className="flex items-center gap-2">
                <Checkbox id={`${id}-clear`} checked={clearCredential} onCheckedChange={(v) => setClearCredential(v === true)} />
                <Label htmlFor={`${id}-clear`} className="text-xs font-normal">
                  Remove the stored credential
                </Label>
              </div>
            )}
          </div>
          <div className="flex items-start justify-between gap-4 rounded-md border p-3">
            <div className="grid gap-1">
              <Label htmlFor={`${id}-tags`}>Mirror tags</Label>
              <p id={`${id}-tags-desc`} className="text-xs text-muted-foreground">
                Annotated tags keep their tagger and message.
              </p>
            </div>
            <Switch id={`${id}-tags`} checked={tags} onCheckedChange={setTags} aria-describedby={`${id}-tags-desc`} />
          </div>
          <div className="flex items-start justify-between gap-4 rounded-md border p-3">
            <div className="grid gap-1">
              <Label htmlFor={`${id}-active`}>Active</Label>
              <p id={`${id}-active-desc`} className="text-xs text-muted-foreground">
                Paused mirrors are not synced.
              </p>
            </div>
            <Switch id={`${id}-active`} checked={active} onCheckedChange={setActive} aria-describedby={`${id}-active-desc`} />
          </div>
          <FormError error={save.error} />
        </form>
        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            Cancel
          </Button>
          <Button type="submit" form={`${id}-form`} disabled={save.isPending}>
            {save.isPending ? "Saving…" : editing ? "Save mirror" : "Add mirror"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

// ---- tab ----

/** Git mirrors of this tree. */
export function MirrorsTab() {
  const { tenant, tree, canManage } = useTreeContext();
  const api = useMirrorsApi();
  const queryClient = useQueryClient();
  const toast = useToast();
  const [editing, setEditing] = useState<Mirror | "new" | null>(null);
  const [deleting, setDeleting] = useState<Mirror | null>(null);
  const key = mirrorsKey(tenant, tree);
  const mirrors = useQuery({
    queryKey: key,
    queryFn: api.list,
    retry: retryV06,
    refetchInterval: (q) => (q.state.data?.mirrors.some((m) => m.last_status === "running") ? 2000 : false),
  });
  const unavailable = mirrors.isError && isUnavailable(mirrors.error);
  const git = mirrors.data?.git;
  const invalidate = () => queryClient.invalidateQueries({ queryKey: key, exact: true });

  const sync = useMutation({
    mutationFn: (m: Mirror) => api.sync(m.id),
    onSuccess: async (s) => {
      if (s.result?.error) toast.error(new Error(s.result.error), "Mirror sync failed");
      else toast.success("Mirror synced", syncSummary(s.result));
      await invalidate();
    },
    onError: () => void invalidate(),
  });
  const toggle = useMutation({
    mutationFn: (m: Mirror) => api.update(m.id, { active: !m.active }),
    onSuccess: async (m) => {
      toast.success(m.active ? "Mirror resumed" : "Mirror paused", m.url);
      await invalidate();
    },
  });
  const remove = useMutation({
    mutationFn: (m: Mirror) => api.remove(m.id),
    meta: { inlineError: true },
    onSuccess: async (_r, m) => {
      toast.success("Mirror deleted", m.url);
      await invalidate();
    },
  });

  return (
    <div className="grid gap-4">
      <div>
        <h2 className="flex items-center gap-2 text-lg font-semibold">
          <GitFork aria-hidden="true" className="size-5" />
          Git mirrors
        </h2>
        <p className="text-sm text-muted-foreground">
          Push this tree's branches and tags to Git remotes. Commits are deterministic, files whose license forbids redistribution are left out and a{" "}
          <span className="font-mono">LICENSES/NOTICE</span> is added.
        </p>
      </div>
      {git && !git.available && (
        <Alert>
          <CircleAlert aria-hidden="true" />
          <AlertTitle>Mirrors are disabled on this server</AlertTitle>
          <AlertDescription>{git.error || "The git binary is missing."}</AlertDescription>
        </Alert>
      )}
      <Card className="gap-0 overflow-hidden py-0">
        <div className="flex flex-wrap items-center justify-between gap-2 border-b p-3">
          <p className="text-sm text-muted-foreground">
            {git?.available && git.version ? `Synced with git ${git.version} after every push (debounced 5 s).` : "Synced after every push."}
          </p>
          {canManage && !unavailable && (
            <Button size="sm" onClick={() => setEditing("new")}>
              <Plus aria-hidden="true" />
              Add mirror
            </Button>
          )}
        </div>
        {mirrors.isPending ? (
          <TableSkeleton label="Loading mirrors" rows={2} />
        ) : unavailable ? (
          <div className="p-4">
            <UnavailableNotice feature="Git mirrors" />
          </div>
        ) : mirrors.isError ? (
          <QueryError error={mirrors.error} onRetry={() => void mirrors.refetch()} title="Could not load mirrors" />
        ) : mirrors.data.mirrors.length === 0 ? (
          <EmptyState icon={GitFork} title="No mirrors">
            {canManage ? "Add one to keep a copy of this tree on GitHub, GitLab or any Git server." : "Tree administrators can mirror this tree to Git remotes."}
          </EmptyState>
        ) : (
          <Table aria-label="Mirrors">
            <TableHeader className="sticky top-0 z-10 bg-background">
              <TableRow className="hover:bg-transparent">
                <TableHead>Remote</TableHead>
                <TableHead className="hidden md:table-cell">Branches</TableHead>
                <TableHead>Status</TableHead>
                <TableHead className="hidden sm:table-cell">Last sync</TableHead>
                <TableHead>
                  <span className="sr-only">Actions</span>
                </TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {mirrors.data.mirrors.map((m) => (
                <TableRow key={m.id} className={m.active ? undefined : "text-muted-foreground"}>
                  <TableCell className="max-w-80">
                    <p className="truncate font-mono text-sm font-medium" title={m.url}>
                      {m.url}
                    </p>
                    <p className="flex flex-wrap items-center gap-1.5 text-xs text-muted-foreground">
                      {m.tags && (
                        <span className="inline-flex items-center gap-1">
                          <Tag aria-hidden="true" className="size-3" />
                          tags
                        </span>
                      )}
                      {m.has_credential && (
                        <span className="inline-flex items-center gap-1" title="A credential is stored">
                          <KeyRound aria-hidden="true" className="size-3" />
                          credential
                        </span>
                      )}
                      <span>by {m.created_by}</span>
                    </p>
                    {m.last_error && (
                      <p className="mt-1 line-clamp-2 text-xs font-medium break-all whitespace-pre-wrap" title={m.last_error} aria-label={`Last error: ${m.last_error}`}>
                        {m.last_error}
                      </p>
                    )}
                  </TableCell>
                  <TableCell className="hidden md:table-cell">
                    {m.branches.length === 0 ? (
                      <span className="text-sm text-muted-foreground">all branches</span>
                    ) : (
                      <span className="flex flex-wrap gap-1">
                        {m.branches.map((b) => (
                          <Badge key={b} variant="outline" className="font-mono">
                            {b}
                          </Badge>
                        ))}
                      </span>
                    )}
                  </TableCell>
                  <TableCell>
                    <StatusBadge m={m} />
                  </TableCell>
                  <TableCell className="hidden text-xs sm:table-cell">
                    <LastSync m={m} />
                  </TableCell>
                  <TableCell className="w-px text-right whitespace-nowrap">
                    {canManage && (
                      <span className="inline-flex items-center gap-1">
                        <Switch
                          checked={m.active}
                          disabled={toggle.isPending && toggle.variables.id === m.id}
                          onCheckedChange={() => toggle.mutate(m)}
                          aria-label={`Active: ${m.url}`}
                        />
                        <Button
                          variant="ghost"
                          size="icon-sm"
                          aria-label={`Sync ${m.url} now`}
                          title="Sync now"
                          disabled={!m.active || git?.available === false || (sync.isPending && sync.variables.id === m.id)}
                          onClick={() => sync.mutate(m)}
                        >
                          <RefreshCw aria-hidden="true" className={sync.isPending && sync.variables.id === m.id ? "animate-spin" : undefined} />
                        </Button>
                        <Button variant="ghost" size="icon-sm" aria-label={`Edit ${m.url}`} onClick={() => setEditing(m)}>
                          <Pencil aria-hidden="true" />
                        </Button>
                        <Button variant="ghost" size="icon-sm" aria-label={`Delete ${m.url}`} onClick={() => setDeleting(m)}>
                          <Trash2 aria-hidden="true" />
                        </Button>
                      </span>
                    )}
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        )}
      </Card>
      {canManage && editing !== null && <MirrorDialog key={editing === "new" ? "new" : editing.id} mirror={editing === "new" ? null : editing} onClose={() => setEditing(null)} />}
      <ConfirmAction
        open={deleting !== null}
        onOpenChange={(o) => !o && setDeleting(null)}
        title="Delete this mirror?"
        description={
          <>
            <span className="font-mono break-all">{deleting?.url}</span> stops receiving pushes. The remote repository is left as it is.
          </>
        }
        confirmLabel="Delete mirror"
        onConfirm={() => (deleting ? remove.mutateAsync(deleting) : Promise.resolve())}
      />
    </div>
  );
}
