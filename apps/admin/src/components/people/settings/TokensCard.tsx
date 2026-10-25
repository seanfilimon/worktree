import { useMutation, useQueryClient } from "@tanstack/react-query";
import { KeyRound, Plus, Trash2 } from "lucide-react";
import { useState, type FormEvent } from "react";
import { useApi } from "@/api/context";
import { keys, useTokens } from "@/api/queries";
import type { APIToken, CreatedToken } from "@/api/types";
import { ConfirmDialog } from "@/components/people/ConfirmDialog";
import { CopyButton } from "@/components/people/CopyButton";
import { EmptyState, FormError, QueryError, TableSkeleton } from "@/components/people/states";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardAction, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Dialog, DialogClose, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { absoluteTime, relativeTime } from "@/lib/format";
import { useToast } from "@/toast/store";

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

function CreateTokenDialog({ open, onOpenChange }: { open: boolean; onOpenChange: (open: boolean) => void }) {
  const api = useApi();
  const queryClient = useQueryClient();
  const [name, setName] = useState("");
  const [ttl, setTtl] = useState(String(24 * 30));
  const [submitted, setSubmitted] = useState(false);
  const [created, setCreated] = useState<CreatedToken | null>(null);
  const nameError = name.trim() === "" ? "Give the token a name." : name.trim().length > 100 ? "At most 100 characters." : undefined;

  const create = useMutation({
    mutationFn: api.auth.createToken,
    meta: { inlineError: true },
    onSuccess: async (res) => {
      setCreated(res);
      await queryClient.invalidateQueries({ queryKey: keys.tokens });
    },
  });

  const changeOpen = (next: boolean) => {
    if (!next) {
      // The raw token is dropped here: it can never be shown again.
      setCreated(null);
      setName("");
      setTtl(String(24 * 30));
      setSubmitted(false);
      create.reset();
    }
    onOpenChange(next);
  };

  const onSubmit = (e: FormEvent) => {
    e.preventDefault();
    setSubmitted(true);
    if (!nameError) create.mutate({ name: name.trim(), ttl_hours: Number(ttl) });
  };

  return (
    <Dialog open={open} onOpenChange={changeOpen}>
      <DialogContent>
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
            <DialogFooter>
              <DialogClose asChild>
                <Button>Done</Button>
              </DialogClose>
            </DialogFooter>
          </>
        ) : (
          <form onSubmit={onSubmit} noValidate className="flex flex-col gap-4">
            <DialogHeader>
              <DialogTitle>New API token</DialogTitle>
              <DialogDescription>For the wt CLI, CI and scripts. It acts with your permissions.</DialogDescription>
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

export function TokensCard() {
  const api = useApi();
  const tokens = useTokens();
  const queryClient = useQueryClient();
  const toast = useToast();
  const [creating, setCreating] = useState(false);
  const [revoking, setRevoking] = useState<APIToken | null>(null);
  const [now] = useState(Date.now);

  const revoke = useMutation({
    mutationFn: api.auth.revokeToken,
    onSuccess: async () => {
      toast.success("Token revoked");
      await queryClient.invalidateQueries({ queryKey: keys.tokens });
    },
  });

  return (
    <Card className="gap-0 overflow-hidden pb-0">
      <CardHeader className="border-b pb-6">
        <CardTitle>API tokens</CardTitle>
        <CardDescription>Personal access tokens for the wt CLI, CI and scripts.</CardDescription>
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
      ) : tokens.data.length === 0 ? (
        <EmptyState icon={KeyRound} title="No API tokens">
          Create one to use <code className="font-mono">wt</code> from CI or scripts.
        </EmptyState>
      ) : (
        <Table aria-label="API tokens">
          <TableHeader>
            <TableRow className="hover:bg-transparent">
              <TableHead className="pl-6">Name</TableHead>
              <TableHead className="hidden sm:table-cell">Created</TableHead>
              <TableHead className="hidden sm:table-cell">Last used</TableHead>
              <TableHead>Expires</TableHead>
              <TableHead>
                <span className="sr-only">Actions</span>
              </TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {tokens.data.map((t) => {
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
                    <Button variant="ghost" size="icon-sm" aria-label={`Revoke ${t.name}`} onClick={() => setRevoking(t)}>
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
