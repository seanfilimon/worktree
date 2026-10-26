import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Plus, Tag as TagIcon, Trash2 } from "lucide-react";
import { useId, useState, type FormEvent } from "react";
import { Link } from "react-router";
import { errorMessage, isApiError } from "@/api/client";
import { useApi } from "@/api/context";
import { keys, useTags } from "@/api/queries";
import type { Tag } from "@/api/types";
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
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { Textarea } from "@/components/ui/textarea";
import { firstLine, shortId } from "@/lib/format";
import { treePath } from "@/lib/paths";
import { useToast } from "@/toast/store";
import { useTreeContext } from "./context";
import { RevisionSelect } from "./history/RevisionSelect";
import { EmptyState, LoadingRows, QueryError, RelativeTime } from "./history/shared";
import type { SignedTag } from "./trust";
import { VerifiedBadge } from "./TrustBadges";

function tagNameError(name: string): string | null {
  if (!name) return "Enter a tag name.";
  if (/\s/.test(name)) return "Tag names cannot contain spaces.";
  if (name.includes("/")) return "Tag names cannot contain /.";
  if (name.startsWith("-") || name.includes("..")) return "Tag names cannot start with - or contain ..";
  return null;
}

function useInvalidateTags() {
  const { tenant, tree } = useTreeContext();
  const queryClient = useQueryClient();
  return () => void queryClient.invalidateQueries({ queryKey: keys.tags(tenant, tree) });
}

function CreateTagDialog() {
  const { tenant, tree, summary } = useTreeContext();
  const api = useApi();
  const toast = useToast();
  const invalidate = useInvalidateTags();
  const id = useId();
  const [open, setOpen] = useState(false);
  const [name, setName] = useState("");
  const [target, setTarget] = useState(summary.tree.default_branch);
  const [message, setMessage] = useState("");
  const [touched, setTouched] = useState(false);

  const create = useMutation({
    mutationFn: () => api.trees.createTag(tenant, tree, { name: name.trim(), target, message: message.trim() || undefined }),
    meta: { inlineError: true },
    onSuccess: (t) => {
      invalidate();
      toast.success("Tag created", `${t.name} → ${shortId(t.target)}`);
      setOpen(false);
    },
  });

  const nameError = touched ? tagNameError(name.trim()) : null;
  const serverError =
    create.error && (isApiError(create.error) && create.error.status === 409 ? `A tag named “${name.trim()}” already exists.` : errorMessage(create.error));

  const submit = (e: FormEvent) => {
    e.preventDefault();
    setTouched(true);
    if (tagNameError(name.trim()) || !target) return;
    create.mutate();
  };

  return (
    <Dialog
      open={open}
      onOpenChange={(o) => {
        setOpen(o);
        if (o) {
          setName("");
          setTarget(summary.tree.default_branch);
          setMessage("");
          setTouched(false);
          create.reset();
        }
      }}
    >
      <DialogTrigger asChild>
        <Button size="sm">
          <Plus aria-hidden="true" />
          New tag
        </Button>
      </DialogTrigger>
      <DialogContent>
        <form onSubmit={submit} aria-label="Create tag" className="grid gap-4" noValidate>
          <DialogHeader>
            <DialogTitle>Create tag</DialogTitle>
            <DialogDescription>Tags pin a name such as a release version to one snapshot.</DialogDescription>
          </DialogHeader>
          <div className="grid gap-2">
            <Label htmlFor={`${id}-name`}>Tag name</Label>
            <Input
              id={`${id}-name`}
              value={name}
              autoFocus
              autoComplete="off"
              spellCheck={false}
              placeholder="v1.2.0"
              className="font-mono"
              aria-invalid={nameError ? true : undefined}
              aria-describedby={nameError ? `${id}-error` : undefined}
              onChange={(e) => setName(e.target.value)}
              onBlur={() => setTouched(true)}
            />
            {nameError && (
              <p id={`${id}-error`} className="text-xs font-medium">
                {nameError}
              </p>
            )}
          </div>
          <RevisionSelect
            tenant={tenant}
            tree={tree}
            branch={summary.tree.default_branch}
            label="Target revision"
            value={target}
            onChange={setTarget}
          />
          <div className="grid gap-2">
            <Label htmlFor={`${id}-message`}>
              Message <span className="font-normal text-muted-foreground">(optional)</span>
            </Label>
            <Textarea id={`${id}-message`} value={message} rows={3} onChange={(e) => setMessage(e.target.value)} />
          </div>
          {serverError && (
            <Alert>
              <AlertDescription className="text-foreground">{serverError}</AlertDescription>
            </Alert>
          )}
          <DialogFooter>
            <Button type="button" variant="outline" onClick={() => setOpen(false)}>
              Cancel
            </Button>
            <Button type="submit" disabled={create.isPending || !target}>
              {create.isPending ? "Creating…" : "Create tag"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

function DeleteTagButton({ tag }: { tag: Tag }) {
  const { tenant, tree } = useTreeContext();
  const api = useApi();
  const toast = useToast();
  const invalidate = useInvalidateTags();
  const remove = useMutation({
    mutationFn: () => api.trees.deleteTag(tenant, tree, tag.name),
    onSuccess: () => {
      invalidate();
      toast.success("Tag deleted", tag.name);
    },
  });

  return (
    <AlertDialog>
      <AlertDialogTrigger asChild>
        <Button variant="ghost" size="icon-sm" aria-label={`Delete tag ${tag.name}`} disabled={remove.isPending}>
          <Trash2 aria-hidden="true" />
        </Button>
      </AlertDialogTrigger>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>Delete tag {tag.name}?</AlertDialogTitle>
          <AlertDialogDescription>The tag is removed; snapshot {shortId(tag.target)} is not affected.</AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>Cancel</AlertDialogCancel>
          <AlertDialogAction onClick={() => remove.mutate()}>
            <Trash2 aria-hidden="true" />
            Delete tag
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}

export function TagsTab() {
  const { tenant, tree, canManage } = useTreeContext();
  const tags = useTags(tenant, tree);

  return (
    <Card className="gap-4">
      <CardHeader>
        <CardTitle>Tags</CardTitle>
        <CardDescription>Newest first.</CardDescription>
        <CardAction>
          <CreateTagDialog />
        </CardAction>
      </CardHeader>
      <CardContent>
        {tags.isPending ? (
          <LoadingRows label="Loading tags" />
        ) : tags.isError ? (
          <QueryError error={tags.error} onRetry={() => void tags.refetch()} />
        ) : tags.data.length === 0 ? (
          <EmptyState title="No tags" icon={<TagIcon aria-hidden="true" />}>
            Create one here or with <code className="font-mono">wt tag NAME --sign</code> and <code className="font-mono">wt tag push NAME</code>.
          </EmptyState>
        ) : (
          <Table aria-label="Tags">
            <TableHeader className="sticky top-0 bg-card">
              <TableRow>
                <TableHead>Tag</TableHead>
                <TableHead>Target</TableHead>
                <TableHead className="hidden md:table-cell">Message</TableHead>
                <TableHead>Tagger</TableHead>
                <TableHead>Created</TableHead>
                {canManage && (
                  <TableHead>
                    <span className="sr-only">Actions</span>
                  </TableHead>
                )}
              </TableRow>
            </TableHeader>
            <TableBody>
              {[...tags.data]
                .sort((a, b) => b.created_at.localeCompare(a.created_at))
                .map((t: SignedTag) => (
                  <TableRow key={t.name}>
                    <TableCell>
                      <span className="flex flex-wrap items-center gap-1.5 font-mono font-medium">
                        <TagIcon aria-hidden="true" className="size-3.5 text-muted-foreground" />
                        {t.name}
                        {/* v0.7 trust (§15.8): signature verified by the server. */}
                        <VerifiedBadge verified={t.verified} signer={t.signer} keyId={t.signing_key_id} className="font-sans" />
                      </span>
                    </TableCell>
                    <TableCell>
                      <Link
                        to={treePath(tenant, tree, `snapshots/${encodeURIComponent(t.target)}`)}
                        className="font-mono hover:underline"
                        title={t.target}
                      >
                        {shortId(t.target)}
                      </Link>
                    </TableCell>
                    <TableCell className="hidden max-w-xs truncate md:table-cell">
                      {firstLine(t.message) || <span className="text-muted-foreground">—</span>}
                    </TableCell>
                    <TableCell>
                      {t.tagger}
                      {t.verified && t.signed_tagger && t.signed_tagger !== t.tagger && (
                        <span className="block text-xs text-muted-foreground" title="Tagger covered by the signature">
                          {t.signed_tagger}
                        </span>
                      )}
                    </TableCell>
                    <TableCell>
                      <RelativeTime value={t.created_at} className="text-muted-foreground" />
                    </TableCell>
                    {canManage && (
                      <TableCell className="text-right">
                        <DeleteTagButton tag={t} />
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
