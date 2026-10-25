import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Ban, Bot, ChevronRight, KeyRound, Plus, Trash2, TriangleAlert } from "lucide-react";
import { useId, useState, type FormEvent } from "react";
import {
  iamKeys,
  useIamApi,
  useServiceAccount,
  useServiceAccounts,
  type CreatedKey,
  type IamToken,
  type ServiceAccount,
} from "@/api/iam";
import { ChipsInput } from "@/components/files/ChipsInput";
import { CopyButton } from "@/components/people/CopyButton";
import { EmptyState, FormError, QueryError, TableSkeleton } from "@/components/people/states";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Dialog, DialogClose, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Separator } from "@/components/ui/separator";
import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from "@/components/ui/sheet";
import { Switch } from "@/components/ui/switch";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { Textarea } from "@/components/ui/textarea";
import { absoluteTime, relativeTime } from "@/lib/format";
import { useToast } from "@/toast/store";
import { ConfirmAction, FieldErrors, IamRoleBadge, RoleSelect, TableCard } from "./shared";
import { IAM_SLUG_HINT, IAM_SLUG_RE, SCOPE_EXAMPLES } from "./util";

interface ServiceAccountsTabProps {
  slug: string;
  editable: boolean;
}

function StatusBadge({ disabled }: { disabled: boolean }) {
  return disabled ? (
    <Badge variant="secondary" className="line-through">
      <Ban aria-hidden="true" />
      Disabled
    </Badge>
  ) : (
    <Badge variant="outline">Active</Badge>
  );
}

export function ServiceAccountsTab({ slug, editable }: ServiceAccountsTabProps) {
  const sas = useServiceAccounts(slug);
  const [creating, setCreating] = useState(false);
  const [open, setOpen] = useState("");

  return (
    <>
      <TableCard
        description="Service accounts are non-human identities for CI and automation. They authenticate with keys."
        actions={
          editable && (
            <Button size="sm" onClick={() => setCreating(true)}>
              <Plus aria-hidden="true" />
              New service account
            </Button>
          )
        }
      >
        {sas.isPending ? (
          <TableSkeleton label="Loading service accounts" rows={3} />
        ) : sas.isError ? (
          <QueryError error={sas.error} onRetry={() => void sas.refetch()} title="Could not load service accounts" />
        ) : sas.data.length === 0 ? (
          <EmptyState icon={Bot} title="No service accounts">
            Create one for CI pipelines or bots instead of sharing a personal token.
          </EmptyState>
        ) : (
          <Table aria-label="Service accounts">
            <TableHeader className="sticky top-0 z-10 bg-background">
              <TableRow className="hover:bg-transparent">
                <TableHead>Name</TableHead>
                <TableHead>Role</TableHead>
                <TableHead>Status</TableHead>
                <TableHead className="hidden md:table-cell">Created</TableHead>
                <TableHead>
                  <span className="sr-only">Open</span>
                </TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {sas.data.map((sa) => (
                <TableRow key={sa.id} className="cursor-pointer" onClick={() => setOpen(sa.name)}>
                  <TableCell>
                    <div className="flex items-center gap-3">
                      <span className="flex size-8 items-center justify-center rounded-md border bg-muted">
                        <Bot aria-hidden="true" className="size-4" />
                      </span>
                      <div className="min-w-0">
                        <p className={sa.disabled ? "font-mono font-medium line-through" : "font-mono font-medium"}>{sa.name}</p>
                        {sa.description && <p className="max-w-72 truncate text-xs text-muted-foreground">{sa.description}</p>}
                      </div>
                    </div>
                  </TableCell>
                  <TableCell>
                    <IamRoleBadge role={sa.role} />
                  </TableCell>
                  <TableCell>
                    <StatusBadge disabled={sa.disabled} />
                  </TableCell>
                  <TableCell className="hidden text-muted-foreground md:table-cell">
                    <time dateTime={sa.created_at} title={absoluteTime(sa.created_at)}>
                      {relativeTime(sa.created_at)}
                    </time>
                    {sa.created_by && <span> by {sa.created_by}</span>}
                  </TableCell>
                  <TableCell className="w-10 text-right">
                    <Button
                      variant="ghost"
                      size="icon-sm"
                      aria-label={`Open service account ${sa.name}`}
                      onClick={(e) => {
                        e.stopPropagation();
                        setOpen(sa.name);
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
      {editable && <CreateServiceAccountDialog open={creating} onOpenChange={setCreating} tenant={slug} onCreated={setOpen} />}
      <ServiceAccountSheet tenant={slug} name={open} onClose={() => setOpen("")} editable={editable} />
    </>
  );
}

function CreateServiceAccountDialog({
  open,
  onOpenChange,
  tenant,
  onCreated,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  tenant: string;
  onCreated: (name: string) => void;
}) {
  const iam = useIamApi();
  const queryClient = useQueryClient();
  const toast = useToast();
  const id = useId();
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [role, setRole] = useState("developer");
  const [touched, setTouched] = useState(false);

  const create = useMutation({
    mutationFn: () => iam.serviceAccounts.create(tenant, { name, description: description.trim(), role }),
    meta: { inlineError: true },
    onSuccess: async (sa: ServiceAccount) => {
      toast.success("Service account created", sa.name);
      await queryClient.invalidateQueries({ queryKey: iamKeys.serviceAccounts(tenant) });
      change(false);
      onCreated(sa.name);
    },
  });

  function change(next: boolean) {
    if (!next) {
      setName("");
      setDescription("");
      setRole("developer");
      setTouched(false);
      create.reset();
    }
    onOpenChange(next);
  }

  const nameErrors = IAM_SLUG_RE.test(name) ? [] : [IAM_SLUG_HINT];
  const submit = (e: FormEvent) => {
    e.preventDefault();
    setTouched(true);
    if (nameErrors.length) return;
    create.mutate();
  };

  return (
    <Dialog open={open} onOpenChange={change}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>New service account</DialogTitle>
          <DialogDescription>It acts with the chosen role inside {tenant}, plus any tree grants.</DialogDescription>
        </DialogHeader>
        <form onSubmit={submit} className="flex flex-col gap-4" noValidate>
          <div className="grid gap-2">
            <Label htmlFor={`${id}-name`}>Name</Label>
            <Input
              id={`${id}-name`}
              value={name}
              onChange={(e) => setName(e.target.value.toLowerCase())}
              className="font-mono"
              placeholder="ci-deployer"
              spellCheck={false}
              autoComplete="off"
              aria-invalid={touched && nameErrors.length > 0}
            />
            {touched && <FieldErrors messages={nameErrors} />}
          </div>
          <div className="grid gap-2">
            <Label htmlFor={`${id}-desc`}>Description</Label>
            <Textarea id={`${id}-desc`} value={description} onChange={(e) => setDescription(e.target.value)} rows={2} />
          </div>
          <div className="grid gap-2">
            <Label htmlFor={`${id}-role`}>Role</Label>
            <RoleSelect id={`${id}-role`} tenant={tenant} value={role} onChange={setRole} disabledRoles={["owner"]} />
          </div>
          <FormError error={create.error} />
          <DialogFooter>
            <DialogClose asChild>
              <Button type="button" variant="outline">
                Cancel
              </Button>
            </DialogClose>
            <Button type="submit" disabled={create.isPending}>
              {create.isPending ? "Creating…" : "Create service account"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

function ServiceAccountSheet({ tenant, name, onClose, editable }: { tenant: string; name: string; onClose: () => void; editable: boolean }) {
  const detail = useServiceAccount(tenant, name);
  return (
    <Sheet open={name !== ""} onOpenChange={(o) => !o && onClose()}>
      <SheetContent className="w-full gap-0 overflow-y-auto sm:max-w-xl">
        <SheetHeader className="border-b">
          <SheetTitle className="flex items-center gap-2 font-mono">
            <Bot aria-hidden="true" className="size-4" />
            {name}
          </SheetTitle>
          <SheetDescription>Service account in {tenant}</SheetDescription>
        </SheetHeader>
        {detail.isPending ? (
          <TableSkeleton label="Loading service account" rows={3} />
        ) : detail.isError ? (
          <QueryError error={detail.error} onRetry={() => void detail.refetch()} title="Could not load service account" />
        ) : (
          <div className="flex flex-col gap-6 p-4">
            <SettingsSection key={detail.data.service_account.updated_at} tenant={tenant} sa={detail.data.service_account} editable={editable} />
            <KeysSection tenant={tenant} sa={detail.data.service_account} keys={detail.data.keys} editable={editable} />
            {editable && <DeleteServiceAccount tenant={tenant} name={name} onDeleted={onClose} />}
          </div>
        )}
      </SheetContent>
    </Sheet>
  );
}

function SettingsSection({ tenant, sa, editable }: { tenant: string; sa: ServiceAccount; editable: boolean }) {
  const iam = useIamApi();
  const queryClient = useQueryClient();
  const toast = useToast();
  const id = useId();
  const [description, setDescription] = useState(sa.description);

  const patch = useMutation({
    mutationFn: (body: { description?: string; role?: string; disabled?: boolean }) => iam.serviceAccounts.update(tenant, sa.name, body),
    meta: { inlineError: true },
    onSuccess: async (next) => {
      toast.success("Service account updated", next.name);
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: iamKeys.serviceAccount(tenant, sa.name) }),
        queryClient.invalidateQueries({ queryKey: iamKeys.serviceAccounts(tenant), exact: true }),
      ]);
    },
  });

  return (
    <section className="grid gap-4" aria-labelledby={`${id}-h`}>
      <h3 id={`${id}-h`} className="text-sm font-semibold">
        Settings
      </h3>
      <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-sm">
        <dt className="text-muted-foreground">Created</dt>
        <dd>
          <time dateTime={sa.created_at} title={absoluteTime(sa.created_at)}>
            {relativeTime(sa.created_at)}
          </time>
          {sa.created_by && ` by ${sa.created_by}`}
        </dd>
        <dt className="text-muted-foreground">Status</dt>
        <dd>
          <StatusBadge disabled={sa.disabled} />
        </dd>
      </dl>
      {editable ? (
        <>
          <div className="flex items-start justify-between gap-4 rounded-md border p-3">
            <div className="grid gap-1">
              <Label htmlFor={`${id}-disabled`}>Disabled</Label>
              <p id={`${id}-disabled-desc`} className="text-xs text-muted-foreground">
                A disabled service account is denied everything; its keys stop working.
              </p>
            </div>
            <Switch
              id={`${id}-disabled`}
              checked={sa.disabled}
              disabled={patch.isPending}
              onCheckedChange={(disabled) => patch.mutate({ disabled })}
              aria-describedby={`${id}-disabled-desc`}
            />
          </div>
          <div className="grid gap-2">
            <Label htmlFor={`${id}-role`}>Role</Label>
            <RoleSelect
              id={`${id}-role`}
              tenant={tenant}
              value={sa.role}
              onChange={(role) => patch.mutate({ role })}
              disabled={patch.isPending}
              disabledRoles={["owner"]}
            />
          </div>
          <form
            className="grid gap-2"
            onSubmit={(e) => {
              e.preventDefault();
              patch.mutate({ description: description.trim() });
            }}
          >
            <Label htmlFor={`${id}-desc`}>Description</Label>
            <Textarea id={`${id}-desc`} value={description} onChange={(e) => setDescription(e.target.value)} rows={2} />
            <div>
              <Button type="submit" size="sm" variant="outline" disabled={description.trim() === sa.description || patch.isPending}>
                Save description
              </Button>
            </div>
          </form>
          <FormError error={patch.error} />
        </>
      ) : (
        <div className="grid gap-1 text-sm">
          <span className="text-muted-foreground">Role</span>
          <IamRoleBadge role={sa.role} />
          {sa.description && <p className="mt-2">{sa.description}</p>}
        </div>
      )}
    </section>
  );
}

function KeysSection({ tenant, sa, keys, editable }: { tenant: string; sa: ServiceAccount; keys: IamToken[]; editable: boolean }) {
  const iam = useIamApi();
  const queryClient = useQueryClient();
  const toast = useToast();
  const id = useId();
  const [creating, setCreating] = useState(false);
  const [revoking, setRevoking] = useState<IamToken | null>(null);
  const revoke = useMutation({
    mutationFn: (k: IamToken) => iam.serviceAccounts.revokeKey(tenant, sa.name, k.id),
    meta: { inlineError: true },
    onSuccess: async (_r, k) => {
      toast.success("Key revoked", k.name);
      await queryClient.invalidateQueries({ queryKey: iamKeys.serviceAccount(tenant, sa.name) });
    },
  });

  return (
    <section className="grid gap-3" aria-labelledby={`${id}-h`}>
      <Separator />
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h3 id={`${id}-h`} className="flex items-center gap-2 text-sm font-semibold">
          Keys <Badge variant="secondary">{keys.length}</Badge>
        </h3>
        {editable && (
          <Button size="sm" onClick={() => setCreating(true)}>
            <KeyRound aria-hidden="true" />
            Create key
          </Button>
        )}
      </div>
      {keys.length === 0 ? (
        <p className="text-sm text-muted-foreground">No keys. Create one to authenticate as {sa.name}.</p>
      ) : (
        <Table aria-label="Keys">
          <TableHeader>
            <TableRow className="hover:bg-transparent">
              <TableHead>Key</TableHead>
              <TableHead>Scopes</TableHead>
              <TableHead className="hidden sm:table-cell">Expires</TableHead>
              {editable && (
                <TableHead>
                  <span className="sr-only">Actions</span>
                </TableHead>
              )}
            </TableRow>
          </TableHeader>
          <TableBody>
            {keys.map((k) => (
              <TableRow key={k.id}>
                <TableCell>
                  <p className="font-medium">{k.name}</p>
                  <p className="text-xs text-muted-foreground">
                    {k.last_used_at ? `used ${relativeTime(k.last_used_at)}` : "never used"}
                    {k.last_ip && ` · ${k.last_ip}`}
                  </p>
                </TableCell>
                <TableCell className="whitespace-normal">
                  {k.scopes?.length ? (
                    <ul aria-label={`Scopes of ${k.name}`} className="flex flex-wrap gap-1">
                      {k.scopes.map((s) => (
                        <li key={s}>
                          <Badge variant="secondary" className="font-mono">
                            {s}
                          </Badge>
                        </li>
                      ))}
                    </ul>
                  ) : (
                    <span className="text-xs text-muted-foreground">Full role</span>
                  )}
                </TableCell>
                <TableCell className="hidden text-muted-foreground sm:table-cell">
                  {k.expires_at ? (
                    <time dateTime={k.expires_at} title={absoluteTime(k.expires_at)}>
                      {relativeTime(k.expires_at)}
                    </time>
                  ) : (
                    "never"
                  )}
                </TableCell>
                {editable && (
                  <TableCell className="w-10 text-right">
                    <Button variant="ghost" size="icon-sm" aria-label={`Revoke key ${k.name}`} onClick={() => setRevoking(k)}>
                      <Trash2 aria-hidden="true" />
                    </Button>
                  </TableCell>
                )}
              </TableRow>
            ))}
          </TableBody>
        </Table>
      )}
      {editable && <CreateKeyDialog open={creating} onOpenChange={setCreating} tenant={tenant} sa={sa.name} />}
      <ConfirmAction
        open={revoking !== null}
        onOpenChange={(o) => !o && setRevoking(null)}
        title={`Revoke key ${revoking?.name ?? ""}?`}
        description="Anything using this key stops authenticating immediately."
        confirmLabel="Revoke key"
        onConfirm={() => (revoking ? revoke.mutateAsync(revoking) : Promise.resolve())}
      />
    </section>
  );
}

const TTL_PRESETS: { label: string; hours: number }[] = [
  { label: "1 day", hours: 24 },
  { label: "30 days", hours: 720 },
  { label: "90 days", hours: 2160 },
  { label: "1 year", hours: 8760 },
  { label: "Never", hours: 0 },
];

function CreateKeyDialog({ open, onOpenChange, tenant, sa }: { open: boolean; onOpenChange: (open: boolean) => void; tenant: string; sa: string }) {
  const iam = useIamApi();
  const queryClient = useQueryClient();
  const id = useId();
  const [name, setName] = useState("");
  const [ttl, setTtl] = useState("720");
  const [scopes, setScopes] = useState<string[]>([]);
  const [touched, setTouched] = useState(false);
  const [created, setCreated] = useState<CreatedKey | null>(null);

  const create = useMutation({
    mutationFn: () => iam.serviceAccounts.createKey(tenant, sa, { name: name.trim(), ttl_hours: Number(ttl), scopes }),
    meta: { inlineError: true },
    onSuccess: async (k) => {
      setCreated(k);
      await queryClient.invalidateQueries({ queryKey: iamKeys.serviceAccount(tenant, sa) });
    },
  });

  function change(next: boolean) {
    if (!next) {
      setName("");
      setTtl("720");
      setScopes([]);
      setTouched(false);
      setCreated(null);
      create.reset();
    }
    onOpenChange(next);
  }

  const ttlNum = Number(ttl);
  const errors = {
    name: name.trim() ? [] : ["Required."],
    ttl: ttl.trim() !== "" && Number.isInteger(ttlNum) && ttlNum >= 0 && ttlNum <= 87600 ? [] : ["Whole hours from 0 (never expires) to 87600."],
  };

  const submit = (e: FormEvent) => {
    e.preventDefault();
    setTouched(true);
    if (errors.name.length || errors.ttl.length) return;
    create.mutate();
  };

  return (
    <Dialog open={open} onOpenChange={change}>
      <DialogContent>
        {created ? (
          <>
            <DialogHeader>
              <DialogTitle>Key created</DialogTitle>
              <DialogDescription>
                Key <span className="font-medium text-foreground">{created.api_token.name}</span> for <span className="font-mono">{sa}</span>.
              </DialogDescription>
            </DialogHeader>
            <Alert>
              <TriangleAlert aria-hidden="true" />
              <AlertTitle>Copy it now</AlertTitle>
              <AlertDescription>This is the only time the key is shown. Store it in your CI secret store.</AlertDescription>
            </Alert>
            <div className="flex items-center gap-2">
              <Label htmlFor={`${id}-token`} className="sr-only">
                Key
              </Label>
              <Input id={`${id}-token`} readOnly value={created.token} className="font-mono text-xs" onFocus={(e) => e.target.select()} />
              <CopyButton value={created.token} label="Copy key" copiedMessage="Key copied" />
            </div>
            <DialogFooter>
              <Button onClick={() => change(false)}>Done</Button>
            </DialogFooter>
          </>
        ) : (
          <>
            <DialogHeader>
              <DialogTitle>Create key</DialogTitle>
              <DialogDescription>
                A bearer key for <span className="font-mono">{sa}</span>. Scopes narrow what it may do; none means the full role.
              </DialogDescription>
            </DialogHeader>
            <form onSubmit={submit} className="flex flex-col gap-4" noValidate>
              <div className="grid gap-2">
                <Label htmlFor={`${id}-name`}>Key name</Label>
                <Input
                  id={`${id}-name`}
                  value={name}
                  onChange={(e) => setName(e.target.value)}
                  placeholder="github-actions"
                  aria-invalid={touched && errors.name.length > 0}
                />
                {touched && <FieldErrors messages={errors.name} />}
              </div>
              <div className="grid gap-2">
                <Label htmlFor={`${id}-ttl`}>Expires after (hours)</Label>
                <Input
                  id={`${id}-ttl`}
                  type="number"
                  min={0}
                  inputMode="numeric"
                  value={ttl}
                  onChange={(e) => setTtl(e.target.value)}
                  aria-invalid={touched && errors.ttl.length > 0}
                  aria-describedby={`${id}-ttl-presets`}
                />
                <div id={`${id}-ttl-presets`} className="flex flex-wrap gap-1">
                  {TTL_PRESETS.map((p) => (
                    <Button
                      key={p.label}
                      type="button"
                      size="xs"
                      variant={ttl === String(p.hours) ? "default" : "outline"}
                      onClick={() => setTtl(String(p.hours))}
                    >
                      {p.label}
                    </Button>
                  ))}
                </div>
                {touched && <FieldErrors messages={errors.ttl} />}
              </div>
              <div className="grid gap-2">
                <Label htmlFor={`${id}-scopes`}>Scopes</Label>
                <ChipsInput id={`${id}-scopes`} values={scopes} onChange={setScopes} placeholder="tree:read" describedBy={`${id}-scopes-hint`} />
                <p id={`${id}-scopes-hint`} className="flex flex-wrap items-center gap-1 text-xs text-muted-foreground">
                  Examples:
                  {SCOPE_EXAMPLES.map((s) => (
                    <Button
                      key={s}
                      type="button"
                      variant="ghost"
                      size="xs"
                      className="h-5 px-1.5 font-mono text-[11px] text-muted-foreground"
                      disabled={scopes.includes(s)}
                      onClick={() => setScopes([...scopes, s])}
                      aria-label={`Add scope ${s}`}
                    >
                      + {s}
                    </Button>
                  ))}
                </p>
              </div>
              <FormError error={create.error} />
              <DialogFooter>
                <DialogClose asChild>
                  <Button type="button" variant="outline">
                    Cancel
                  </Button>
                </DialogClose>
                <Button type="submit" disabled={create.isPending}>
                  {create.isPending ? "Creating…" : "Create key"}
                </Button>
              </DialogFooter>
            </form>
          </>
        )}
      </DialogContent>
    </Dialog>
  );
}

function DeleteServiceAccount({ tenant, name, onDeleted }: { tenant: string; name: string; onDeleted: () => void }) {
  const iam = useIamApi();
  const queryClient = useQueryClient();
  const toast = useToast();
  const [open, setOpen] = useState(false);
  const remove = useMutation({
    mutationFn: () => iam.serviceAccounts.remove(tenant, name),
    meta: { inlineError: true },
    onSuccess: async () => {
      toast.success("Service account deleted", name);
      onDeleted();
      await queryClient.invalidateQueries({ queryKey: iamKeys.serviceAccounts(tenant) });
    },
  });
  return (
    <section className="grid gap-3">
      <Separator />
      <div className="flex flex-wrap items-center justify-between gap-2">
        <p className="text-sm text-muted-foreground">Deleting removes all keys and tree grants.</p>
        <Button variant="outline" size="sm" onClick={() => setOpen(true)}>
          <Trash2 aria-hidden="true" />
          Delete service account
        </Button>
      </div>
      <ConfirmAction
        open={open}
        onOpenChange={setOpen}
        title={`Delete ${name}?`}
        description="Its keys stop working immediately. This cannot be undone."
        confirmLabel="Delete service account"
        onConfirm={() => remove.mutateAsync()}
      />
    </section>
  );
}
