import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Ban, PauseCircle, PlayCircle } from "lucide-react";
import { useState, type FormEvent } from "react";
import { keys } from "@/api/queries";
import type { Tenant } from "@/api/types";
import { isNotImplemented, tenantStatus, useOpsApi, type TenantStatus, type TenantV05 } from "@/api/v05ops";
import { FormError } from "@/components/people/states";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Dialog, DialogClose, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import { useToast } from "@/toast/store";
import { NotImplementedNotice, TenantStatusBadge } from "./opsShared";

const MAX_REASON = 500;

interface Transition {
  to: TenantStatus;
  label: string;
  title: string;
  effect: string;
  needsReason: boolean;
}

const SUSPEND: Transition = {
  to: "suspended",
  label: "Suspend",
  title: "Suspend tenant",
  effect: "Members keep read access; every write (push, merge, settings) is denied. Nothing is revoked.",
  needsReason: true,
};
const DEACTIVATE: Transition = {
  to: "deactivated",
  label: "Deactivate",
  title: "Deactivate tenant",
  effect: "All access is denied for everyone except server admins until the tenant is reactivated.",
  needsReason: true,
};
const RESUME: Transition = {
  to: "active",
  label: "Resume",
  title: "Resume tenant",
  effect: "Restores normal access for members, teams and service accounts.",
  needsReason: false,
};

function transitionsFrom(status: TenantStatus): Transition[] {
  if (status === "active") return [SUSPEND, DEACTIVATE];
  if (status === "suspended") return [RESUME, DEACTIVATE];
  return [{ ...RESUME, label: "Reactivate", title: "Reactivate tenant" }];
}

const ICONS: Record<TenantStatus, typeof Ban> = { active: PlayCircle, suspended: PauseCircle, deactivated: Ban };

function StatusDialog({ tenant, transition, onOpenChange }: { tenant: Tenant; transition: Transition | null; onOpenChange: (open: boolean) => void }) {
  const ops = useOpsApi();
  const queryClient = useQueryClient();
  const toast = useToast();
  const [reason, setReason] = useState("");
  const trimmed = reason.trim();
  const error =
    transition?.needsReason && trimmed === ""
      ? "Give a reason; members and admins see it."
      : trimmed.length > MAX_REASON
        ? `At most ${MAX_REASON} characters.`
        : undefined;
  const [submitted, setSubmitted] = useState(false);

  const change = useMutation({
    mutationFn: (t: Transition) => ops.tenants.setStatus(tenant.slug, { status: t.to, status_reason: t.to === "active" ? "" : trimmed }),
    meta: { inlineError: true },
    onSuccess: async (updated, t) => {
      queryClient.setQueryData<TenantV05>(keys.tenant(tenant.slug), (old) => ({ ...(old ?? tenant), ...updated, status: updated.status ?? t.to }));
      toast.success(`Tenant ${t.to === "active" ? "resumed" : t.to}`, tenant.slug);
      close(false);
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: keys.tenants }),
        queryClient.invalidateQueries({ queryKey: keys.me }),
      ]);
    },
  });

  function close(open: boolean) {
    if (!open) {
      setReason("");
      setSubmitted(false);
      change.reset();
    }
    onOpenChange(open);
  }

  const onSubmit = (e: FormEvent) => {
    e.preventDefault();
    setSubmitted(true);
    if (transition && !error) change.mutate(transition);
  };

  const showError = submitted && error;

  return (
    <Dialog open={transition !== null} onOpenChange={close}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{transition?.title}</DialogTitle>
          <DialogDescription>
            <span className="font-mono text-foreground">{tenant.slug}</span>: {transition?.effect}
          </DialogDescription>
        </DialogHeader>
        <form onSubmit={onSubmit} noValidate className="flex flex-col gap-4" aria-label="Change tenant status">
          {transition?.needsReason && (
            <div className="grid gap-2">
              <Label htmlFor="tenant-status-reason">Reason</Label>
              <Textarea
                id="tenant-status-reason"
                value={reason}
                rows={3}
                placeholder="e.g. Billing overdue since 2026-10-01"
                aria-invalid={showError ? true : undefined}
                aria-describedby={showError ? "tenant-status-reason-error" : undefined}
                onChange={(e) => setReason(e.target.value)}
              />
              {showError && (
                <p id="tenant-status-reason-error" className="text-sm font-medium">
                  {error}
                </p>
              )}
            </div>
          )}
          <FormError error={isNotImplemented(change.error) ? null : change.error} />
          {isNotImplemented(change.error) && <NotImplementedNotice feature="Tenant status" />}
          <DialogFooter>
            <DialogClose asChild>
              <Button type="button" variant="outline">
                Cancel
              </Button>
            </DialogClose>
            <Button type="submit" disabled={change.isPending}>
              {change.isPending ? "Saving…" : transition?.label}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

/** Tenant lifecycle (active / suspended / deactivated); server admins change it. */
export function TenantStatusCard({ tenant, editable }: { tenant: TenantV05; editable: boolean }) {
  const status = tenantStatus(tenant);
  const [transition, setTransition] = useState<Transition | null>(null);

  return (
    <Card>
      <CardHeader>
        <CardTitle>Status</CardTitle>
        <CardDescription>
          {editable
            ? "Suspended tenants are read-only; deactivated tenants are closed to everyone but server admins."
            : "Only server admins can suspend or deactivate a tenant."}
        </CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        <div className="flex flex-wrap items-center gap-3" aria-label="Tenant status" role="group">
          <TenantStatusBadge status={status} />
          {status !== "active" && tenant.status_reason && (
            <p className="text-sm">
              <span className="text-muted-foreground">Reason: </span>
              {tenant.status_reason}
            </p>
          )}
        </div>
        {editable && (
          <div className="flex flex-wrap gap-2">
            {transitionsFrom(status).map((t) => {
              const Icon = ICONS[t.to];
              return (
                <Button key={t.to} variant={t.to === "active" ? "default" : "outline"} onClick={() => setTransition(t)}>
                  <Icon aria-hidden="true" />
                  {t.label}
                </Button>
              );
            })}
          </div>
        )}
      </CardContent>
      <StatusDialog tenant={tenant} transition={transition} onOpenChange={(open) => !open && setTransition(null)} />
    </Card>
  );
}
