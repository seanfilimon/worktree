import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Ban, Lock, Pencil, Plus, Tag as TagIcon } from "lucide-react";
import { useId, useMemo, useState, type FormEvent } from "react";
import { errorMessage } from "@/api/client";
import { useTags } from "@/api/queries";
import { isNotImplemented, useTagProtections, useTreeV05Api, v05Keys, type TagProtection } from "@/api/v05tree";
import { ChipsInput } from "@/components/files/ChipsInput";
import { ConfirmDelete } from "@/components/files/ConfirmDelete";
import { MatchPreview } from "@/components/files/MatchPreview";
import { EmptyState, ErrorState, LoadingRows } from "@/components/files/states";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { pathMatch } from "@/lib/glob";
import { useToast } from "@/toast/store";
import { useTreeContext } from "../context";
import { FeatureUnavailable } from "./shared";

interface FormState {
  pattern: string;
  no_delete: boolean;
  no_move: boolean;
  allowed_taggers: string[];
}

const NEW_RULE: FormState = { pattern: "", no_delete: true, no_move: true, allowed_taggers: [] };

function patternError(pattern: string): string | undefined {
  const p = pattern.trim();
  if (!p) return "Required, e.g. v* or release-*.";
  if (/\s/.test(p)) return "Tag patterns cannot contain spaces.";
  if ((p.match(/\[/g)?.length ?? 0) !== (p.match(/\]/g)?.length ?? 0)) return "Unbalanced [ ] in pattern.";
  return undefined;
}

function SwitchRow({ id, label, description, checked, onChange }: { id: string; label: string; description: string; checked: boolean; onChange: (v: boolean) => void }) {
  return (
    <div className="flex items-start justify-between gap-4 rounded-md border p-3">
      <div className="grid gap-1">
        <Label htmlFor={id}>{label}</Label>
        <p id={`${id}-desc`} className="text-xs text-muted-foreground">
          {description}
        </p>
      </div>
      <Switch id={id} checked={checked} onCheckedChange={onChange} aria-describedby={`${id}-desc`} />
    </div>
  );
}

function TagProtectionDialog({ initial, editing, onClose }: { initial: FormState; editing: boolean; onClose: () => void }) {
  const api = useTreeV05Api();
  const { tenant, tree, summary } = useTreeContext();
  const tags = useTags(tenant, tree);
  const queryClient = useQueryClient();
  const toast = useToast();
  const id = useId();
  const [form, setForm] = useState(initial);
  const [touched, setTouched] = useState(false);
  const set = <K extends keyof FormState>(k: K, v: FormState[K]) => setForm((f) => ({ ...f, [k]: v }));

  const pattern = form.pattern.trim();
  const error = patternError(form.pattern);
  const names = useMemo(() => (tags.data ?? []).map((t) => t.name), [tags.data]);
  const matches = useMemo(() => (pattern && !error ? names.filter((n) => pathMatch(pattern, n)) : []), [names, pattern, error]);

  const save = useMutation({
    mutationFn: (body: TagProtection) => api.tagProtections.put(tenant, tree, body),
    meta: { inlineError: true },
    onSuccess: async (p) => {
      toast.success(editing ? "Tag protection updated" : "Tag protection added", p?.pattern ?? pattern);
      onClose();
      await queryClient.invalidateQueries({ queryKey: v05Keys.tagProtections(tenant, tree) });
    },
  });

  const submit = (e: FormEvent) => {
    e.preventDefault();
    setTouched(true);
    if (error) return;
    save.mutate({
      tree_id: summary.tree.id,
      pattern,
      no_delete: form.no_delete,
      no_move: form.no_move,
      allowed_taggers: form.allowed_taggers,
    });
  };

  return (
    <DialogContent className="sm:max-w-lg">
      <DialogHeader>
        <DialogTitle>{editing ? `Edit tag protection: ${initial.pattern}` : "Add tag protection"}</DialogTitle>
        <DialogDescription>Rules apply to every tag whose name matches the pattern.</DialogDescription>
      </DialogHeader>
      <form id={`${id}-form`} onSubmit={submit} aria-label={editing ? "Edit tag protection" : "Add tag protection"} className="grid gap-4" noValidate>
        <div className="grid gap-2">
          <Label htmlFor={`${id}-pattern`}>Tag pattern</Label>
          <Input
            id={`${id}-pattern`}
            value={form.pattern}
            onChange={(e) => set("pattern", e.target.value)}
            readOnly={editing}
            placeholder="v*"
            autoComplete="off"
            className="font-mono"
            aria-invalid={touched && error ? true : undefined}
            aria-describedby={`${id}-pattern-hint`}
          />
          <p id={`${id}-pattern-hint`} className={touched && error ? "text-xs font-semibold" : "text-xs text-muted-foreground"}>
            {(touched && error) || "Glob: * matches any run of characters, ? one character."}
          </p>
          <MatchPreview
            noun={["tag", "tags"]}
            matches={matches}
            total={names.length}
            pending={tags.isPending}
            empty={pattern ? "No existing tag matches." : "Type a pattern to preview the tags it protects."}
          />
        </div>
        <SwitchRow
          id={`${id}-delete`}
          label="Prevent deletion"
          description="Matching tags cannot be deleted."
          checked={form.no_delete}
          onChange={(v) => set("no_delete", v)}
        />
        <SwitchRow
          id={`${id}-move`}
          label="Prevent moving"
          description="An existing matching tag cannot be re-pointed to another snapshot."
          checked={form.no_move}
          onChange={(v) => set("no_move", v)}
        />
        <div className="grid gap-2">
          <Label htmlFor={`${id}-taggers`}>Allowed taggers</Label>
          <ChipsInput
            id={`${id}-taggers`}
            values={form.allowed_taggers}
            onChange={(v) => set("allowed_taggers", v)}
            placeholder="alice, release-bot"
            describedBy={`${id}-taggers-hint`}
          />
          <p id={`${id}-taggers-hint`} className="text-xs text-muted-foreground">
            Usernames that may create matching tags. Empty = anyone who can write to the tree.
          </p>
        </div>
        {save.isError && (
          <Alert>
            <AlertDescription className="text-foreground">{errorMessage(save.error)}</AlertDescription>
          </Alert>
        )}
      </form>
      <DialogFooter>
        <Button variant="outline" onClick={onClose}>
          Cancel
        </Button>
        <Button type="submit" form={`${id}-form`} disabled={save.isPending}>
          {editing ? "Save changes" : "Add tag protection"}
        </Button>
      </DialogFooter>
    </DialogContent>
  );
}

/** Tag protections (§13.4): `GET|PUT|DELETE {tree}/tag-protections`. */
export function TagProtectionsSection() {
  const api = useTreeV05Api();
  const { tenant, tree, canManage } = useTreeContext();
  const rules = useTagProtections(tenant, tree);
  const tags = useTags(tenant, tree);
  const queryClient = useQueryClient();
  const toast = useToast();
  const [dialog, setDialog] = useState<{ initial: FormState; editing: boolean } | null>(null);
  const unavailable = rules.isError && isNotImplemented(rules.error);

  const remove = useMutation({
    mutationFn: (pattern: string) => api.tagProtections.remove(tenant, tree, pattern),
    onSuccess: async (_r, pattern) => {
      toast.success("Tag protection removed", pattern);
      await queryClient.invalidateQueries({ queryKey: v05Keys.tagProtections(tenant, tree) });
    },
  });

  const list = rules.data ?? [];
  const names = (tags.data ?? []).map((t) => t.name);

  return (
    <section aria-labelledby="tag-protections-title" className="overflow-hidden rounded-xl border bg-card">
      <div className="flex flex-wrap items-center justify-between gap-2 border-b px-4 py-3">
        <div>
          <h2 id="tag-protections-title" className="font-semibold">
            Tag protections
          </h2>
          <p className="text-sm text-muted-foreground">Keep release tags from being deleted or re-pointed.</p>
        </div>
        {canManage && !unavailable && (
          <Button size="sm" variant="outline" onClick={() => setDialog({ initial: NEW_RULE, editing: false })}>
            <Plus aria-hidden="true" />
            Add tag protection
          </Button>
        )}
      </div>
      {unavailable ? (
        <FeatureUnavailable feature="Tag protections" />
      ) : rules.isPending ? (
        <LoadingRows rows={2} label="Loading tag protections" />
      ) : rules.isError ? (
        <ErrorState error={rules.error} onRetry={() => void rules.refetch()} />
      ) : list.length === 0 ? (
        <EmptyState icon={TagIcon} title="No protected tags">
          Protect <code className="font-mono">v*</code> so published versions stay put.
        </EmptyState>
      ) : (
        <Table aria-label="Tag protections">
          <TableHeader>
            <TableRow>
              <TableHead>Pattern</TableHead>
              <TableHead>Rules</TableHead>
              <TableHead className="hidden md:table-cell">Allowed taggers</TableHead>
              {canManage && (
                <TableHead>
                  <span className="sr-only">Actions</span>
                </TableHead>
              )}
            </TableRow>
          </TableHeader>
          <TableBody>
            {list.map((p) => {
              const matched = names.filter((n) => pathMatch(p.pattern, n)).length;
              const taggers = p.allowed_taggers ?? [];
              return (
                <TableRow key={p.pattern}>
                  <TableCell>
                    <span className="block font-mono font-semibold">{p.pattern}</span>
                    <span className="text-xs text-muted-foreground">
                      {matched} {matched === 1 ? "tag" : "tags"}
                    </span>
                  </TableCell>
                  <TableCell>
                    <span className="flex flex-wrap gap-1">
                      {p.no_move && (
                        <Badge>
                          <Lock aria-hidden="true" />
                          no move
                        </Badge>
                      )}
                      {p.no_delete && (
                        <Badge variant="outline">
                          <Ban aria-hidden="true" />
                          no delete
                        </Badge>
                      )}
                      {!p.no_move && !p.no_delete && <span className="text-muted-foreground">—</span>}
                    </span>
                  </TableCell>
                  <TableCell className="hidden max-w-60 truncate text-muted-foreground md:table-cell">
                    {taggers.length ? <span className="font-mono">{taggers.join(", ")}</span> : "anyone with write access"}
                  </TableCell>
                  {canManage && (
                    <TableCell className="text-right whitespace-nowrap">
                      <Button
                        variant="ghost"
                        size="icon-sm"
                        aria-label={`Edit tag protection ${p.pattern}`}
                        onClick={() =>
                          setDialog({
                            initial: { pattern: p.pattern, no_delete: p.no_delete, no_move: p.no_move, allowed_taggers: taggers },
                            editing: true,
                          })
                        }
                      >
                        <Pencil aria-hidden="true" />
                      </Button>
                      <ConfirmDelete
                        label={`Delete tag protection ${p.pattern}`}
                        title={`Delete tag protection “${p.pattern}”?`}
                        onConfirm={() => remove.mutate(p.pattern)}
                        pending={remove.isPending && remove.variables === p.pattern}
                      >
                        {matched > 0 ? `${matched} ${matched === 1 ? "tag loses" : "tags lose"} this protection.` : "No tag currently matches."}
                      </ConfirmDelete>
                    </TableCell>
                  )}
                </TableRow>
              );
            })}
          </TableBody>
        </Table>
      )}
      <Dialog open={dialog !== null} onOpenChange={(open) => !open && setDialog(null)}>
        {dialog && (
          <TagProtectionDialog
            key={dialog.initial.pattern || "new"}
            initial={dialog.initial}
            editing={dialog.editing}
            onClose={() => setDialog(null)}
          />
        )}
      </Dialog>
    </section>
  );
}
