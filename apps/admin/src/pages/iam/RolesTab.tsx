import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Pencil, Plus, Puzzle, Trash2 } from "lucide-react";
import { Fragment, useId, useMemo, useState, type FormEvent } from "react";
import {
  actionArea,
  groupByArea,
  iamKeys,
  permits,
  useIamActions,
  useIamApi,
  useRoles,
  type IamAction,
  type RoleDef,
} from "@/api/iam";
import { EmptyState, FormError, QueryError, TableSkeleton } from "@/components/people/states";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";
import { Dialog, DialogClose, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { Textarea } from "@/components/ui/textarea";
import { useToast } from "@/toast/store";
import { AllowedMark, ConfirmAction, FieldErrors, TableCard } from "./shared";
import { isBuiltinRole, ROLE_NAME_HINT, ROLE_NAME_RE } from "./util";

interface RolesTabProps {
  slug: string;
  editable: boolean;
}

export function RolesTab({ slug, editable }: RolesTabProps) {
  const roles = useRoles(slug);
  const actions = useIamActions();
  const [editing, setEditing] = useState<RoleDef | "new" | null>(null);
  const [deleting, setDeleting] = useState("");
  const iam = useIamApi();
  const queryClient = useQueryClient();
  const toast = useToast();

  const remove = useMutation({
    mutationFn: (name: string) => iam.roles.remove(slug, name),
    meta: { inlineError: true },
    onSuccess: async (_r, name) => {
      toast.success("Role deleted", name);
      await queryClient.invalidateQueries({ queryKey: iamKeys.roles(slug) });
    },
  });

  const builtin = roles.data?.filter((r) => r.builtin) ?? [];
  const custom = roles.data?.filter((r) => !r.builtin) ?? [];

  return (
    <div className="flex flex-col gap-4">
      <TableCard
        description="Custom roles combine individual actions. Assign them like built-in roles: to members, teams, grants and service accounts."
        actions={
          editable && (
            <Button size="sm" onClick={() => setEditing("new")}>
              <Plus aria-hidden="true" />
              New role
            </Button>
          )
        }
      >
        {roles.isPending ? (
          <TableSkeleton label="Loading roles" rows={3} />
        ) : roles.isError ? (
          <QueryError error={roles.error} onRetry={() => void roles.refetch()} title="Could not load roles" />
        ) : custom.length === 0 ? (
          <EmptyState icon={Puzzle} title="No custom roles">
            Built-in roles cover most needs. Create a custom role for anything in between.
          </EmptyState>
        ) : (
          <Table aria-label="Custom roles">
            <TableHeader className="sticky top-0 z-10 bg-background">
              <TableRow className="hover:bg-transparent">
                <TableHead>Role</TableHead>
                <TableHead>Permissions</TableHead>
                {editable && (
                  <TableHead>
                    <span className="sr-only">Actions</span>
                  </TableHead>
                )}
              </TableRow>
            </TableHeader>
            <TableBody>
              {custom.map((r) => (
                <TableRow key={r.name}>
                  <TableCell className="align-top">
                    <p className="font-mono font-medium">{r.name}</p>
                    {r.description && <p className="text-xs whitespace-normal text-muted-foreground">{r.description}</p>}
                  </TableCell>
                  <TableCell className="whitespace-normal">
                    <ul aria-label={`Permissions of ${r.name}`} className="flex flex-wrap gap-1">
                      {r.permissions.map((p) => (
                        <li key={p}>
                          <Badge variant={p.endsWith("*") ? "default" : "secondary"} className="font-mono">
                            {p}
                          </Badge>
                        </li>
                      ))}
                    </ul>
                  </TableCell>
                  {editable && (
                    <TableCell className="w-20 text-right align-top">
                      <div className="flex justify-end gap-1">
                        <Button variant="ghost" size="icon-sm" aria-label={`Edit ${r.name}`} onClick={() => setEditing(r)}>
                          <Pencil aria-hidden="true" />
                        </Button>
                        <Button variant="ghost" size="icon-sm" aria-label={`Delete ${r.name}`} onClick={() => setDeleting(r.name)}>
                          <Trash2 aria-hidden="true" />
                        </Button>
                      </div>
                    </TableCell>
                  )}
                </TableRow>
              ))}
            </TableBody>
          </Table>
        )}
      </TableCard>

      <Card>
        <CardHeader>
          <CardTitle>Built-in roles</CardTitle>
          <CardDescription>Read-only. Each built-in role includes everything the roles to its right can do.</CardDescription>
        </CardHeader>
        <CardContent>
          {actions.isPending || roles.isPending ? (
            <TableSkeleton label="Loading permission matrix" rows={4} />
          ) : actions.isError ? (
            <QueryError error={actions.error} onRetry={() => void actions.refetch()} title="Could not load actions" />
          ) : (
            <BuiltinMatrix actions={actions.data} roles={builtin} />
          )}
        </CardContent>
      </Card>

      {editable && editing !== null && (
        <RoleEditorDialog
          tenant={slug}
          role={editing === "new" ? null : editing}
          actions={actions.data ?? []}
          onClose={() => setEditing(null)}
        />
      )}
      <ConfirmAction
        open={deleting !== ""}
        onOpenChange={(o) => !o && setDeleting("")}
        title={`Delete role ${deleting}?`}
        description="Only roles that are not assigned to anyone can be deleted."
        confirmLabel="Delete role"
        conflictHint="Reassign the members, teams, tree grants, service accounts and invitations that use it, then try again."
        onConfirm={() => remove.mutateAsync(deleting)}
      />
    </div>
  );
}

function BuiltinMatrix({ actions, roles }: { actions: IamAction[]; roles: RoleDef[] }) {
  const groups = groupByArea(actions, (a) => a.area || actionArea(a.action));
  return (
    <div className="overflow-x-auto">
      <Table aria-label="Built-in role permissions">
        <TableHeader>
          <TableRow className="hover:bg-transparent">
            <TableHead>Action</TableHead>
            {roles.map((r) => (
              <TableHead key={r.name} className="text-center capitalize">
                {r.name}
              </TableHead>
            ))}
          </TableRow>
        </TableHeader>
        <TableBody>
          {groups.map(([area, list]) => (
            <Fragment key={area}>
              <TableRow className="bg-muted/50 hover:bg-muted/50">
                <TableHead colSpan={roles.length + 1} scope="colgroup" className="h-8 text-xs tracking-wide uppercase">
                  {area}
                </TableHead>
              </TableRow>
              {list.map((a) => (
                <TableRow key={a.action}>
                  <TableHead scope="row" className="h-auto py-2 font-normal">
                    <span className="block font-mono text-xs text-foreground">{a.action}</span>
                    <span className="block text-xs whitespace-normal text-muted-foreground">{a.description}</span>
                  </TableHead>
                  {roles.map((r) => (
                    <TableCell key={r.name} className="text-center">
                      <AllowedMark allowed={permits(r.permissions, a.action)} label={`${r.name} ${a.action}`} />
                    </TableCell>
                  ))}
                </TableRow>
              ))}
            </Fragment>
          ))}
        </TableBody>
      </Table>
    </div>
  );
}

function RoleEditorDialog({
  tenant,
  role,
  actions,
  onClose,
}: {
  tenant: string;
  role: RoleDef | null;
  actions: IamAction[];
  onClose: () => void;
}) {
  const iam = useIamApi();
  const queryClient = useQueryClient();
  const toast = useToast();
  const id = useId();
  const [name, setName] = useState(role?.name ?? "");
  const [description, setDescription] = useState(role?.description ?? "");
  const [perms, setPerms] = useState<string[]>(role?.permissions ?? []);
  const [touched, setTouched] = useState(false);
  const groups = useMemo(() => groupByArea(actions, (a) => a.area || actionArea(a.action)), [actions]);
  const known = useMemo(() => new Set([...actions.map((a) => a.action), ...groups.map(([g]) => `${g}:*`)]), [actions, groups]);
  const extra = perms.filter((p) => !known.has(p));

  const save = useMutation({
    mutationFn: () =>
      role
        ? iam.roles.update(tenant, role.name, { description: description.trim(), permissions: perms })
        : iam.roles.create(tenant, { name, description: description.trim(), permissions: perms }),
    meta: { inlineError: true },
    onSuccess: async (r) => {
      toast.success(role ? "Role updated" : "Role created", r.name);
      await queryClient.invalidateQueries({ queryKey: iamKeys.roles(tenant) });
      onClose();
    },
  });

  const errors = {
    name: role ? [] : !ROLE_NAME_RE.test(name) ? [ROLE_NAME_HINT] : isBuiltinRole(name) ? ["That name is a built-in role."] : [],
    perms: perms.length ? [] : ["Pick at least one action."],
  };

  const toggle = (p: string, on: boolean) => setPerms((cur) => (on ? [...cur, p] : cur.filter((x) => x !== p)));
  const toggleArea = (area: string, on: boolean) =>
    setPerms((cur) => (on ? [...cur.filter((p) => actionArea(p) !== area), `${area}:*`] : cur.filter((p) => p !== `${area}:*`)));

  const submit = (e: FormEvent) => {
    e.preventDefault();
    setTouched(true);
    if (errors.name.length || errors.perms.length) return;
    save.mutate();
  };

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle>{role ? `Edit role ${role.name}` : "New custom role"}</DialogTitle>
          <DialogDescription>Choose the actions this role allows. Area wildcards include actions added in future versions.</DialogDescription>
        </DialogHeader>
        <form onSubmit={submit} className="flex min-w-0 flex-col gap-4" noValidate>
          <div className="grid gap-4 sm:grid-cols-2">
            <div className="grid content-start gap-2">
              <Label htmlFor={`${id}-name`}>Name</Label>
              <Input
                id={`${id}-name`}
                value={name}
                disabled={role !== null}
                onChange={(e) => setName(e.target.value.toLowerCase())}
                className="font-mono"
                placeholder="release-manager"
                spellCheck={false}
                aria-invalid={touched && errors.name.length > 0}
              />
              {touched && <FieldErrors messages={errors.name} />}
            </div>
            <div className="grid content-start gap-2">
              <Label htmlFor={`${id}-desc`}>Description</Label>
              <Textarea id={`${id}-desc`} value={description} onChange={(e) => setDescription(e.target.value)} rows={2} />
            </div>
          </div>
          <fieldset className="grid min-w-0 gap-2">
            <legend className="mb-2 flex items-center gap-2 text-sm font-medium">
              Permissions <Badge variant="secondary">{perms.length}</Badge>
            </legend>
            <ScrollArea className="h-72 rounded-md border">
              <div className="grid gap-4 p-3 sm:grid-cols-2">
                {groups.map(([area, list]) => {
                  const all = perms.includes(`${area}:*`) || perms.includes("*");
                  return (
                    <fieldset key={area} className="grid content-start gap-2" aria-label={`${area} actions`}>
                      <legend className="sr-only">{area}</legend>
                      <label className="flex items-center gap-2 text-sm font-semibold capitalize">
                        <Checkbox checked={perms.includes(`${area}:*`)} onCheckedChange={(v) => toggleArea(area, v === true)} />
                        {area} <span className="font-mono text-xs font-normal text-muted-foreground">{area}:*</span>
                      </label>
                      <ul className="grid gap-1.5 pl-6">
                        {list.map((a) => (
                          <li key={a.action}>
                            <label className="flex items-start gap-2 text-sm">
                              <Checkbox
                                className="mt-0.5"
                                checked={all || perms.includes(a.action)}
                                disabled={all}
                                onCheckedChange={(v) => toggle(a.action, v === true)}
                                aria-describedby={`${id}-${a.action}`}
                              />
                              <span className="min-w-0">
                                <span className="block font-mono text-xs">{a.action}</span>
                                <span id={`${id}-${a.action}`} className="block text-xs text-muted-foreground">
                                  {a.description}
                                </span>
                              </span>
                            </label>
                          </li>
                        ))}
                      </ul>
                    </fieldset>
                  );
                })}
              </div>
            </ScrollArea>
            {extra.length > 0 && (
              <p className="text-xs text-muted-foreground">
                Also keeps: <span className="font-mono">{extra.join(", ")}</span>
              </p>
            )}
            {touched && <FieldErrors messages={errors.perms} />}
          </fieldset>
          <FormError error={save.error} />
          <DialogFooter>
            <DialogClose asChild>
              <Button type="button" variant="outline">
                Cancel
              </Button>
            </DialogClose>
            <Button type="submit" disabled={save.isPending}>
              {save.isPending ? "Saving…" : role ? "Save role" : "Create role"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
