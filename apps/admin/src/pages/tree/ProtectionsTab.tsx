import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Ban, Info, Lock, Pencil, Plus, ShieldCheck } from "lucide-react";
import { useId, useMemo, useState, type FormEvent } from "react";
import { useApi } from "@/api/context";
import { keys, useBranches, useProtections } from "@/api/queries";
import type { BranchProtection } from "@/api/types";
import { ChipsInput } from "@/components/files/ChipsInput";
import { ConfirmDelete } from "@/components/files/ConfirmDelete";
import { MatchPreview } from "@/components/files/MatchPreview";
import { EmptyState, ErrorState, LoadingRows } from "@/components/files/states";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { pathMatch } from "@/lib/glob";
import { useToast } from "@/toast/store";
import { useTreeContext } from "./context";

interface FormState {
  pattern: string;
  no_direct_push: boolean;
  required_approvals: string;
  allowed_pushers: string[];
  no_delete: boolean;
}

const NEW_RULE: FormState = { pattern: "", no_direct_push: true, required_approvals: "1", allowed_pushers: [], no_delete: true };

function toForm(p: BranchProtection): FormState {
  return {
    pattern: p.pattern,
    no_direct_push: p.no_direct_push,
    required_approvals: String(p.required_approvals),
    allowed_pushers: p.allowed_pushers ?? [],
    no_delete: p.no_delete,
  };
}

/** Problems Go's path.Match would reject, or that make a pattern useless. */
function patternError(pattern: string): string | undefined {
  const p = pattern.trim();
  if (!p) return "Required, e.g. main or release/*.";
  if (/\s/.test(p)) return "Branch patterns cannot contain spaces.";
  let open = false;
  for (let i = 0; i < p.length; i++) {
    const c = p.charAt(i);
    if (c === "\\") i++;
    else if (c === "[") {
      if (open) return "Character classes cannot be nested.";
      open = true;
    } else if (c === "]") open = false;
  }
  return open ? "Unclosed [ in pattern." : undefined;
}

function SwitchField({
  id,
  label,
  description,
  checked,
  onChange,
}: {
  id: string;
  label: string;
  description: string;
  checked: boolean;
  onChange: (v: boolean) => void;
}) {
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

function ProtectionDialog({
  initial,
  editing,
  existing,
  onClose,
}: {
  initial: FormState;
  editing: boolean;
  existing: readonly string[];
  onClose: () => void;
}) {
  const api = useApi();
  const { tenant, tree, summary } = useTreeContext();
  const branches = useBranches(tenant, tree);
  const queryClient = useQueryClient();
  const toast = useToast();
  const id = useId();
  const [form, setForm] = useState(initial);
  const [touched, setTouched] = useState(false);
  const set = <K extends keyof FormState>(k: K, v: FormState[K]) => setForm((f) => ({ ...f, [k]: v }));

  const approvals = Number(form.required_approvals);
  const errors = {
    pattern: patternError(form.pattern),
    approvals:
      form.required_approvals.trim() !== "" && Number.isInteger(approvals) && approvals >= 0 && approvals <= 20
        ? undefined
        : "A whole number from 0 to 20.",
  };
  const pattern = form.pattern.trim();
  const names = useMemo(() => (branches.data ?? []).map((b) => b.name), [branches.data]);
  const matches = useMemo(
    () => (pattern && !errors.pattern ? names.filter((n) => pathMatch(pattern, n)) : []),
    [names, pattern, errors.pattern],
  );
  const replaces = !editing && existing.includes(pattern);

  const save = useMutation({
    mutationFn: (body: BranchProtection) => api.trees.putProtection(tenant, tree, body),
    onSuccess: async (p) => {
      toast.success(editing ? "Protection updated" : "Protection added", p.pattern);
      onClose();
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: keys.protections(tenant, tree) }),
        queryClient.invalidateQueries({ queryKey: keys.branches(tenant, tree) }),
      ]);
    },
  });

  const onSubmit = (e: FormEvent) => {
    e.preventDefault();
    setTouched(true);
    if (errors.pattern || errors.approvals) return;
    save.mutate({
      tree_id: summary.tree.id,
      pattern,
      no_direct_push: form.no_direct_push,
      required_approvals: approvals,
      allowed_pushers: form.allowed_pushers,
      no_delete: form.no_delete,
    });
  };

  const showPattern = touched && errors.pattern;
  const showApprovals = touched && errors.approvals;

  return (
    <DialogContent className="sm:max-w-lg">
      <DialogHeader>
        <DialogTitle>{editing ? `Edit protection: ${initial.pattern}` : "Add branch protection"}</DialogTitle>
        <DialogDescription>Rules apply to every branch whose name matches the pattern, now and in future.</DialogDescription>
      </DialogHeader>
      <form
        id={`${id}-form`}
        onSubmit={onSubmit}
        aria-label={editing ? "Edit protection" : "Add protection"}
        className="grid gap-4"
        noValidate
      >
        <div className="grid gap-2">
          <Label htmlFor={`${id}-pattern`}>Branch pattern</Label>
          <Input
            id={`${id}-pattern`}
            value={form.pattern}
            onChange={(e) => set("pattern", e.target.value)}
            readOnly={editing}
            placeholder="release/*"
            autoComplete="off"
            className="font-mono"
            aria-invalid={showPattern ? true : undefined}
            aria-describedby={`${id}-pattern-hint`}
          />
          <p id={`${id}-pattern-hint`} className={showPattern ? "text-xs font-semibold" : "text-xs text-muted-foreground"}>
            {showPattern ||
              (replaces
                ? "A rule with this pattern exists — saving replaces it."
                : "Glob: * matches within one path segment, ? one character, [abc] a class.")}
          </p>
          <MatchPreview
            noun={["branch", "branches"]}
            matches={matches}
            total={names.length}
            pending={branches.isPending}
            empty={
              pattern
                ? "No existing branch matches. The rule will apply to matching branches when they are created."
                : "Type a pattern to preview the branches it protects."
            }
          />
        </div>
        <SwitchField
          id={`${id}-push`}
          label="No direct push"
          description="Changes land only through merge requests."
          checked={form.no_direct_push}
          onChange={(v) => set("no_direct_push", v)}
        />
        <div className="grid gap-2">
          <Label htmlFor={`${id}-approvals`}>Required approvals</Label>
          <Input
            id={`${id}-approvals`}
            type="number"
            inputMode="numeric"
            min={0}
            max={20}
            value={form.required_approvals}
            onChange={(e) => set("required_approvals", e.target.value)}
            className="w-28 tabular-nums"
            aria-invalid={showApprovals ? true : undefined}
            aria-describedby={`${id}-approvals-hint`}
          />
          <p id={`${id}-approvals-hint`} className={showApprovals ? "text-xs font-semibold" : "text-xs text-muted-foreground"}>
            {showApprovals || "Approvals a merge request needs before it can merge (0 to 20)."}
          </p>
        </div>
        <div className="grid gap-2">
          <Label htmlFor={`${id}-pushers`}>Allowed pushers</Label>
          <ChipsInput
            id={`${id}-pushers`}
            values={form.allowed_pushers}
            onChange={(v) => set("allowed_pushers", v)}
            placeholder="alice, bob"
            describedBy={`${id}-pushers-hint`}
          />
          <p id={`${id}-pushers-hint`} className="text-xs text-muted-foreground">
            Usernames that may push. Press Enter or comma to add. Empty = anyone with a write role.
          </p>
        </div>
        <SwitchField
          id={`${id}-delete`}
          label="Prevent deletion"
          description="Matching branches cannot be deleted."
          checked={form.no_delete}
          onChange={(v) => set("no_delete", v)}
        />
      </form>
      <DialogFooter>
        <Button variant="outline" onClick={onClose}>
          Cancel
        </Button>
        <Button type="submit" form={`${id}-form`} disabled={save.isPending}>
          {editing ? "Save changes" : "Add protection"}
        </Button>
      </DialogFooter>
    </DialogContent>
  );
}

function Explanation() {
  return (
    <Card className="gap-3 py-4">
      <CardHeader className="px-4">
        <CardTitle className="flex items-center gap-2 text-sm">
          <Info aria-hidden="true" className="size-4" />
          How protections work
        </CardTitle>
      </CardHeader>
      <CardContent className="grid gap-2 px-4 text-sm text-muted-foreground">
        <p>
          A rule applies to every branch whose name matches its pattern (Go <code className="font-mono text-foreground">path.Match</code>):{" "}
          <code className="font-mono text-foreground">main</code> matches only main,{" "}
          <code className="font-mono text-foreground">release/*</code> matches release/1.0 but not release/1.0/hotfix.
        </p>
        <p>
          <span className="font-semibold text-foreground">No direct push</span> forces changes through merge requests, which then need the{" "}
          <span className="font-semibold text-foreground">required approvals</span>.{" "}
          <span className="font-semibold text-foreground">Allowed pushers</span> restricts who may push at all;{" "}
          <span className="font-semibold text-foreground">prevent deletion</span> blocks removing the branch.
        </p>
        <p>Staged work in progress is never blocked — only pushes and merges are checked.</p>
      </CardContent>
    </Card>
  );
}

export function ProtectionsTab() {
  const api = useApi();
  const { tenant, tree, canManage } = useTreeContext();
  const protections = useProtections(tenant, tree);
  const branches = useBranches(tenant, tree);
  const queryClient = useQueryClient();
  const toast = useToast();
  const [dialog, setDialog] = useState<{ initial: FormState; editing: boolean } | null>(null);

  const remove = useMutation({
    mutationFn: (pattern: string) => api.trees.deleteProtection(tenant, tree, pattern),
    onSuccess: async (_r, pattern) => {
      toast.success("Protection removed", pattern);
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: keys.protections(tenant, tree) }),
        queryClient.invalidateQueries({ queryKey: keys.branches(tenant, tree) }),
      ]);
    },
  });

  const rules = protections.data ?? [];
  const names = (branches.data ?? []).map((b) => b.name);

  return (
    <div className="grid gap-5 lg:grid-cols-[minmax(0,1fr)_20rem] lg:items-start">
      <section aria-labelledby="protections-title" className="overflow-hidden rounded-xl border bg-card">
        <div className="flex flex-wrap items-center justify-between gap-2 border-b px-4 py-3">
          <div>
            <h2 id="protections-title" className="font-semibold">
              Branch protections
            </h2>
            <p className="text-sm text-muted-foreground">Guard important branches against direct pushes and deletion.</p>
          </div>
          {canManage && (
            <Button size="sm" onClick={() => setDialog({ initial: NEW_RULE, editing: false })}>
              <Plus aria-hidden="true" />
              Add protection
            </Button>
          )}
        </div>
        {protections.isPending ? (
          <LoadingRows rows={3} label="Loading protections" />
        ) : protections.isError ? (
          <ErrorState error={protections.error} onRetry={() => void protections.refetch()} />
        ) : rules.length === 0 ? (
          <EmptyState icon={ShieldCheck} title="No protected branches">
            Protect your default branch to require reviews before merging.
          </EmptyState>
        ) : (
          <Table aria-label="Branch protections">
            <TableHeader>
              <TableRow>
                <TableHead>Pattern</TableHead>
                <TableHead>Rules</TableHead>
                <TableHead className="text-right">Approvals</TableHead>
                <TableHead className="hidden md:table-cell">Allowed pushers</TableHead>
                {canManage && (
                  <TableHead>
                    <span className="sr-only">Actions</span>
                  </TableHead>
                )}
              </TableRow>
            </TableHeader>
            <TableBody>
              {rules.map((p) => {
                const matched = names.filter((n) => pathMatch(p.pattern, n)).length;
                return (
                  <TableRow key={p.pattern}>
                    <TableCell>
                      <span className="block font-mono font-semibold">{p.pattern}</span>
                      <span className="text-xs text-muted-foreground">
                        {matched} {matched === 1 ? "branch" : "branches"}
                      </span>
                    </TableCell>
                    <TableCell>
                      <span className="flex flex-wrap gap-1">
                        {p.no_direct_push && (
                          <Badge>
                            <Lock aria-hidden="true" />
                            no direct push
                          </Badge>
                        )}
                        {p.no_delete && (
                          <Badge variant="outline">
                            <Ban aria-hidden="true" />
                            no delete
                          </Badge>
                        )}
                        {!p.no_direct_push && !p.no_delete && <span className="text-muted-foreground">—</span>}
                      </span>
                    </TableCell>
                    <TableCell className="text-right font-semibold tabular-nums">{p.required_approvals}</TableCell>
                    <TableCell className="hidden max-w-60 truncate text-muted-foreground md:table-cell">
                      {(p.allowed_pushers ?? []).length ? (
                        <span className="font-mono">{(p.allowed_pushers ?? []).join(", ")}</span>
                      ) : (
                        "role-based"
                      )}
                    </TableCell>
                    {canManage && (
                      <TableCell className="text-right whitespace-nowrap">
                        <Button
                          variant="ghost"
                          size="icon-sm"
                          aria-label={`Edit ${p.pattern}`}
                          onClick={() => setDialog({ initial: toForm(p), editing: true })}
                        >
                          <Pencil aria-hidden="true" />
                        </Button>
                        <ConfirmDelete
                          label={`Delete ${p.pattern}`}
                          title={`Delete protection “${p.pattern}”?`}
                          onConfirm={() => remove.mutate(p.pattern)}
                          pending={remove.isPending && remove.variables === p.pattern}
                        >
                          {matched > 0
                            ? `${matched} ${matched === 1 ? "branch loses" : "branches lose"} this protection immediately.`
                            : "No branch currently matches this pattern."}
                        </ConfirmDelete>
                      </TableCell>
                    )}
                  </TableRow>
                );
              })}
            </TableBody>
          </Table>
        )}
      </section>
      <Explanation />
      <Dialog open={dialog !== null} onOpenChange={(open) => !open && setDialog(null)}>
        {dialog && (
          <ProtectionDialog
            key={dialog.initial.pattern || "new"}
            initial={dialog.initial}
            editing={dialog.editing}
            existing={rules.map((r) => r.pattern)}
            onClose={() => setDialog(null)}
          />
        )}
      </Dialog>
    </div>
  );
}
