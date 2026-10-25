import { useMutation, useQueryClient } from "@tanstack/react-query";
import { KeyRound, Plus, Trash2 } from "lucide-react";
import { useState, type FormEvent } from "react";
import { isNotImplemented, normalizePublicKey, opsKeys, useOpsApi, useSigningKeys, type SigningKey } from "@/api/v05ops";
import { ConfirmDialog } from "@/components/people/ConfirmDialog";
import { EmptyState, FormError, QueryError, TableSkeleton } from "@/components/people/states";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardAction, CardDescription, CardFooter, CardHeader, CardTitle } from "@/components/ui/card";
import { Dialog, DialogClose, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { Textarea } from "@/components/ui/textarea";
import { absoluteTime, relativeTime } from "@/lib/format";
import { cn } from "@/lib/utils";
import { NotImplementedNotice } from "@/pages/tenants/opsShared";
import { useToast } from "@/toast/store";

const MAX_NAME = 64;

function When({ iso, empty = "never" }: { iso: string | undefined; empty?: string }) {
  if (!iso) return <span className="text-muted-foreground">{empty}</span>;
  return (
    <time dateTime={iso} title={absoluteTime(iso)}>
      {relativeTime(iso)}
    </time>
  );
}

function AddKeyDialog({ open, onOpenChange }: { open: boolean; onOpenChange: (open: boolean) => void }) {
  const ops = useOpsApi();
  const queryClient = useQueryClient();
  const toast = useToast();
  const [name, setName] = useState("");
  const [key, setKey] = useState("");
  const [submitted, setSubmitted] = useState(false);
  const trimmed = name.trim();
  const nameError = trimmed === "" ? "Required." : trimmed.length > MAX_NAME ? `At most ${MAX_NAME} characters.` : undefined;
  const parsed = normalizePublicKey(key);

  const add = useMutation({
    mutationFn: ops.signingKeys.add,
    meta: { inlineError: true },
    onSuccess: async (k) => {
      toast.success("Signing key added", k.fingerprint || k.name);
      changeOpen(false);
      await queryClient.invalidateQueries({ queryKey: opsKeys.signingKeys });
    },
  });

  function changeOpen(next: boolean) {
    if (!next) {
      setName("");
      setKey("");
      setSubmitted(false);
      add.reset();
    }
    onOpenChange(next);
  }

  const onSubmit = (e: FormEvent) => {
    e.preventDefault();
    setSubmitted(true);
    if (!nameError && parsed.key) add.mutate({ name: trimmed, public_key: parsed.key });
  };

  return (
    <Dialog open={open} onOpenChange={changeOpen}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Add signing key</DialogTitle>
          <DialogDescription>
            Register an Ed25519 public key so snapshots you sign verify on push. Locally,{" "}
            <span className="font-mono">wt identity show</span> prints it and <span className="font-mono">wt identity register</span>{" "}
            does this for you.
          </DialogDescription>
        </DialogHeader>
        <form onSubmit={onSubmit} noValidate className="flex flex-col gap-4" aria-label="Add signing key">
          <div className="grid gap-2">
            <Label htmlFor="signing-key-name">Name</Label>
            <Input
              id="signing-key-name"
              value={name}
              placeholder="laptop"
              autoComplete="off"
              aria-invalid={submitted && nameError ? true : undefined}
              aria-describedby={submitted && nameError ? "signing-key-name-error" : undefined}
              onChange={(e) => setName(e.target.value)}
            />
            {submitted && nameError && (
              <p id="signing-key-name-error" className="text-sm font-medium">
                {nameError}
              </p>
            )}
          </div>
          <div className="grid gap-2">
            <Label htmlFor="signing-key-public">Public key (base64)</Label>
            <Textarea
              id="signing-key-public"
              value={key}
              rows={3}
              spellCheck={false}
              placeholder="32-byte Ed25519 public key, base64"
              aria-invalid={submitted && parsed.error ? true : undefined}
              aria-describedby={submitted && parsed.error ? "signing-key-public-error" : undefined}
              onChange={(e) => setKey(e.target.value)}
              className="font-mono text-xs"
            />
            {submitted && parsed.error && (
              <p id="signing-key-public-error" className="text-sm font-medium">
                {parsed.error}
              </p>
            )}
          </div>
          {isNotImplemented(add.error) ? <NotImplementedNotice feature="Signing keys" /> : <FormError error={add.error} />}
          <DialogFooter>
            <DialogClose asChild>
              <Button type="button" variant="outline">
                Cancel
              </Button>
            </DialogClose>
            <Button type="submit" disabled={add.isPending}>
              {add.isPending ? "Adding…" : "Add key"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

/** Settings → Security: the caller's registered snapshot-signing keys. */
export function SigningKeysCard() {
  const ops = useOpsApi();
  const keysQuery = useSigningKeys();
  const queryClient = useQueryClient();
  const toast = useToast();
  const [adding, setAdding] = useState(false);
  const [revoking, setRevoking] = useState<SigningKey | null>(null);
  const unavailable = isNotImplemented(keysQuery.error);

  const revoke = useMutation({
    mutationFn: ops.signingKeys.revoke,
    onSuccess: async () => {
      toast.success("Signing key revoked");
      await queryClient.invalidateQueries({ queryKey: opsKeys.signingKeys });
    },
  });

  return (
    <Card className="gap-0 overflow-hidden pb-0">
      <CardHeader className="border-b pb-6">
        <CardTitle>Signing keys</CardTitle>
        <CardDescription>Ed25519 public keys that verify the snapshots you sign. Revoked keys stop verifying new pushes.</CardDescription>
        {!unavailable && (
          <CardAction>
            <Button size="sm" variant="outline" onClick={() => setAdding(true)}>
              <Plus aria-hidden="true" />
              Add key
            </Button>
          </CardAction>
        )}
      </CardHeader>
      {unavailable ? (
        <div className="p-4">
          <NotImplementedNotice feature="Snapshot signing" />
        </div>
      ) : keysQuery.isPending ? (
        <TableSkeleton label="Loading signing keys" rows={2} />
      ) : keysQuery.isError ? (
        <QueryError error={keysQuery.error} onRetry={() => void keysQuery.refetch()} title="Could not load signing keys" />
      ) : keysQuery.data.length === 0 ? (
        <EmptyState icon={KeyRound} title="No signing keys">
          Generate one with <span className="font-mono">wt identity generate</span>, then add its public key here.
        </EmptyState>
      ) : (
        <Table aria-label="Signing keys">
          <TableHeader>
            <TableRow className="hover:bg-transparent">
              <TableHead className="pl-6">Name</TableHead>
              <TableHead>Fingerprint</TableHead>
              <TableHead className="hidden sm:table-cell">Added</TableHead>
              <TableHead className="hidden md:table-cell">Last used</TableHead>
              <TableHead>
                <span className="sr-only">Actions</span>
              </TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {keysQuery.data.map((k) => {
              const revoked = Boolean(k.revoked_at);
              return (
                <TableRow key={k.id} className={cn(revoked && "text-muted-foreground")}>
                  <TableCell className="pl-6">
                    <div className="flex flex-wrap items-center gap-1.5">
                      <span className={cn("font-medium", revoked && "line-through")}>{k.name}</span>
                      <Badge variant="outline" className="font-mono uppercase">
                        {k.algorithm || "ed25519"}
                      </Badge>
                      {revoked && <Badge>Revoked</Badge>}
                    </div>
                  </TableCell>
                  <TableCell className="max-w-56 truncate font-mono text-xs" title={k.fingerprint}>
                    {k.fingerprint}
                  </TableCell>
                  <TableCell className="hidden text-muted-foreground sm:table-cell">
                    <When iso={k.created_at} />
                  </TableCell>
                  <TableCell className="hidden text-muted-foreground md:table-cell">
                    <When iso={k.last_used_at} />
                  </TableCell>
                  <TableCell className="w-10 pr-4 text-right">
                    {!revoked && (
                      <Button variant="ghost" size="icon-sm" aria-label={`Revoke signing key: ${k.name}`} onClick={() => setRevoking(k)}>
                        <Trash2 aria-hidden="true" />
                      </Button>
                    )}
                  </TableCell>
                </TableRow>
              );
            })}
          </TableBody>
        </Table>
      )}
      <CardFooter className="border-t py-4 text-xs text-muted-foreground">
        Protected branches can require every new snapshot to carry a verified signature.
      </CardFooter>
      <AddKeyDialog open={adding} onOpenChange={setAdding} />
      <ConfirmDialog
        open={revoking !== null}
        onOpenChange={(open) => !open && setRevoking(null)}
        title="Revoke this signing key?"
        description={`Snapshots signed with “${revoking?.name ?? ""}” no longer verify on push. This cannot be undone; add the key again to re-register it.`}
        confirmLabel="Revoke"
        pending={revoke.isPending}
        onConfirm={() => revoke.mutateAsync(revoking?.id ?? "")}
      />
    </Card>
  );
}
