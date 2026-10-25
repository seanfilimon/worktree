import { useMutation, useQueryClient } from "@tanstack/react-query";
import { KeyRound, Plus, Trash2 } from "lucide-react";
import { useId, useState, type FormEvent } from "react";
import { useApi } from "@/api/context";
import {
  securityKeys,
  useIamActions,
  useSecureTokens,
  useSecurityApi,
  type CreatedScopedToken,
  type SecureToken,
} from "@/api/security";
import { ConfirmDialog } from "@/components/people/ConfirmDialog";
import { CopyButton } from "@/components/people/CopyButton";
import { EmptyState, FormError, QueryError, TableSkeleton } from "@/components/people/states";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardAction, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { absoluteTime, relativeTime } from "@/lib/format";
import { useToast } from "@/toast/store";
import { FALLBACK_ACTIONS, groupActions, normalizeScopes } from "./scopes";

const TTL_OPTIONS = [
  { hours: 24, label: "1 day" },
  { hours: 24 * 7, label: "7 days" },
  { hours: 24 * 30, label: "30 days" },
  { hours: 24 * 90, label: "90 days" },
  { hours: 24 * 365, label: "1 year" },
  { hours: 0, label: "Never" },
];

function When({ iso, empty = "never" }: { iso: string | undefined; empty?: string }) {
  if (!iso) return <span className="text-muted-foreground">{empty}</span>;
  return (
    <time dateTime={iso} title={absoluteTime(iso)}>
      {relativeTime(iso)}
    </time>
  );
}

interface ScopePickerProps {
  selected: Set<string>;
  onChange: (next: Set<string>) => void;
  invalid: boolean;
  describedBy?: string;
}

/** Checklist of actions grouped by area, with an `area:*` toggle per area. */
function ScopePicker({ selected, onChange, invalid, describedBy }: ScopePickerProps) {
  const id = useId();
  const actions = useIamActions();
  const areas = groupActions(actions.data && actions.data.length > 0 ? actions.data : FALLBACK_ACTIONS);

  const toggle = (scope: string, on: boolean) => {
    const next = new Set(selected);
    if (on) next.add(scope);
    else next.delete(scope);
    onChange(next);
  };

  return (
    <fieldset
      aria-label="Token scopes"
      aria-invalid={invalid ? true : undefined}
      aria-describedby={describedBy}
      className="rounded-md border"
    >
      <ScrollArea className="h-56">
        <div className="flex flex-col divide-y">
          {areas.map(({ area, actions: list }) => {
            const wildcard = `${area}:*`;
            const all = selected.has(wildcard);
            return (
              <div key={area} className="flex flex-col gap-2 p-3">
                <div className="flex items-center gap-2">
                  <Checkbox id={`${id}-${area}`} checked={all} onCheckedChange={(v) => toggle(wildcard, v === true)} />
                  <Label htmlFor={`${id}-${area}`} className="font-mono text-xs font-semibold">
                    {wildcard}
                  </Label>
                  <span className="text-xs text-muted-foreground">every {area} action</span>
                </div>
                <div className="grid gap-1.5 pl-6 sm:grid-cols-2">
                  {list.map((a) => (
                    <div key={a.action} className="flex items-start gap-2">
                      <Checkbox
                        id={`${id}-${a.action}`}
                        checked={all || selected.has(a.action)}
                        disabled={all}
                        onCheckedChange={(v) => toggle(a.action, v === true)}
                        aria-describedby={a.description ? `${id}-${a.action}-d` : undefined}
                        className="mt-0.5"
                      />
                      <div className="grid gap-0.5">
                        <Label htmlFor={`${id}-${a.action}`} className="font-mono text-xs">
                          {a.action}
                        </Label>
                        {a.description && (
                          <span id={`${id}-${a.action}-d`} className="text-xs text-muted-foreground">
                            {a.description}
                          </span>
                        )}
                      </div>
                    </div>
                  ))}
                </div>
              </div>
            );
          })}
        </div>
      </ScrollArea>
    </fieldset>
  );
}

function CreateTokenDialog({ open, onOpenChange }: { open: boolean; onOpenChange: (open: boolean) => void }) {
  const sec = useSecurityApi();
  const queryClient = useQueryClient();
  const [name, setName] = useState("");
  const [ttl, setTtl] = useState(String(24 * 30));
  const [access, setAccess] = useState<"full" | "scoped">("full");
  const [scopes, setScopes] = useState<Set<string>>(new Set());
  const [submitted, setSubmitted] = useState(false);
  const [created, setCreated] = useState<CreatedScopedToken | null>(null);
  const nameError =
    name.trim() === "" ? "Give the token a name." : name.trim().length > 100 ? "At most 100 characters." : undefined;
  const normalized = normalizeScopes(scopes);
  const scopeError = access === "scoped" && normalized.length === 0 ? "Pick at least one scope." : undefined;

  const create = useMutation({
    mutationFn: sec.tokens.create,
    meta: { inlineError: true },
    onSuccess: async (res) => {
      setCreated(res);
      await queryClient.invalidateQueries({ queryKey: securityKeys.tokens });
    },
  });

  const changeOpen = (next: boolean) => {
    if (!next) {
      // The raw token is dropped here: it can never be shown again.
      setCreated(null);
      setName("");
      setTtl(String(24 * 30));
      setAccess("full");
      setScopes(new Set());
      setSubmitted(false);
      create.reset();
    }
    onOpenChange(next);
  };

  const onSubmit = (e: FormEvent) => {
    e.preventDefault();
    setSubmitted(true);
    if (nameError || scopeError) return;
    create.mutate({
      name: name.trim(),
      ttl_hours: Number(ttl),
      ...(access === "scoped" ? { scopes: normalized } : {}),
    });
  };

  return (
    <Dialog open={open} onOpenChange={changeOpen}>
      <DialogContent className="sm:max-w-xl">
        {created ? (
          <>
            <DialogHeader>
              <DialogTitle>Token “{created.api_token.name}” created</DialogTitle>
              <DialogDescription>Copy it now. For your security it will not be shown again.</DialogDescription>
            </DialogHeader>
            <div className="flex gap-2">
              <Input
                readOnly
                aria-label="New API token"
                value={created.token}
                onFocus={(e) => e.currentTarget.select()}
                className="font-mono text-xs"
              />
              <CopyButton value={created.token} label="Copy token" copiedMessage="Token copied to clipboard" />
            </div>
            {created.api_token.scopes && created.api_token.scopes.length > 0 && (
              <p className="text-xs text-muted-foreground">
                Limited to <span className="font-mono">{created.api_token.scopes.join(", ")}</span>.
              </p>
            )}
            <DialogFooter>
              <DialogClose asChild>
                <Button>Done</Button>
              </DialogClose>
            </DialogFooter>
          </>
        ) : (
          <form onSubmit={onSubmit} noValidate className="flex min-w-0 flex-col gap-4">
            <DialogHeader>
              <DialogTitle>New API token</DialogTitle>
              <DialogDescription>
                For the wt CLI, CI and scripts. It never exceeds your own permissions.
              </DialogDescription>
            </DialogHeader>
            <div className="grid gap-2">
              <Label htmlFor="token-name">Token name</Label>
              <Input
                id="token-name"
                value={name}
                placeholder="ci-deploy"
                autoComplete="off"
                aria-invalid={submitted && nameError ? true : undefined}
                aria-describedby={submitted && nameError ? "token-name-error" : undefined}
                onChange={(e) => setName(e.target.value)}
              />
              {submitted && nameError && (
                <p id="token-name-error" className="text-sm font-medium">
                  {nameError}
                </p>
              )}
            </div>
            <div className="grid gap-2">
              <Label htmlFor="token-ttl">Expires</Label>
              <Select value={ttl} onValueChange={setTtl}>
                <SelectTrigger id="token-ttl" className="w-full">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {TTL_OPTIONS.map((o) => (
                    <SelectItem key={o.hours} value={String(o.hours)}>
                      {o.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <div className="grid gap-2">
              <span id="token-access-label" className="text-sm font-medium">
                Access
              </span>
              <RadioGroup
                aria-labelledby="token-access-label"
                value={access}
                onValueChange={(v) => setAccess(v === "scoped" ? "scoped" : "full")}
                className="grid gap-2 sm:grid-cols-2"
              >
                <Label className="flex items-start gap-2 rounded-md border p-3 font-normal has-[[data-state=checked]]:border-foreground">
                  <RadioGroupItem value="full" className="mt-0.5" />
                  <span className="grid gap-0.5">
                    <span className="font-medium">Full access</span>
                    <span className="text-xs text-muted-foreground">Everything your account can do.</span>
                  </span>
                </Label>
                <Label className="flex items-start gap-2 rounded-md border p-3 font-normal has-[[data-state=checked]]:border-foreground">
                  <RadioGroupItem value="scoped" className="mt-0.5" />
                  <span className="grid gap-0.5">
                    <span className="font-medium">Selected scopes</span>
                    <span className="text-xs text-muted-foreground">Only the actions you pick.</span>
                  </span>
                </Label>
              </RadioGroup>
            </div>
            {access === "scoped" && (
              <div className="grid gap-2">
                <ScopePicker
                  selected={scopes}
                  onChange={setScopes}
                  invalid={submitted && scopeError !== undefined}
                  describedBy="token-scope-note"
                />
                <p
                  id="token-scope-note"
                  className={submitted && scopeError ? "text-sm font-medium" : "text-xs text-muted-foreground"}
                >
                  {submitted && scopeError
                    ? scopeError
                    : normalized.length > 0
                      ? `Scopes: ${normalized.join(", ")}`
                      : "Requests outside these scopes are denied, even if your role allows them."}
                </p>
              </div>
            )}
            <FormError error={create.error} />
            <DialogFooter>
              <DialogClose asChild>
                <Button type="button" variant="outline">
                  Cancel
                </Button>
              </DialogClose>
              <Button type="submit" disabled={create.isPending}>
                {create.isPending ? "Creating…" : "Create token"}
              </Button>
            </DialogFooter>
          </form>
        )}
      </DialogContent>
    </Dialog>
  );
}

function ScopeList({ token }: { token: SecureToken }) {
  const scopes = token.scopes ?? [];
  if (scopes.length === 0) return <span className="text-muted-foreground">full access</span>;
  return (
    <span className="flex flex-wrap gap-1">
      {scopes.map((s) => (
        <Badge key={s} variant="outline" className="font-mono">
          {s}
        </Badge>
      ))}
    </span>
  );
}

/** Settings → API tokens, with optional scopes (§12.3 POST /auth/tokens). */
export function TokensCard() {
  const api = useApi();
  const tokens = useSecureTokens();
  const queryClient = useQueryClient();
  const toast = useToast();
  const [creating, setCreating] = useState(false);
  const [revoking, setRevoking] = useState<SecureToken | null>(null);
  const [now] = useState(Date.now);

  const revoke = useMutation({
    mutationFn: api.auth.revokeToken,
    onSuccess: async () => {
      toast.success("Token revoked");
      await queryClient.invalidateQueries({ queryKey: securityKeys.tokens });
    },
  });

  // Sessions have their own card; older servers list them here too.
  const rows = (tokens.data ?? []).filter((t) => t.kind !== "session");

  return (
    <Card className="gap-0 overflow-hidden pb-0">
      <CardHeader className="border-b pb-6">
        <CardTitle>API tokens</CardTitle>
        <CardDescription>
          Personal access tokens for the wt CLI, CI and scripts, optionally limited to scopes.
        </CardDescription>
        <CardAction>
          <Button size="sm" onClick={() => setCreating(true)}>
            <Plus aria-hidden="true" />
            New token
          </Button>
        </CardAction>
      </CardHeader>
      {tokens.isPending ? (
        <TableSkeleton label="Loading tokens" rows={2} />
      ) : tokens.isError ? (
        <QueryError error={tokens.error} onRetry={() => void tokens.refetch()} title="Could not load tokens" />
      ) : rows.length === 0 ? (
        <EmptyState icon={KeyRound} title="No API tokens">
          Create one to use <code className="font-mono">wt</code> from CI or scripts.
        </EmptyState>
      ) : (
        <Table aria-label="API tokens">
          <TableHeader>
            <TableRow className="hover:bg-transparent">
              <TableHead className="pl-6">Name</TableHead>
              <TableHead className="hidden md:table-cell">Scopes</TableHead>
              <TableHead className="hidden sm:table-cell">Created</TableHead>
              <TableHead className="hidden sm:table-cell">Last used</TableHead>
              <TableHead>Expires</TableHead>
              <TableHead>
                <span className="sr-only">Actions</span>
              </TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {rows.map((t) => {
              const expired = t.expires_at ? Date.parse(t.expires_at) < now : false;
              return (
                <TableRow key={t.id}>
                  <TableCell className="pl-6 font-medium">
                    <span className={expired ? "line-through" : undefined}>{t.name}</span>
                    {expired && (
                      <Badge variant="outline" className="ml-2">
                        expired
                      </Badge>
                    )}
                  </TableCell>
                  <TableCell className="hidden max-w-64 text-xs md:table-cell">
                    <ScopeList token={t} />
                  </TableCell>
                  <TableCell className="hidden text-muted-foreground sm:table-cell">
                    <When iso={t.created_at} />
                  </TableCell>
                  <TableCell className="hidden text-muted-foreground sm:table-cell">
                    <When iso={t.last_used_at} />
                  </TableCell>
                  <TableCell className="text-muted-foreground">
                    <When iso={t.expires_at} />
                  </TableCell>
                  <TableCell className="w-10 pr-4 text-right">
                    <Button
                      variant="ghost"
                      size="icon-sm"
                      aria-label={`Revoke ${t.name}`}
                      onClick={() => setRevoking(t)}
                    >
                      <Trash2 aria-hidden="true" />
                    </Button>
                  </TableCell>
                </TableRow>
              );
            })}
          </TableBody>
        </Table>
      )}
      <CreateTokenDialog open={creating} onOpenChange={setCreating} />
      <ConfirmDialog
        open={revoking !== null}
        onOpenChange={(open) => !open && setRevoking(null)}
        title={`Revoke “${revoking?.name ?? ""}”?`}
        description="Anything using this token stops working immediately. This cannot be undone."
        confirmLabel="Revoke"
        pending={revoke.isPending}
        onConfirm={() => revoke.mutateAsync(revoking?.id ?? "")}
      />
    </Card>
  );
}
