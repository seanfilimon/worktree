import { useMutation, useQueryClient } from "@tanstack/react-query";
import { ChevronRight, Plus, Trash2, UserMinus, UserPlus, Users } from "lucide-react";
import { useId, useState, type FormEvent } from "react";
import { Link } from "react-router";
import { iamKeys, TEAM_ROLES, useIamApi, useTeam, useTeams, type Team, type TeamRole } from "@/api/iam";
import { useTenantTrees } from "@/api/queries";
import { EmptyState, FormError, QueryError, TableSkeleton } from "@/components/people/states";
import { UserAvatar } from "@/components/people/UserAvatar";
import { slugify, USERNAME_HINT, USERNAME_RE } from "@/components/people/validation";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Dialog, DialogClose, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Separator } from "@/components/ui/separator";
import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from "@/components/ui/sheet";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { Textarea } from "@/components/ui/textarea";
import { absoluteTime, relativeTime } from "@/lib/format";
import { treePath } from "@/lib/paths";
import { useToast } from "@/toast/store";
import { ConfirmAction, FieldErrors, IamRoleBadge, TableCard } from "./shared";
import { IAM_SLUG_HINT, IAM_SLUG_RE } from "./util";

interface TeamsTabProps {
  slug: string;
  /** Caller can manage IAM in this tenant. */
  editable: boolean;
}

export function TeamsTab({ slug, editable }: TeamsTabProps) {
  const teams = useTeams(slug);
  const [creating, setCreating] = useState(false);
  const [open, setOpen] = useState<string>("");

  return (
    <>
      <TableCard
        description="Teams group accounts. Grant a team a role on a tree from the tree's Access tab."
        actions={
          editable && (
            <Button size="sm" onClick={() => setCreating(true)}>
              <Plus aria-hidden="true" />
              New team
            </Button>
          )
        }
      >
        {teams.isPending ? (
          <TableSkeleton label="Loading teams" rows={3} />
        ) : teams.isError ? (
          <QueryError error={teams.error} onRetry={() => void teams.refetch()} title="Could not load teams" />
        ) : teams.data.length === 0 ? (
          <EmptyState icon={Users} title="No teams yet">
            Create a team to manage access for a group of people at once.
          </EmptyState>
        ) : (
          <Table aria-label="Teams">
            <TableHeader className="sticky top-0 z-10 bg-background">
              <TableRow className="hover:bg-transparent">
                <TableHead>Team</TableHead>
                <TableHead className="hidden md:table-cell">Description</TableHead>
                <TableHead className="hidden sm:table-cell">Created</TableHead>
                <TableHead>
                  <span className="sr-only">Open</span>
                </TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {teams.data.map((t) => (
                <TableRow key={t.id} className="cursor-pointer" onClick={() => setOpen(t.slug)}>
                  <TableCell>
                    <div className="flex items-center gap-3">
                      <UserAvatar name={t.name || t.slug} square />
                      <div className="min-w-0">
                        <p className="font-medium">{t.name || t.slug}</p>
                        <p className="font-mono text-xs text-muted-foreground">{t.slug}</p>
                      </div>
                    </div>
                  </TableCell>
                  <TableCell className="hidden max-w-80 truncate text-muted-foreground md:table-cell">{t.description || "—"}</TableCell>
                  <TableCell className="hidden text-muted-foreground sm:table-cell">
                    <time dateTime={t.created_at} title={absoluteTime(t.created_at)}>
                      {relativeTime(t.created_at)}
                    </time>
                  </TableCell>
                  <TableCell className="w-10 text-right">
                    <Button
                      variant="ghost"
                      size="icon-sm"
                      aria-label={`Open team ${t.slug}`}
                      onClick={(e) => {
                        e.stopPropagation();
                        setOpen(t.slug);
                      }}
                    >
                      <ChevronRight aria-hidden="true" />
                    </Button>
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        )}
      </TableCard>
      {editable && <CreateTeamDialog open={creating} onOpenChange={setCreating} tenant={slug} onCreated={setOpen} />}
      <TeamSheet tenant={slug} team={open} onClose={() => setOpen("")} editable={editable} />
    </>
  );
}

function CreateTeamDialog({
  open,
  onOpenChange,
  tenant,
  onCreated,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  tenant: string;
  onCreated: (slug: string) => void;
}) {
  const iam = useIamApi();
  const queryClient = useQueryClient();
  const toast = useToast();
  const id = useId();
  const [name, setName] = useState("");
  const [teamSlug, setTeamSlug] = useState("");
  const [slugEdited, setSlugEdited] = useState(false);
  const [description, setDescription] = useState("");
  const [touched, setTouched] = useState(false);

  const create = useMutation({
    mutationFn: () => iam.teams.create(tenant, { slug: teamSlug, name: name.trim(), description: description.trim() }),
    meta: { inlineError: true },
    onSuccess: async (t: Team) => {
      toast.success("Team created", t.slug);
      await queryClient.invalidateQueries({ queryKey: iamKeys.teams(tenant) });
      change(false);
      onCreated(t.slug);
    },
  });

  function change(next: boolean) {
    if (!next) {
      setName("");
      setTeamSlug("");
      setSlugEdited(false);
      setDescription("");
      setTouched(false);
      create.reset();
    }
    onOpenChange(next);
  }

  const errors = {
    name: name.trim() ? [] : ["Required."],
    slug: IAM_SLUG_RE.test(teamSlug) ? [] : [IAM_SLUG_HINT],
  };

  const submit = (e: FormEvent) => {
    e.preventDefault();
    setTouched(true);
    if (errors.name.length || errors.slug.length) return;
    create.mutate();
  };

  return (
    <Dialog open={open} onOpenChange={change}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>New team</DialogTitle>
          <DialogDescription>Teams belong to {tenant} and can be granted roles on its trees.</DialogDescription>
        </DialogHeader>
        <form onSubmit={submit} className="flex flex-col gap-4" noValidate>
          <div className="grid gap-2">
            <Label htmlFor={`${id}-name`}>Name</Label>
            <Input
              id={`${id}-name`}
              value={name}
              aria-invalid={touched && errors.name.length > 0}
              onChange={(e) => {
                setName(e.target.value);
                if (!slugEdited) setTeamSlug(slugify(e.target.value));
              }}
              placeholder="Platform"
            />
            {touched && <FieldErrors messages={errors.name} />}
          </div>
          <div className="grid gap-2">
            <Label htmlFor={`${id}-slug`}>Slug</Label>
            <Input
              id={`${id}-slug`}
              value={teamSlug}
              className="font-mono"
              spellCheck={false}
              aria-invalid={touched && errors.slug.length > 0}
              onChange={(e) => {
                setSlugEdited(true);
                setTeamSlug(e.target.value.toLowerCase());
              }}
              placeholder="platform"
            />
            {touched && <FieldErrors messages={errors.slug} />}
          </div>
          <div className="grid gap-2">
            <Label htmlFor={`${id}-desc`}>Description</Label>
            <Textarea id={`${id}-desc`} value={description} onChange={(e) => setDescription(e.target.value)} rows={2} />
          </div>
          <FormError error={create.error} />
          <DialogFooter>
            <DialogClose asChild>
              <Button type="button" variant="outline">
                Cancel
              </Button>
            </DialogClose>
            <Button type="submit" disabled={create.isPending}>
              {create.isPending ? "Creating…" : "Create team"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

function TeamSheet({ tenant, team, onClose, editable }: { tenant: string; team: string; onClose: () => void; editable: boolean }) {
  const detail = useTeam(tenant, team);
  const d = detail.data;
  return (
    <Sheet open={team !== ""} onOpenChange={(o) => !o && onClose()}>
      <SheetContent className="w-full gap-0 overflow-y-auto sm:max-w-xl">
        <SheetHeader className="border-b">
          <SheetTitle>{d?.team.name || team}</SheetTitle>
          <SheetDescription className="font-mono">{team}</SheetDescription>
        </SheetHeader>
        {detail.isPending ? (
          <TableSkeleton label="Loading team" rows={3} />
        ) : detail.isError ? (
          <QueryError error={detail.error} onRetry={() => void detail.refetch()} title="Could not load team" />
        ) : (
          <div className="flex flex-col gap-6 p-4">
            {editable && <TeamDetailsForm key={detail.data.team.updated_at} tenant={tenant} team={detail.data.team} />}
            <TeamMembers tenant={tenant} team={team} members={detail.data.members} editable={editable} />
            <TeamGrants tenant={tenant} grants={detail.data.grants} />
            {editable && <DeleteTeam tenant={tenant} team={team} onDeleted={onClose} />}
          </div>
        )}
      </SheetContent>
    </Sheet>
  );
}

function TeamDetailsForm({ tenant, team }: { tenant: string; team: Team }) {
  const iam = useIamApi();
  const queryClient = useQueryClient();
  const toast = useToast();
  const id = useId();
  const [name, setName] = useState(team.name);
  const [description, setDescription] = useState(team.description);
  const dirty = name.trim() !== team.name || description.trim() !== team.description;

  const save = useMutation({
    mutationFn: () => iam.teams.update(tenant, team.slug, { name: name.trim(), description: description.trim() }),
    meta: { inlineError: true },
    onSuccess: async () => {
      toast.success("Team updated", team.slug);
      await queryClient.invalidateQueries({ queryKey: iamKeys.teams(tenant) });
    },
  });

  return (
    <section aria-labelledby={`${id}-h`} className="grid gap-3">
      <h3 id={`${id}-h`} className="text-sm font-semibold">
        Details
      </h3>
      <form
        className="grid gap-3"
        onSubmit={(e) => {
          e.preventDefault();
          if (name.trim()) save.mutate();
        }}
      >
        <div className="grid gap-2">
          <Label htmlFor={`${id}-name`}>Name</Label>
          <Input id={`${id}-name`} value={name} onChange={(e) => setName(e.target.value)} />
        </div>
        <div className="grid gap-2">
          <Label htmlFor={`${id}-desc`}>Description</Label>
          <Textarea id={`${id}-desc`} value={description} onChange={(e) => setDescription(e.target.value)} rows={2} />
        </div>
        <FormError error={save.error} />
        <div>
          <Button type="submit" size="sm" variant="outline" disabled={!dirty || !name.trim() || save.isPending}>
            {save.isPending ? "Saving…" : "Save details"}
          </Button>
        </div>
      </form>
    </section>
  );
}

function TeamMembers({
  tenant,
  team,
  members,
  editable,
}: {
  tenant: string;
  team: string;
  members: { account: { id: string; username: string; display_name: string }; role: TeamRole }[];
  editable: boolean;
}) {
  const iam = useIamApi();
  const queryClient = useQueryClient();
  const toast = useToast();
  const id = useId();
  const [username, setUsername] = useState("");
  const [role, setRole] = useState<TeamRole>("member");
  const [touched, setTouched] = useState(false);
  const [removing, setRemoving] = useState("");

  const invalidate = () => queryClient.invalidateQueries({ queryKey: iamKeys.team(tenant, team) });
  const put = useMutation({
    mutationFn: (v: { username: string; role: TeamRole }) => iam.teams.setMember(tenant, team, v.username, v.role),
    meta: { inlineError: true },
    onSuccess: async (_m, v) => {
      toast.success("Team member saved", `${v.username} · ${v.role}`);
      await invalidate();
    },
  });
  const remove = useMutation({
    mutationFn: (u: string) => iam.teams.removeMember(tenant, team, u),
    meta: { inlineError: true },
    onSuccess: async (_r, u) => {
      toast.success("Removed from team", u);
      await invalidate();
    },
  });

  const name = username.trim().toLowerCase();
  const nameErrors = USERNAME_RE.test(name) ? [] : [USERNAME_HINT];

  const add = (e: FormEvent) => {
    e.preventDefault();
    setTouched(true);
    if (nameErrors.length) return;
    put.mutate(
      { username: name, role },
      {
        onSuccess: () => {
          setUsername("");
          setTouched(false);
        },
      },
    );
  };

  return (
    <section aria-labelledby={`${id}-h`} className="grid gap-3">
      <h3 id={`${id}-h`} className="flex items-center gap-2 text-sm font-semibold">
        Members <Badge variant="secondary">{members.length}</Badge>
      </h3>
      {editable && (
        <form onSubmit={add} className="grid gap-2 rounded-md border p-3" noValidate aria-label="Add team member">
          <div className="flex flex-col gap-2 sm:flex-row">
            <div className="grid flex-1 gap-1.5">
              <Label htmlFor={`${id}-user`} className="sr-only">
                Username
              </Label>
              <Input
                id={`${id}-user`}
                value={username}
                onChange={(e) => setUsername(e.target.value)}
                placeholder="username"
                className="font-mono"
                autoComplete="off"
                spellCheck={false}
                aria-invalid={touched && nameErrors.length > 0}
              />
            </div>
            <Select value={role} onValueChange={(v) => setRole(v as TeamRole)}>
              <SelectTrigger className="w-full capitalize sm:w-36" aria-label="Team role">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {TEAM_ROLES.map((r) => (
                  <SelectItem key={r} value={r} className="capitalize">
                    {r}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <Button type="submit" disabled={put.isPending}>
              <UserPlus aria-hidden="true" />
              Add
            </Button>
          </div>
          {touched && <FieldErrors messages={nameErrors} />}
          <p className="text-xs text-muted-foreground">Maintainers can manage this team's membership.</p>
        </form>
      )}
      <FormError error={put.error ?? remove.error} />
      {members.length === 0 ? (
        <p className="text-sm text-muted-foreground">No members yet.</p>
      ) : (
        <Table aria-label="Team members">
          <TableHeader>
            <TableRow className="hover:bg-transparent">
              <TableHead>Account</TableHead>
              <TableHead>Role</TableHead>
              {editable && (
                <TableHead>
                  <span className="sr-only">Actions</span>
                </TableHead>
              )}
            </TableRow>
          </TableHeader>
          <TableBody>
            {members.map((m) => (
              <TableRow key={m.account.id}>
                <TableCell>
                  <div className="flex items-center gap-2">
                    <UserAvatar name={m.account.display_name || m.account.username} />
                    <span className="font-medium">{m.account.username}</span>
                  </div>
                </TableCell>
                <TableCell>
                  {editable ? (
                    <Select value={m.role} onValueChange={(r) => put.mutate({ username: m.account.username, role: r as TeamRole })}>
                      <SelectTrigger size="sm" className="w-32 capitalize" aria-label={`Team role of ${m.account.username}`}>
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        {TEAM_ROLES.map((r) => (
                          <SelectItem key={r} value={r} className="capitalize">
                            {r}
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                  ) : (
                    <Badge variant={m.role === "maintainer" ? "default" : "outline"} className="capitalize">
                      {m.role}
                    </Badge>
                  )}
                </TableCell>
                {editable && (
                  <TableCell className="w-10 text-right">
                    <Button variant="ghost" size="icon-sm" aria-label={`Remove ${m.account.username} from team`} onClick={() => setRemoving(m.account.username)}>
                      <UserMinus aria-hidden="true" />
                    </Button>
                  </TableCell>
                )}
              </TableRow>
            ))}
          </TableBody>
        </Table>
      )}
      <ConfirmAction
        open={removing !== ""}
        onOpenChange={(o) => !o && setRemoving("")}
        title={`Remove ${removing} from ${team}?`}
        description="They lose any access granted through this team."
        confirmLabel="Remove member"
        onConfirm={() => remove.mutateAsync(removing)}
      />
    </section>
  );
}

function TeamGrants({ tenant, grants }: { tenant: string; grants: { tree_id: string; role: string }[] }) {
  const trees = useTenantTrees(tenant);
  const id = useId();
  const byId = new Map((trees.data ?? []).map((t) => [t.tree.id, t.tree.name]));
  return (
    <section aria-labelledby={`${id}-h`} className="grid gap-3">
      <Separator />
      <h3 id={`${id}-h`} className="flex items-center gap-2 text-sm font-semibold">
        Tree grants <Badge variant="secondary">{grants.length}</Badge>
      </h3>
      {grants.length === 0 ? (
        <p className="text-sm text-muted-foreground">This team has no tree grants. Add one from a tree's Access tab.</p>
      ) : (
        <Table aria-label="Team grants">
          <TableHeader>
            <TableRow className="hover:bg-transparent">
              <TableHead>Tree</TableHead>
              <TableHead>Role</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {grants.map((g) => {
              const name = byId.get(g.tree_id);
              return (
                <TableRow key={g.tree_id}>
                  <TableCell className="font-mono">
                    {name ? (
                      <Link className="underline-offset-4 hover:underline" to={treePath(tenant, name, "access")}>
                        {tenant}/{name}
                      </Link>
                    ) : (
                      g.tree_id
                    )}
                  </TableCell>
                  <TableCell>
                    <IamRoleBadge role={g.role} />
                  </TableCell>
                </TableRow>
              );
            })}
          </TableBody>
        </Table>
      )}
    </section>
  );
}

function DeleteTeam({ tenant, team, onDeleted }: { tenant: string; team: string; onDeleted: () => void }) {
  const iam = useIamApi();
  const queryClient = useQueryClient();
  const toast = useToast();
  const [open, setOpen] = useState(false);
  const remove = useMutation({
    mutationFn: () => iam.teams.remove(tenant, team),
    meta: { inlineError: true },
    onSuccess: async () => {
      toast.success("Team deleted", team);
      onDeleted();
      await queryClient.invalidateQueries({ queryKey: iamKeys.teams(tenant) });
    },
  });
  return (
    <section className="grid gap-3">
      <Separator />
      <div className="flex flex-wrap items-center justify-between gap-2">
        <p className="text-sm text-muted-foreground">Deleting a team removes its members and tree grants.</p>
        <Button variant="outline" size="sm" onClick={() => setOpen(true)}>
          <Trash2 aria-hidden="true" />
          Delete team
        </Button>
      </div>
      <ConfirmAction
        open={open}
        onOpenChange={setOpen}
        title={`Delete team ${team}?`}
        description="Members keep their tenant roles but lose access granted through this team. This cannot be undone."
        confirmLabel="Delete team"
        onConfirm={() => remove.mutateAsync()}
      />
    </section>
  );
}
