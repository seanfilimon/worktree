import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useState, type FormEvent } from "react";
import { useApi } from "@/api/context";
import { keys } from "@/api/queries";
import type { Tenant, TenantPlan, UpdateTenantRequest } from "@/api/types";
import { useAuth } from "@/auth/context";
import { PlanBadge } from "@/components/people/badges";
import { FormError } from "@/components/people/states";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardFooter, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { useToast } from "@/toast/store";

const PLANS: TenantPlan[] = ["free", "pro", "enterprise"];
const MAX_NAME = 100;

function useUpdateTenant(tenant: Tenant) {
  const api = useApi();
  const queryClient = useQueryClient();
  const toast = useToast();
  return useMutation({
    mutationFn: (body: UpdateTenantRequest) => api.tenants.update(tenant.slug, body),
    meta: { inlineError: true },
    onSuccess: async (updated) => {
      queryClient.setQueryData(keys.tenant(tenant.slug), updated);
      toast.success("Tenant updated", updated.name);
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: keys.tenants }),
        queryClient.invalidateQueries({ queryKey: keys.me }),
      ]);
    },
  });
}

function GeneralCard({ tenant, editable }: { tenant: Tenant; editable: boolean }) {
  const update = useUpdateTenant(tenant);
  const [name, setName] = useState(tenant.name);
  const trimmed = name.trim();
  const error = trimmed === "" ? "Required." : trimmed.length > MAX_NAME ? `At most ${MAX_NAME} characters.` : undefined;
  const dirty = trimmed !== tenant.name;

  const onSubmit = (e: FormEvent) => {
    e.preventDefault();
    if (!error && dirty) update.mutate({ name: trimmed });
  };

  return (
    <Card>
      <form onSubmit={onSubmit} aria-label="Tenant name" noValidate className="flex flex-col gap-6">
        <CardHeader>
          <CardTitle>General</CardTitle>
          <CardDescription>The slug is permanent: it is part of every remote URL.</CardDescription>
        </CardHeader>
        <CardContent className="grid gap-4 sm:grid-cols-2">
          <div className="grid gap-2">
            <Label htmlFor="tenant-name">Display name</Label>
            <Input
              id="tenant-name"
              value={name}
              disabled={!editable}
              aria-invalid={error ? true : undefined}
              aria-describedby={error ? "tenant-name-error" : undefined}
              onChange={(e) => setName(e.target.value)}
            />
            {error && (
              <p id="tenant-name-error" className="text-sm font-medium">
                {error}
              </p>
            )}
          </div>
          <div className="grid gap-2">
            <Label htmlFor="tenant-slug">Slug</Label>
            <Input id="tenant-slug" value={tenant.slug} readOnly disabled className="font-mono" />
          </div>
          <div className="sm:col-span-2">
            <FormError error={update.error} />
          </div>
        </CardContent>
        {editable && (
          <CardFooter className="justify-end gap-2 border-t pt-6">
            <Button type="button" variant="outline" disabled={!dirty || update.isPending} onClick={() => setName(tenant.name)}>
              Reset
            </Button>
            <Button type="submit" disabled={!dirty || Boolean(error) || update.isPending}>
              {update.isPending ? "Saving…" : "Save"}
            </Button>
          </CardFooter>
        )}
      </form>
    </Card>
  );
}

function PlanCard({ tenant }: { tenant: Tenant }) {
  const update = useUpdateTenant(tenant);
  const [plan, setPlan] = useState<TenantPlan>(tenant.plan);

  return (
    <Card>
      <CardHeader>
        <CardTitle>Plan</CardTitle>
        <CardDescription>Only server admins can change a tenant's plan.</CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        <div className="flex flex-wrap items-center gap-3">
          <Select value={plan} onValueChange={(v) => setPlan(v as TenantPlan)}>
            <SelectTrigger aria-label="Plan" className="w-44 capitalize">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {PLANS.map((p) => (
                <SelectItem key={p} value={p} className="capitalize">
                  {p}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          <Button disabled={plan === tenant.plan || update.isPending} onClick={() => update.mutate({ plan })}>
            {update.isPending ? "Saving…" : "Change plan"}
          </Button>
        </div>
        <FormError error={update.error} />
      </CardContent>
    </Card>
  );
}

export function TenantSettingsTab({ tenant, editable }: { tenant: Tenant; editable: boolean }) {
  const { isAdmin } = useAuth();
  return (
    <div className="flex flex-col gap-4">
      {!editable && (
        <p role="note" className="rounded-md border bg-muted/50 px-4 py-3 text-sm text-muted-foreground">
          You can view these settings. Tenant owners and admins can change them.
        </p>
      )}
      <GeneralCard key={tenant.name} tenant={tenant} editable={editable} />
      {isAdmin ? (
        <PlanCard key={tenant.plan} tenant={tenant} />
      ) : (
        <Card>
          <CardHeader>
            <CardTitle>Plan</CardTitle>
            <CardDescription>Contact a server admin to change the plan.</CardDescription>
          </CardHeader>
          <CardContent>
            <PlanBadge plan={tenant.plan} />
          </CardContent>
        </Card>
      )}
    </div>
  );
}
