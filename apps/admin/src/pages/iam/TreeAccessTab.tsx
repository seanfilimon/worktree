import { useMutation, useQueryClient } from "@tanstack/react-query";
import { FileCode2, KeyRound, Plus, Trash2 } from "lucide-react";
import { useId, useState, type FormEvent } from "react";
import { Link } from "react-router";
import {
  iamKeys,
  SUBJECT_TYPES,
  useIamApi,
  useServiceAccounts,
  useTeams,
  useTreeGrants,
  type Grant,
  type SubjectType,
} from "@/api/iam";
import { useMembers } from "@/api/queries";
import { canManage, useAuth } from "@/auth/context";
import { EmptyState, FormError, QueryError, TableSkeleton } from "@/components/people/states";
import { USERNAME_HINT, USERNAME_RE } from "@/components/people/validation";
import { Button } from "@/components/ui/button";
import { Dialog, DialogClose, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { absoluteTime, relativeTime } from "@/lib/format";
import { useToast } from "@/toast/store";
import { useTreeContext } from "@/pages/tree/context";
import { AccessConfigTab } from "./AccessConfigTab";
import { ConfirmAction, FieldErrors, IamRoleBadge, RoleSelect, SubjectIcon, SubjectLabel, TableCard } from "./shared";
import { SUBJECT_LABEL } from "./util";

/** Tree → Access: role grants for users, teams and service accounts. */
export function TreeAccessTab() {
  const { tenant, tree, summary } = useTreeContext();
  const { roleIn, isAdmin } = useAuth();
  const editable = canManage(roleIn(summary.tree.tenant_id), isAdmin);
  const grants = useTreeGrants(tenant, tree);
  const iam = useIamApi();
  const queryClient = useQueryClient();
  const toast = useToast();
  const [adding, setAdding] = useState(false);
  const [removing, setRemoving] = useState<Grant | null>(null);

  const remove = useMutation({
    mutationFn: (g: Grant) => iam.grants.remove(tenant, tree, g.subject_type, g.subject),
    meta: { inlineError: true },
    onSuccess: async (_r, g) => {
      toast.success("Grant removed", g.subject);
      await queryClient.invalidateQueries({ queryKey: iamKeys.grants(tenant, tree) });
    },
  });

  return (
    <>
      <TableCard
        description={
          <>
            Grants give a user, team or service account a role on this tree, on top of their tenant role. Check the result in the{" "}
            <Link to={`/iam/simulator?tenant=${encodeURIComponent(tenant)}&tree=${encodeURIComponent(tree)}`} className="underline underline-offset-4">
              access simulator
            </Link>
            .
          </>
        }
        actions={
          editable && (
            <Button size="sm" onClick={() => setAdding(true)}>
              <Plus aria-hidden="true" />
              Add grant
            </Button>
          )
        }
      >
        {grants.isPending ? (
          <TableSkeleton label="Loading grants" rows={3} />
        ) : grants.isError ? (
          <QueryError error={grants.error} onRetry={() => void grants.refetch()} title="Could not load grants" />
        ) : grants.data.length === 0 ? (
          <EmptyState icon={KeyRound} title="No grants">
            Access to {tenant}/{tree} comes from tenant roles{summary.tree.visibility === "private" ? "" : " and visibility"} only.
          </EmptyState>
        ) : (
          <Table aria-label="Grants">
            <TableHeader className="sticky top-0 z-10 bg-background">
              <TableRow className="hover:bg-transparent">
                <TableHead>Subject</TableHead>
                <TableHead>Role</TableHead>
                <TableHead className="hidden md:table-cell">Granted</TableHead>
                {editable && (
                  <TableHead>
                    <span className="sr-only">Actions</span>
                  </TableHead>
                )}
              </TableRow>
            </TableHeader>
            <TableBody>
              {grants.data.map((g) => (
                <TableRow key={`${g.subject_type}:${g.subject_id}`}>
                  <TableCell>
                    <SubjectLabel type={g.subject_type} name={g.subject} />
                  </TableCell>
                  <TableCell>
                    <IamRoleBadge role={g.role} />
                  </TableCell>
                  <TableCell className="hidden text-muted-foreground md:table-cell">
                    <time dateTime={g.created_at} title={absoluteTime(g.created_at)}>
                      {relativeTime(g.created_at)}
                    </time>
                    {g.created_by && ` by ${g.created_by}`}
                  </TableCell>
                  {editable && (
                    <TableCell className="w-10 text-right">
                      <Button variant="ghost" size="icon-sm" aria-label={`Remove grant for ${g.subject}`} onClick={() => setRemoving(g)}>
                        <Trash2 aria-hidden="true" />
                      </Button>
                    </TableCell>
                  )}
                </TableRow>
              ))}
            </TableBody>
          </Table>
        )}
      </TableCard>
      {/* v0.6 (§14.9): the tree-level declarative access document (deny statements only). */}
      <section aria-labelledby="tree-access-config" className="mt-6 flex flex-col gap-3">
        <div>
          <h2 id="tree-access-config" className="flex items-center gap-2 font-semibold">
            <FileCode2 aria-hidden="true" className="size-4" />
            Tree access config
          </h2>
          <p className="text-sm text-muted-foreground">
            Deny policies scoped to {tenant}/{tree}, applied from <span className="font-mono">.wt-tree/access/policies.toml</span>.
          </p>
        </div>
        <AccessConfigTab slug={tenant} tree={tree} editable={editable} />
      </section>
      {editable && adding && <AddGrantDialog onClose={() => setAdding(false)} />}
      <ConfirmAction
        open={removing !== null}
        onOpenChange={(o) => !o && setRemoving(null)}
        title={`Remove grant for ${removing?.subject ?? ""}?`}
        description="They keep their tenant role but lose the extra access this grant gave on this tree."
        confirmLabel="Remove grant"
        onConfirm={() => (removing ? remove.mutateAsync(removing) : Promise.resolve())}
      />
    </>
  );
}

function AddGrantDialog({ onClose }: { onClose: () => void }) {
  const { tenant, tree } = useTreeContext();
  const iam = useIamApi();
  const queryClient = useQueryClient();
  const toast = useToast();
  const id = useId();
  const [type, setType] = useState<SubjectType>("user");
  const [subject, setSubject] = useState("");
  const [role, setRole] = useState("developer");
  const [touched, setTouched] = useState(false);
  const members = useMembers(tenant);
  const teams = useTeams(tenant);
  const sas = useServiceAccounts(tenant);

  const put = useMutation({
    mutationFn: () => iam.grants.put(tenant, tree, { subject_type: type, subject: subject.trim(), role }),
    meta: { inlineError: true },
    onSuccess: async (g) => {
      toast.success("Grant saved", `${g.subject} · ${g.role}`);
      await queryClient.invalidateQueries({ queryKey: iamKeys.grants(tenant, tree) });
      onClose();
    },
  });

  const s = subject.trim();
  const errors = !s ? ["Required."] : type === "user" && !USERNAME_RE.test(s) ? [USERNAME_HINT] : [];

  const submit = (e: FormEvent) => {
    e.preventDefault();
    setTouched(true);
    if (errors.length) return;
    put.mutate();
  };

  const options =
    type === "team"
      ? (teams.data ?? []).map((t) => ({ value: t.slug, label: t.name ? `${t.name} (${t.slug})` : t.slug }))
      : (sas.data ?? []).map((sa) => ({ value: sa.name, label: sa.name }));

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Add grant</DialogTitle>
          <DialogDescription>
            Give a subject a role on <span className="font-mono text-foreground">{`${tenant}/${tree}`}</span>. An existing grant for the same
            subject is replaced.
          </DialogDescription>
        </DialogHeader>
        <form onSubmit={submit} className="flex flex-col gap-4" noValidate>
          <div className="grid gap-2">
            <span id={`${id}-type`} className="text-sm leading-none font-medium">
              Subject type
            </span>
            <ToggleGroup
              type="single"
              variant="outline"
              value={type}
              onValueChange={(v) => {
                if (!v) return;
                setType(v as SubjectType);
                setSubject("");
                setTouched(false);
              }}
              aria-labelledby={`${id}-type`}
              className="flex-wrap"
            >
              {SUBJECT_TYPES.map((t) => (
                <ToggleGroupItem key={t} value={t} className="data-[state=on]:bg-foreground data-[state=on]:text-background">
                  <SubjectIcon type={t} />
                  {SUBJECT_LABEL[t]}
                </ToggleGroupItem>
              ))}
            </ToggleGroup>
          </div>
          <div className="grid gap-2">
            <Label htmlFor={`${id}-subject`}>{type === "user" ? "Username" : SUBJECT_LABEL[type]}</Label>
            {type === "user" ? (
              <>
                <Input
                  id={`${id}-subject`}
                  value={subject}
                  onChange={(e) => setSubject(e.target.value.toLowerCase())}
                  list={`${id}-members`}
                  placeholder="jane"
                  className="font-mono"
                  autoComplete="off"
                  spellCheck={false}
                  aria-invalid={touched && errors.length > 0}
                  aria-describedby={`${id}-subject-hint`}
                />
                <datalist id={`${id}-members`}>
                  {(members.data ?? []).map((m) => (
                    <option key={m.account.id} value={m.account.username} />
                  ))}
                </datalist>
                <p id={`${id}-subject-hint`} className="text-xs text-muted-foreground">
                  Any account, including people outside {tenant}.
                </p>
              </>
            ) : (
              <Select value={subject} onValueChange={setSubject}>
                <SelectTrigger id={`${id}-subject`} className="w-full" aria-invalid={touched && errors.length > 0}>
                  <SelectValue placeholder={options.length ? `Choose a ${SUBJECT_LABEL[type].toLowerCase()}` : `No ${SUBJECT_LABEL[type].toLowerCase()}s yet`} />
                </SelectTrigger>
                <SelectContent>
                  {options.map((o) => (
                    <SelectItem key={o.value} value={o.value}>
                      {o.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            )}
            {touched && <FieldErrors messages={errors} />}
          </div>
          <div className="grid gap-2">
            <Label htmlFor={`${id}-role`}>Role</Label>
            <RoleSelect id={`${id}-role`} tenant={tenant} value={role} onChange={setRole} />
          </div>
          <FormError error={put.error} />
          <DialogFooter>
            <DialogClose asChild>
              <Button type="button" variant="outline">
                Cancel
              </Button>
            </DialogClose>
            <Button type="submit" disabled={put.isPending}>
              {put.isPending ? "Saving…" : "Save grant"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
