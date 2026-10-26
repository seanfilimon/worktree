import { useMutation, useQueryClient } from "@tanstack/react-query";
import { RotateCcw, Timer, Trash2 } from "lucide-react";
import { useEffect, useState } from "react";
import { keys } from "@/api/queries";
import { countdown, lifecycleKeys, useLifecycleApi, useTenantDeletion, type TenantDeletion, type TenantV08 } from "@/api/tenantLifecycle";
import { FormError } from "@/components/people/states";
import {
  AlertDialog,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Skeleton } from "@/components/ui/skeleton";
import { absoluteTime } from "@/lib/format";
import { useToast } from "@/toast/store";

/** Current time, refreshed every `ms`. */
function useNow(ms: number): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const id = setInterval(() => setNow(Date.now()), ms);
    return () => clearInterval(id);
  }, [ms]);
  return now;
}

function Countdown({ purgeAt }: { purgeAt: string }) {
  const now = useNow(1000);
  const target = Date.parse(purgeAt);
  return (
    <span role="timer" aria-label="Time until purge" className="font-mono text-2xl font-semibold tabular-nums">
      {Number.isNaN(target) ? "—" : countdown(target, now)}
    </span>
  );
}

function DeleteDialog({ tenant, open, onOpenChange }: { tenant: TenantV08; open: boolean; onOpenChange: (open: boolean) => void }) {
  const api = useLifecycleApi();
  const queryClient = useQueryClient();
  const toast = useToast();
  const [typed, setTyped] = useState("");
  const remove = useMutation({
    mutationFn: () => api.remove(tenant.slug),
    meta: { inlineError: true },
    onSuccess: async (d) => {
      queryClient.setQueryData<TenantDeletion | null>(lifecycleKeys.deletion(tenant.slug), d);
      toast.success("Deletion scheduled", `${tenant.slug} is purged ${absoluteTime(d.purge_at)}.`);
      change(false);
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: keys.tenant(tenant.slug) }),
        queryClient.invalidateQueries({ queryKey: keys.tenants }),
      ]);
    },
  });

  function change(next: boolean) {
    if (!next) {
      setTyped("");
      remove.reset();
    }
    onOpenChange(next);
  }

  return (
    <AlertDialog open={open} onOpenChange={change}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>Delete tenant</AlertDialogTitle>
          <AlertDialogDescription>
            <span className="font-mono text-foreground">{tenant.slug}</span> becomes read-only at once and its service-account keys are
            revoked. After the 30-day recovery window its trees, members, teams and settings are deleted for good.
          </AlertDialogDescription>
        </AlertDialogHeader>
        <div className="grid gap-2">
          <Label htmlFor="tenant-delete-confirm">
            Type <span className="font-mono">{tenant.slug}</span> to confirm
          </Label>
          <Input id="tenant-delete-confirm" className="font-mono" value={typed} autoComplete="off" onChange={(e) => setTyped(e.target.value)} />
        </div>
        <FormError error={remove.error} />
        <AlertDialogFooter>
          <AlertDialogCancel disabled={remove.isPending}>Cancel</AlertDialogCancel>
          <Button disabled={typed !== tenant.slug || remove.isPending} onClick={() => remove.mutate()}>
            {remove.isPending ? "Deleting…" : "Delete tenant"}
          </Button>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}

/**
 * Danger zone (§16.4): delete the tenant (owners; 30-day recovery window)
 * or, while a deletion is scheduled, a countdown to the purge and Restore.
 */
export function TenantDangerZone({ tenant, owner }: { tenant: TenantV08; owner: boolean }) {
  const api = useLifecycleApi();
  const queryClient = useQueryClient();
  const toast = useToast();
  const deletion = useTenantDeletion(tenant.slug);
  const [deleting, setDeleting] = useState(false);
  const restore = useMutation({
    mutationFn: () => api.restore(tenant.slug),
    meta: { inlineError: true },
    onSuccess: async (updated) => {
      queryClient.setQueryData(keys.tenant(tenant.slug), updated);
      queryClient.setQueryData<TenantDeletion | null>(lifecycleKeys.deletion(tenant.slug), null);
      toast.success("Tenant restored", tenant.slug);
      await queryClient.invalidateQueries({ queryKey: keys.tenants });
    },
  });

  if (tenant.kind === "user") {
    return (
      <Card>
        <CardHeader>
          <CardTitle>Danger zone</CardTitle>
          <CardDescription>Personal tenants belong to their account and cannot be renamed or deleted on their own.</CardDescription>
        </CardHeader>
      </Card>
    );
  }

  const scheduled = deletion.data ?? null;
  return (
    <Card className="border-foreground">
      <CardHeader>
        <CardTitle>Danger zone</CardTitle>
        <CardDescription>
          {owner ? "Deleting a tenant can be undone for 30 days." : "Only owners and server admins can delete or restore this tenant."}
        </CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        {deletion.isPending ? (
          <Skeleton className="h-10 w-64" />
        ) : scheduled ? (
          <div role="group" aria-label="Scheduled deletion" className="flex flex-col gap-3 rounded-md border border-dashed p-4">
            <div className="flex flex-wrap items-center gap-3">
              <Timer aria-hidden="true" className="size-5" />
              <span className="text-sm font-medium">Purged in</span>
              <Countdown purgeAt={scheduled.purge_at} />
            </div>
            <p className="text-sm text-muted-foreground">
              Scheduled by {scheduled.requested_by} on {absoluteTime(scheduled.requested_at)}; purge at {absoluteTime(scheduled.purge_at)}. The tenant is
              read-only until then.
              {scheduled.revoked_keys > 0 && ` ${scheduled.revoked_keys} service-account key(s) were revoked and stay revoked.`}
            </p>
            {owner && (
              <div>
                <Button disabled={restore.isPending} onClick={() => restore.mutate()}>
                  <RotateCcw aria-hidden="true" />
                  {restore.isPending ? "Restoring…" : "Restore tenant"}
                </Button>
              </div>
            )}
            <FormError error={restore.error} />
          </div>
        ) : (
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div className="text-sm">
              <p className="font-medium">Delete this tenant</p>
              <p className="text-muted-foreground">Suspends it now and purges it after the recovery window.</p>
            </div>
            {owner && (
              <Button variant="outline" onClick={() => setDeleting(true)}>
                <Trash2 aria-hidden="true" />
                Delete tenant
              </Button>
            )}
          </div>
        )}
        {deletion.isError && <FormError error={deletion.error} />}
      </CardContent>
      {owner && <DeleteDialog tenant={tenant} open={deleting} onOpenChange={setDeleting} />}
    </Card>
  );
}
