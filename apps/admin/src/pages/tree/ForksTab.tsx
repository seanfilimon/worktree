/**
 * Forks of a tree and its upstream (docs/ENGINEERING_PLAN.md §16.2): the
 * upstream card of a fork (branch relations, sync, merge request upstream),
 * the list of forks and the fork dialog. ForkBadge is the fork marker of
 * the tree header. Endpoints and helpers live in ./forksApi.
 */
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ArrowUpRight, CircleAlert, GitFork, GitPullRequest, RefreshCw, TriangleAlert } from "lucide-react";
import { useId, useState, type FormEvent } from "react";
import { Link, useNavigate } from "react-router";
import { useApi } from "@/api/context";
import type { Visibility } from "@/api/types";
import { isUnavailable, retryV06 } from "@/api/v06";
import { useAuth } from "@/auth/context";
import { ChipsInput } from "@/components/files/ChipsInput";
import { EmptyState, FormError, QueryError, TableSkeleton } from "@/components/people/states";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { Textarea } from "@/components/ui/textarea";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { UnavailableNotice } from "@/components/v06/UnavailableNotice";
import { absoluteTime, relativeTime, shortId } from "@/lib/format";
import { mergeRequestPath, treePath } from "@/lib/paths";
import { useToast } from "@/toast/store";
import { BranchSelect } from "./BranchSelect";
import { useTreeContext } from "./context";
import {
  allowedVisibilities,
  createForkMergeRequest,
  forkKeys,
  forkNameError,
  RELATION_LABEL,
  syncSummary,
  useForksApi,
  useUpstream,
  type BranchRelation,
  type ForkTree,
  type Upstream,
} from "./forksApi";

// ---- small pieces ----

function When({ at, never = "never" }: { at?: string; never?: string }) {
  if (!at) return <span className="text-muted-foreground">{never}</span>;
  return (
    <time className="text-muted-foreground" dateTime={at} title={absoluteTime(at)}>
      {relativeTime(at)}
    </time>
  );
}

function RelationBadge({ status }: { status: BranchRelation }) {
  const label = RELATION_LABEL[status];
  switch (status) {
    case "up_to_date":
      return <Badge variant="secondary">{label}</Badge>;
    case "diverged":
      return (
        <Badge variant="outline" className="font-semibold">
          <TriangleAlert aria-hidden="true" />
          {label}
        </Badge>
      );
    case "behind":
      return (
        <Badge variant="outline" className="font-semibold">
          {label}
        </Badge>
      );
    default:
      return <Badge variant="outline">{label}</Badge>;
  }
}

/**
 * Fork marker for the tree header: "fork of tenant/tree" (a link), or a
 * note that the upstream is gone or hidden. Renders nothing for trees that
 * are not forks (no request is made then).
 */
export function ForkBadge({ tenant, tree, forkOf }: { tenant: string; tree: string; forkOf?: string }) {
  const upstream = useUpstream(tenant, tree, Boolean(forkOf));
  if (!forkOf) return null;
  const u = upstream.data;
  if (u?.fork && u.tenant && u.name) {
    return (
      <Badge variant="outline" asChild>
        <Link to={treePath(u.tenant, u.name)} aria-label={`Fork of ${u.tenant}/${u.name}`}>
          <GitFork aria-hidden="true" />
          fork of <span className="font-mono">{`${u.tenant}/${u.name}`}</span>
        </Link>
      </Badge>
    );
  }
  return (
    <Badge variant="outline" title={u?.upstream_missing ? "The upstream tree was deleted" : undefined}>
      <GitFork aria-hidden="true" />
      {u?.upstream_missing ? "fork (upstream deleted)" : "fork"}
    </Badge>
  );
}

// ---- fork dialog ----

function ForkDialog({ onClose }: { onClose: () => void }) {
  const { tenant, tree, summary } = useTreeContext();
  const api = useForksApi(tenant, tree);
  const { account, me } = useAuth();
  const navigate = useNavigate();
  const toast = useToast();
  const id = useId();
  const upstreamVisibility = summary.tree.visibility;
  // Destinations: the personal tenant plus tenants where the caller is above viewer.
  const personal = account?.username ?? "";
  const destinations = [
    ...(personal ? [personal] : []),
    ...(me.data?.memberships ?? []).filter((m) => m.role !== "viewer" && m.tenant.slug !== personal).map((m) => m.tenant.slug),
  ];
  const [dest, setDest] = useState(destinations[0] ?? "");
  const [name, setName] = useState("");
  const [visibility, setVisibility] = useState<Visibility>(upstreamVisibility);
  const [branches, setBranches] = useState<string[]>([]);
  const [description, setDescription] = useState("");
  const [touched, setTouched] = useState(false);
  const nameError = forkNameError(name);

  const create = useMutation({
    mutationFn: () =>
      api.create({
        tenant: dest,
        visibility,
        ...(name.trim() && { name: name.trim() }),
        ...(description.trim() && { description: description.trim() }),
        ...(branches.length > 0 && { branches }),
      }),
    meta: { inlineError: true },
    onSuccess: (t: ForkTree) => {
      toast.success("Tree forked", `${dest}/${t.name}`);
      onClose();
      void navigate(treePath(dest, t.name));
    },
  });

  const submit = (e: FormEvent) => {
    e.preventDefault();
    setTouched(true);
    if (nameError || !dest) return;
    create.mutate();
  };

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="max-h-[90dvh] overflow-y-auto sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>Fork this tree</DialogTitle>
          <DialogDescription>
            Copies the branches and tags of <span className="font-mono text-foreground">{`${tenant}/${tree}`}</span> into a new tree. Protections, licenses and
            settings are not copied; file contents are shared, not duplicated.
          </DialogDescription>
        </DialogHeader>
        <form id={`${id}-form`} onSubmit={submit} aria-label="Fork this tree" className="grid gap-4" noValidate>
          <div className="grid gap-2">
            <Label htmlFor={`${id}-tenant`}>Destination tenant</Label>
            <Select value={dest} onValueChange={setDest}>
              <SelectTrigger id={`${id}-tenant`} className="w-full" aria-label="Destination tenant">
                <SelectValue placeholder="Select a tenant" />
              </SelectTrigger>
              <SelectContent>
                {destinations.map((d) => (
                  <SelectItem key={d} value={d}>
                    {d === personal ? `${d} (personal)` : d}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <p className="text-xs text-muted-foreground">You need permission to create trees there. License rules may refuse forks out of this tenant.</p>
          </div>
          <div className="grid gap-2">
            <Label htmlFor={`${id}-name`}>Name</Label>
            <Input
              id={`${id}-name`}
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder={tree}
              autoComplete="off"
              spellCheck={false}
              className="font-mono"
              aria-invalid={touched && nameError ? true : undefined}
              aria-describedby={`${id}-name-hint`}
            />
            <p id={`${id}-name-hint`} className={touched && nameError ? "text-xs font-semibold" : "text-xs text-muted-foreground"}>
              {(touched && nameError) || "Default: the same name as this tree."}
            </p>
          </div>
          <div className="grid gap-2">
            <Label id={`${id}-vis`}>Visibility</Label>
            <ToggleGroup
              type="single"
              variant="outline"
              value={visibility}
              onValueChange={(v) => v && setVisibility(v as Visibility)}
              aria-labelledby={`${id}-vis`}
              className="justify-start"
            >
              {allowedVisibilities(upstreamVisibility).map((v) => (
                <ToggleGroupItem key={v} value={v} className="capitalize">
                  {v}
                </ToggleGroupItem>
              ))}
            </ToggleGroup>
            <p className="text-xs text-muted-foreground">A fork can never be more visible than its upstream ({upstreamVisibility}).</p>
          </div>
          <div className="grid gap-2">
            <Label htmlFor={`${id}-branches`}>Branches</Label>
            <ChipsInput id={`${id}-branches`} values={branches} onChange={setBranches} placeholder="main, release/1.0" describedBy={`${id}-branches-hint`} />
            <p id={`${id}-branches-hint`} className="text-xs text-muted-foreground">
              Empty copies every branch. Tags are always copied.
            </p>
          </div>
          <div className="grid gap-2">
            <Label htmlFor={`${id}-desc`}>Description</Label>
            <Textarea id={`${id}-desc`} value={description} onChange={(e) => setDescription(e.target.value)} rows={2} placeholder={summary.tree.description} />
          </div>
          <FormError error={create.error} />
        </form>
        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            Cancel
          </Button>
          <Button type="submit" form={`${id}-form`} disabled={create.isPending || !dest}>
            {create.isPending ? "Forking…" : "Fork"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

// ---- merge request upstream ----

function UpstreamMergeRequestDialog({ upstream, onClose }: { upstream: Upstream; onClose: () => void }) {
  const { tenant, tree, summary } = useTreeContext();
  const { client } = useApi();
  const navigate = useNavigate();
  const toast = useToast();
  const id = useId();
  const upTenant = upstream.tenant ?? "";
  const upTree = upstream.name ?? "";
  const [source, setSource] = useState(summary.tree.default_branch);
  const [target, setTarget] = useState(upstream.default_branch ?? "main");
  const [title, setTitle] = useState("");
  const [description, setDescription] = useState("");
  const [touched, setTouched] = useState(false);

  const open = useMutation({
    mutationFn: () =>
      createForkMergeRequest(client, upTenant, upTree, {
        title: title.trim(),
        description,
        source_branch: source,
        target_branch: target,
        source_tenant: tenant,
        source_tree: tree,
      }),
    meta: { inlineError: true },
    onSuccess: (mr) => {
      toast.success(`Opened !${mr.number} on ${upTenant}/${upTree}`, mr.title);
      onClose();
      void navigate(mergeRequestPath(upTenant, upTree, mr.number));
    },
  });

  const submit = (e: FormEvent) => {
    e.preventDefault();
    setTouched(true);
    if (!title.trim() || !source || !target) return;
    open.mutate();
  };

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="max-h-[90dvh] overflow-y-auto sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>Open a merge request upstream</DialogTitle>
          <DialogDescription>
            Propose a branch of <span className="font-mono text-foreground">{`${tenant}/${tree}`}</span> for{" "}
            <span className="font-mono text-foreground">{`${upTenant}/${upTree}`}</span>. Reviewers there merge it with their usual permissions.
          </DialogDescription>
        </DialogHeader>
        <form id={`${id}-form`} onSubmit={submit} aria-label="Open a merge request upstream" className="grid gap-4" noValidate>
          <div className="grid gap-4 sm:grid-cols-2">
            <div className="grid gap-2">
              <span className="text-sm font-medium">From (this fork)</span>
              <BranchSelect tenant={tenant} tree={tree} value={source} onChange={setSource} label="Source branch" className="w-full" />
            </div>
            <div className="grid gap-2">
              <span className="text-sm font-medium">Into (upstream)</span>
              <BranchSelect tenant={upTenant} tree={upTree} value={target} onChange={setTarget} label="Target branch" className="w-full" />
            </div>
          </div>
          <div className="grid gap-2">
            <Label htmlFor={`${id}-title`}>Title</Label>
            <Input
              id={`${id}-title`}
              value={title}
              onChange={(e) => setTitle(e.target.value)}
              aria-invalid={touched && !title.trim() ? true : undefined}
              aria-describedby={`${id}-title-hint`}
            />
            {touched && !title.trim() && (
              <p id={`${id}-title-hint`} className="text-xs font-semibold">
                Required.
              </p>
            )}
          </div>
          <div className="grid gap-2">
            <Label htmlFor={`${id}-desc`}>Description</Label>
            <Textarea id={`${id}-desc`} value={description} onChange={(e) => setDescription(e.target.value)} rows={3} />
          </div>
          <FormError error={open.error} />
        </form>
        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            Cancel
          </Button>
          <Button type="submit" form={`${id}-form`} disabled={open.isPending}>
            {open.isPending ? "Opening…" : "Open merge request"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

// ---- upstream card ----

function UpstreamCard({ forkOf }: { forkOf: string }) {
  const { tenant, tree } = useTreeContext();
  const api = useForksApi(tenant, tree);
  const queryClient = useQueryClient();
  const toast = useToast();
  const [proposing, setProposing] = useState(false);
  const upstream = useUpstream(tenant, tree);
  const u = upstream.data;

  const sync = useMutation({
    mutationFn: () => api.sync(),
    onSuccess: async (r) => {
      if (r.diverged.length > 0) toast.error(new Error(syncSummary(r)), "Some branches diverged");
      else toast.success("Fork synced", syncSummary(r));
      await queryClient.invalidateQueries({ queryKey: ["tree", tenant, tree] });
    },
  });

  return (
    <Card className="gap-0 overflow-hidden py-0">
      <div className="flex flex-wrap items-center justify-between gap-2 border-b p-3">
        <div className="min-w-0">
          <h3 className="flex items-center gap-2 text-sm font-semibold">
            <GitFork aria-hidden="true" className="size-4" />
            Upstream
          </h3>
          {u?.fork && u.tenant && u.name ? (
            <p className="text-sm text-muted-foreground">
              Forked from{" "}
              <Link to={treePath(u.tenant, u.name)} className="font-mono text-foreground underline-offset-4 hover:underline">
                {`${u.tenant}/${u.name}`}
              </Link>
              {u.forked_by && <> by {u.forked_by}</>}
              {u.forked_at && (
                <>
                  {" "}
                  <When at={u.forked_at} />
                </>
              )}
              . Last sync: <When at={u.last_sync_at} />
            </p>
          ) : (
            <p className="text-sm text-muted-foreground">
              Forked from tree <span className="font-mono">{shortId(forkOf)}</span>.
            </p>
          )}
        </div>
        {u?.fork && u.tenant && u.name && (
          <span className="flex flex-wrap gap-2">
            <Button size="sm" variant="outline" onClick={() => sync.mutate()} disabled={sync.isPending}>
              <RefreshCw aria-hidden="true" className={sync.isPending ? "animate-spin" : undefined} />
              {sync.isPending ? "Syncing…" : "Sync with upstream"}
            </Button>
            <Button size="sm" onClick={() => setProposing(true)}>
              <GitPullRequest aria-hidden="true" />
              Open merge request upstream
            </Button>
          </span>
        )}
      </div>
      {upstream.isPending ? (
        <TableSkeleton label="Loading upstream" rows={2} />
      ) : upstream.isError ? (
        isUnavailable(upstream.error) ? (
          <div className="p-4">
            <UnavailableNotice feature="Forks" />
          </div>
        ) : (
          <QueryError error={upstream.error} onRetry={() => void upstream.refetch()} title="Could not load the upstream" />
        )
      ) : u?.upstream_missing ? (
        <div className="p-4">
          <Alert>
            <CircleAlert aria-hidden="true" />
            <AlertTitle>The upstream was deleted</AlertTitle>
            <AlertDescription>This fork keeps its full history and works as an independent tree. It can no longer be synced.</AlertDescription>
          </Alert>
        </div>
      ) : u?.upstream_hidden ? (
        <p className="p-4 text-sm text-muted-foreground">You cannot read the upstream tree, so branch relations are not shown.</p>
      ) : (
        <Table aria-label="Branches compared with the upstream">
          <TableHeader>
            <TableRow className="hover:bg-transparent">
              <TableHead>Branch</TableHead>
              <TableHead>Relation</TableHead>
              <TableHead className="hidden sm:table-cell">Fork tip</TableHead>
              <TableHead className="hidden sm:table-cell">Upstream tip</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {(u?.branches ?? []).map((b) => (
              <TableRow key={b.name}>
                <TableCell className="font-mono text-sm">{b.name}</TableCell>
                <TableCell>
                  <RelationBadge status={b.status} />
                </TableCell>
                <TableCell className="hidden font-mono text-xs text-muted-foreground sm:table-cell">{b.tip ? shortId(b.tip) : "—"}</TableCell>
                <TableCell className="hidden font-mono text-xs text-muted-foreground sm:table-cell">{b.upstream_tip ? shortId(b.upstream_tip) : "—"}</TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      )}
      {proposing && u && <UpstreamMergeRequestDialog upstream={u} onClose={() => setProposing(false)} />}
    </Card>
  );
}

// ---- tab ----

/** Forks of this tree and its upstream (§16.2). */
export function ForksTab() {
  const { tenant, tree, summary } = useTreeContext();
  const { account } = useAuth();
  const api = useForksApi(tenant, tree);
  const [forking, setForking] = useState(false);
  const forkOf = (summary.tree as ForkTree).fork_of;
  const forks = useQuery({ queryKey: forkKeys.forks(tenant, tree), queryFn: api.list, retry: retryV06 });
  const unavailable = forks.isError && isUnavailable(forks.error);

  return (
    <div className="grid gap-4">
      <div>
        <h2 className="flex items-center gap-2 text-lg font-semibold">
          <GitFork aria-hidden="true" className="size-5" />
          Forks
        </h2>
        <p className="text-sm text-muted-foreground">
          A fork is a copy of this tree in another tenant that shares its history. Forks sync from their upstream and propose changes back with merge requests.
        </p>
      </div>
      {forkOf && <UpstreamCard forkOf={forkOf} />}
      <Card className="gap-0 overflow-hidden py-0">
        <div className="flex flex-wrap items-center justify-between gap-2 border-b p-3">
          <p className="text-sm text-muted-foreground">Forks of {`${tenant}/${tree}`} that you can see.</p>
          {account && !unavailable && (
            <Button size="sm" onClick={() => setForking(true)}>
              <GitFork aria-hidden="true" />
              Fork
            </Button>
          )}
        </div>
        {forks.isPending ? (
          <TableSkeleton label="Loading forks" rows={2} />
        ) : unavailable ? (
          <div className="p-4">
            <UnavailableNotice feature="Forks" />
          </div>
        ) : forks.isError ? (
          <QueryError error={forks.error} onRetry={() => void forks.refetch()} title="Could not load forks" />
        ) : forks.data.length === 0 ? (
          <EmptyState icon={GitFork} title="No forks">
            Fork this tree to work on it in your own tenant.
          </EmptyState>
        ) : (
          <Table aria-label="Forks">
            <TableHeader className="sticky top-0 z-10 bg-background">
              <TableRow className="hover:bg-transparent">
                <TableHead>Fork</TableHead>
                <TableHead className="hidden sm:table-cell">Visibility</TableHead>
                <TableHead className="hidden md:table-cell">Branches</TableHead>
                <TableHead className="hidden sm:table-cell">Created</TableHead>
                <TableHead className="hidden md:table-cell">Last sync</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {forks.data.map((f) => (
                <TableRow key={f.tree_id}>
                  <TableCell className="max-w-80">
                    <Link to={treePath(f.tenant, f.name, "forks")} className="inline-flex items-center gap-1 font-mono text-sm font-medium underline-offset-4 hover:underline">
                      {`${f.tenant}/${f.name}`}
                      <ArrowUpRight aria-hidden="true" className="size-3.5" />
                    </Link>
                    <p className="text-xs text-muted-foreground">by {f.created_by}</p>
                  </TableCell>
                  <TableCell className="hidden capitalize sm:table-cell">
                    <Badge variant="outline">{f.visibility}</Badge>
                  </TableCell>
                  <TableCell className="hidden text-sm tabular-nums md:table-cell">{f.branches.length}</TableCell>
                  <TableCell className="hidden text-xs sm:table-cell">
                    <When at={f.created_at} />
                  </TableCell>
                  <TableCell className="hidden text-xs md:table-cell">
                    <When at={f.last_sync_at} />
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        )}
      </Card>
      {forking && <ForkDialog onClose={() => setForking(false)} />}
    </div>
  );
}
