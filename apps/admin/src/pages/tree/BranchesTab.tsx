import { useMutation, useQueryClient } from "@tanstack/react-query";
import { GitBranch, GitCompareArrows, History, Lock, Plus, Trash2 } from "lucide-react";
import { useId, useState, type FormEvent } from "react";
import { Link } from "react-router";
import { errorMessage, isApiError } from "@/api/client";
import { useApi } from "@/api/context";
import { keys, useBranches } from "@/api/queries";
import type { Branch } from "@/api/types";
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
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardAction, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { shortId } from "@/lib/format";
import { treePath } from "@/lib/paths";
import { useToast } from "@/toast/store";
import { useTreeContext } from "./context";
import { RevisionSelect } from "./history/RevisionSelect";
import { EmptyState, LoadingRows, QueryError, RelativeTime } from "./history/shared";

/** Client-side check mirroring common ref-name rules; the server has the final word. */
function branchNameError(name: string): string | null {
  if (!name) return "Enter a branch name.";
  if (/\s/.test(name)) return "Branch names cannot contain spaces.";
  if (name.startsWith("-") || name.startsWith("/") || name.endsWith("/") || name.endsWith(".")) return "Branch names cannot start with - or /, or end with / or .";
  if (name.includes("..") || name.includes("//")) return "Branch names cannot contain .. or //.";
  if (/[~^:?*[\\]/.test(name)) return "Branch names cannot contain ~ ^ : ? * [ or \\.";
  return null;
}

function useInvalidateBranches() {
  const { tenant, tree } = useTreeContext();
  const queryClient = useQueryClient();
  return () => {
    void queryClient.invalidateQueries({ queryKey: keys.branches(tenant, tree) });
    void queryClient.invalidateQueries({ queryKey: keys.tree(tenant, tree), exact: true });
  };
}

function CreateBranchDialog() {
  const { tenant, tree, summary } = useTreeContext();
  const api = useApi();
  const toast = useToast();
  const invalidate = useInvalidateBranches();
  const nameId = useId();
  const [open, setOpen] = useState(false);
  const [name, setName] = useState("");
  const [from, setFrom] = useState("");
  const [touched, setTouched] = useState(false);

  const create = useMutation({
    mutationFn: () => api.trees.createBranch(tenant, tree, { name: name.trim(), from: from || undefined }),
    meta: { inlineError: true },
    onSuccess: (b) => {
      invalidate();
      toast.success("Branch created", b.name);
      setOpen(false);
    },
  });

  const nameError = touched ? branchNameError(name.trim()) : null;
  const serverError =
    create.error && (isApiError(create.error) && create.error.status === 409 ? `A branch named “${name.trim()}” already exists.` : errorMessage(create.error));

  const submit = (e: FormEvent) => {
    e.preventDefault();
    setTouched(true);
    if (branchNameError(name.trim())) return;
    create.mutate();
  };

  return (
    <Dialog
      open={open}
      onOpenChange={(o) => {
        setOpen(o);
        if (o) {
          setName("");
          setFrom("");
          setTouched(false);
          create.reset();
        }
      }}
    >
      <DialogTrigger asChild>
        <Button size="sm">
          <Plus aria-hidden="true" />
          New branch
        </Button>
      </DialogTrigger>
      <DialogContent>
        <form onSubmit={submit} aria-label="Create branch" className="grid gap-4" noValidate>
          <DialogHeader>
            <DialogTitle>Create branch</DialogTitle>
            <DialogDescription>Branches start at a revision. Pushing to it then works like any other branch.</DialogDescription>
          </DialogHeader>
          <div className="grid gap-2">
            <Label htmlFor={nameId}>Branch name</Label>
            <Input
              id={nameId}
              value={name}
              autoFocus
              autoComplete="off"
              spellCheck={false}
              placeholder="feature/my-change"
              className="font-mono"
              aria-invalid={nameError ? true : undefined}
              aria-describedby={nameError ? `${nameId}-error` : undefined}
              onChange={(e) => setName(e.target.value)}
              onBlur={() => setTouched(true)}
            />
            {nameError && (
              <p id={`${nameId}-error`} className="text-xs font-medium">
                {nameError}
              </p>
            )}
          </div>
          <RevisionSelect
            tenant={tenant}
            tree={tree}
            branch={summary.tree.default_branch}
            label="Start from"
            emptyLabel={`Tip of ${summary.tree.default_branch} (default)`}
            value={from}
            onChange={setFrom}
          />
          {serverError && (
            <Alert>
              <AlertDescription className="text-foreground">{serverError}</AlertDescription>
            </Alert>
          )}
          <DialogFooter>
            <Button type="button" variant="outline" onClick={() => setOpen(false)}>
              Cancel
            </Button>
            <Button type="submit" disabled={create.isPending}>
              {create.isPending ? "Creating…" : "Create branch"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

function DeleteBranchButton({ branch }: { branch: Branch }) {
  const { tenant, tree } = useTreeContext();
  const api = useApi();
  const toast = useToast();
  const invalidate = useInvalidateBranches();
  const remove = useMutation({
    mutationFn: () => api.trees.deleteBranch(tenant, tree, branch.name),
    onSuccess: () => {
      invalidate();
      toast.success("Branch deleted", branch.name);
    },
  });

  return (
    <AlertDialog>
      <AlertDialogTrigger asChild>
        <Button variant="ghost" size="icon-sm" aria-label={`Delete branch ${branch.name}`} disabled={remove.isPending}>
          <Trash2 aria-hidden="true" />
        </Button>
      </AlertDialogTrigger>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>Delete branch {branch.name}?</AlertDialogTitle>
          <AlertDialogDescription>
            The branch pointer is removed. Snapshots stay reachable from other branches and tags.
            {branch.protected && " Protection rules may block this."}
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>Cancel</AlertDialogCancel>
          <AlertDialogAction onClick={() => remove.mutate()}>
            <Trash2 aria-hidden="true" />
            Delete branch
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}

export function BranchesTab() {
  const { tenant, tree, summary } = useTreeContext();
  const branches = useBranches(tenant, tree);
  const defaultBranch = summary.tree.default_branch;
  const compareHref = (name: string) => `${treePath(tenant, tree, "compare")}?${new URLSearchParams({ from: defaultBranch, to: name }).toString()}`;
  const historyHref = (name: string) => `${treePath(tenant, tree, "history")}?${new URLSearchParams({ branch: name }).toString()}`;

  return (
    <Card className="gap-4">
      <CardHeader>
        <CardTitle>Branches</CardTitle>
        <CardDescription>Default branch first, then alphabetical.</CardDescription>
        <CardAction>
          <CreateBranchDialog />
        </CardAction>
      </CardHeader>
      <CardContent>
        {branches.isPending ? (
          <LoadingRows label="Loading branches" />
        ) : branches.isError ? (
          <QueryError error={branches.error} onRetry={() => void branches.refetch()} />
        ) : branches.data.length === 0 ? (
          <EmptyState title="No branches yet" icon={<GitBranch aria-hidden="true" />}>
            Push a branch with <code className="font-mono">wt push</code>.
          </EmptyState>
        ) : (
          <Table aria-label="Branches">
            <TableHeader className="sticky top-0 bg-card">
              <TableRow>
                <TableHead>Branch</TableHead>
                <TableHead>Tip</TableHead>
                <TableHead>Updated</TableHead>
                <TableHead className="hidden md:table-cell">Created</TableHead>
                <TableHead>
                  <span className="sr-only">Actions</span>
                </TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {[...branches.data]
                .sort((a, b) => (a.name === defaultBranch ? -1 : b.name === defaultBranch ? 1 : a.name.localeCompare(b.name)))
                .map((b) => {
                  const isDefault = b.name === defaultBranch;
                  return (
                    <TableRow key={b.name}>
                      <TableCell>
                        <span className="flex flex-wrap items-center gap-1.5">
                          <GitBranch aria-hidden="true" className="size-3.5 text-muted-foreground" />
                          <span className="font-mono font-medium">{b.name}</span>
                          {isDefault && <Badge>default</Badge>}
                          {b.protected && (
                            <Badge variant="outline" title="Branch protection rules apply">
                              <Lock aria-hidden="true" />
                              protected
                            </Badge>
                          )}
                        </span>
                      </TableCell>
                      <TableCell>
                        {b.tip ? (
                          <Link
                            to={treePath(tenant, tree, `snapshots/${encodeURIComponent(b.tip)}`)}
                            className="font-mono hover:underline"
                            title={b.tip}
                          >
                            {shortId(b.tip)}
                          </Link>
                        ) : (
                          <span className="text-muted-foreground">empty</span>
                        )}
                      </TableCell>
                      <TableCell>
                        <RelativeTime value={b.updated_at} className="text-muted-foreground" />
                      </TableCell>
                      <TableCell className="hidden md:table-cell">
                        <RelativeTime value={b.created_at} className="text-muted-foreground" />
                      </TableCell>
                      <TableCell>
                        <span className="flex items-center justify-end gap-1">
                          <Button variant="ghost" size="icon-sm" asChild>
                            <Link to={historyHref(b.name)} aria-label={`History of ${b.name}`} title="History">
                              <History aria-hidden="true" />
                            </Link>
                          </Button>
                          {!isDefault && (
                            <Button variant="ghost" size="icon-sm" asChild>
                              <Link to={compareHref(b.name)} aria-label={`Compare ${b.name} with ${defaultBranch}`} title={`Compare with ${defaultBranch}`}>
                                <GitCompareArrows aria-hidden="true" />
                              </Link>
                            </Button>
                          )}
                          {!isDefault && <DeleteBranchButton branch={b} />}
                        </span>
                      </TableCell>
                    </TableRow>
                  );
                })}
            </TableBody>
          </Table>
        )}
      </CardContent>
    </Card>
  );
}
