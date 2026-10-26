import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useState, type FormEvent } from "react";
import { useNavigate } from "react-router";
import { keys } from "@/api/queries";
import { SLUG_RE, useLifecycleApi, type TenantV08 } from "@/api/tenantLifecycle";
import { FormError } from "@/components/people/states";
import { Button } from "@/components/ui/button";
import { Dialog, DialogClose, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { absoluteTime } from "@/lib/format";
import { useToast } from "@/toast/store";

/**
 * Changes the tenant's slug (§16.4, owners). The old slug stays reserved
 * for 90 days and answers "tenant moved"; worktree remotes are updated
 * with `wt tenant switch NEW`.
 */
export function TenantRenameDialog({ tenant, open, onOpenChange }: { tenant: TenantV08; open: boolean; onOpenChange: (open: boolean) => void }) {
  const api = useLifecycleApi();
  const queryClient = useQueryClient();
  const toast = useToast();
  const navigate = useNavigate();
  const [slug, setSlug] = useState("");
  const [submitted, setSubmitted] = useState(false);
  const trimmed = slug.trim();
  const error =
    trimmed === ""
      ? "Required."
      : !SLUG_RE.test(trimmed)
        ? "1–63 lowercase letters, digits and '-', not starting or ending with '-'."
        : trimmed === tenant.slug
          ? "That is the current slug."
          : undefined;

  const rename = useMutation({
    mutationFn: (value: string) => api.rename(tenant.slug, value),
    meta: { inlineError: true },
    onSuccess: async (res) => {
      queryClient.setQueryData(keys.tenant(res.tenant.slug), res.tenant);
      queryClient.removeQueries({ queryKey: keys.tenant(tenant.slug) });
      toast.success("Tenant renamed", `${tenant.slug} → ${res.tenant.slug}; the old slug is reserved until ${absoluteTime(res.reservation.expires_at)}.`);
      close(false);
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: keys.tenants }),
        queryClient.invalidateQueries({ queryKey: keys.me }),
      ]);
      void navigate(`/tenants/${encodeURIComponent(res.tenant.slug)}?tab=settings`, { replace: true });
    },
  });

  function close(next: boolean) {
    if (!next) {
      setSlug("");
      setSubmitted(false);
      rename.reset();
    }
    onOpenChange(next);
  }

  const onSubmit = (e: FormEvent) => {
    e.preventDefault();
    setSubmitted(true);
    if (!error) rename.mutate(trimmed);
  };
  const showError = submitted && error;

  return (
    <Dialog open={open} onOpenChange={close}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Change slug</DialogTitle>
          <DialogDescription>
            <span className="font-mono text-foreground">{tenant.slug}</span> is part of every remote URL and policy resource.
          </DialogDescription>
        </DialogHeader>
        <form onSubmit={onSubmit} noValidate aria-label="Change slug" className="flex flex-col gap-4">
          <div className="grid gap-2">
            <Label htmlFor="tenant-new-slug">New slug</Label>
            <Input
              id="tenant-new-slug"
              className="font-mono"
              value={slug}
              placeholder={`${tenant.slug}-new`}
              aria-invalid={showError ? true : undefined}
              aria-describedby={showError ? "tenant-new-slug-error" : undefined}
              onChange={(e) => setSlug(e.target.value)}
            />
            {showError && (
              <p id="tenant-new-slug-error" className="text-sm font-medium">
                {error}
              </p>
            )}
          </div>
          <ul className="list-disc space-y-1 pl-5 text-sm text-muted-foreground">
            <li>The old slug stays reserved for 90 days; requests naming it are told the new slug.</li>
            <li>Policies, license grantees and tenant grants are rewritten automatically.</li>
            <li>
              Clients update their remotes with <span className="font-mono text-foreground">wt tenant switch NEW_SLUG</span>.
            </li>
          </ul>
          <FormError error={rename.error} />
          <DialogFooter>
            <DialogClose asChild>
              <Button type="button" variant="outline">
                Cancel
              </Button>
            </DialogClose>
            <Button type="submit" disabled={rename.isPending}>
              {rename.isPending ? "Renaming…" : "Change slug"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
