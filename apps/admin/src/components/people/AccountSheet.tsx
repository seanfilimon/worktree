import { useQueries } from "@tanstack/react-query";
import { Building2 } from "lucide-react";
import { Link } from "react-router";
import { useApi } from "@/api/context";
import { AccountAttributesSection } from "@/components/abac/AccountAttributesSection";
import { keys, useTenants } from "@/api/queries";
import type { Account, Role, Tenant } from "@/api/types";
import { Badge } from "@/components/ui/badge";
import { Separator } from "@/components/ui/separator";
import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from "@/components/ui/sheet";
import { Skeleton } from "@/components/ui/skeleton";
import { absoluteTime, relativeTime } from "@/lib/format";
import { tenantPath } from "@/lib/paths";
import { RoleBadge, TenantKindBadge } from "./badges";
import { EmptyState, QueryError } from "./states";
import { UserAvatar } from "./UserAvatar";

/**
 * There is no per-account membership endpoint, so memberships are collected
 * from the member lists of every tenant (server admins can read them all).
 */
function useMembershipsOf(account: Account | null) {
  const api = useApi();
  const tenants = useTenants();
  const list = account ? (tenants.data ?? []) : [];
  const members = useQueries({
    queries: list.map((t) => ({ queryKey: keys.members(t.slug), queryFn: () => api.tenants.members(t.slug) })),
  });
  const memberships: { tenant: Tenant; role: Role }[] = [];
  list.forEach((tenant, i) => {
    const m = members[i]?.data?.find((x) => x.account.id === account?.id);
    if (m) memberships.push({ tenant, role: m.role });
  });
  return {
    memberships,
    pending: tenants.isPending || members.some((m) => m.isPending),
    error: tenants.error ?? members.find((m) => m.isError)?.error ?? null,
    retry: () => {
      void tenants.refetch();
      for (const m of members) if (m.isError) void m.refetch();
    },
    personal: tenants.data?.find((t) => t.id === account?.tenant_id),
  };
}

export function AccountSheet({ account, onClose }: { account: Account | null; onClose: () => void }) {
  const { memberships, pending, error, retry, personal } = useMembershipsOf(account);

  return (
    <Sheet open={account !== null} onOpenChange={(open) => !open && onClose()}>
      <SheetContent className="w-full overflow-y-auto sm:max-w-md">
        {account && (
          <>
            <SheetHeader className="flex-row items-center gap-3">
              <UserAvatar name={account.display_name || account.username} size="lg" />
              <div className="min-w-0">
                <SheetTitle className={account.disabled ? "line-through" : undefined}>{account.username}</SheetTitle>
                <SheetDescription>{account.display_name || account.email}</SheetDescription>
              </div>
            </SheetHeader>
            <div className="flex flex-col gap-5 px-4 pb-6">
              <div className="flex flex-wrap gap-1.5">
                {account.is_admin && <Badge>Server admin</Badge>}
                {account.disabled ? <Badge variant="outline">Disabled</Badge> : <Badge variant="outline">Active</Badge>}
              </div>
              <dl className="grid grid-cols-[max-content_1fr] gap-x-4 gap-y-2 text-sm">
                <dt className="text-muted-foreground">Email</dt>
                <dd className="break-all">{account.email}</dd>
                <dt className="text-muted-foreground">Personal tenant</dt>
                <dd className="font-mono">{personal ? <Link to={tenantPath(personal.slug)} className="hover:underline">{personal.slug}</Link> : "—"}</dd>
                <dt className="text-muted-foreground">Created</dt>
                <dd>
                  <time dateTime={account.created_at} title={absoluteTime(account.created_at)}>
                    {relativeTime(account.created_at)}
                  </time>
                </dd>
                <dt className="text-muted-foreground">Account id</dt>
                <dd className="font-mono text-xs break-all">{account.id}</dd>
              </dl>
              <Separator />
              <section aria-labelledby="account-memberships" className="flex flex-col gap-2">
                <h3 id="account-memberships" className="text-sm font-semibold">
                  Memberships
                </h3>
                {error ? (
                  <QueryError error={error} onRetry={retry} title="Could not load memberships" />
                ) : pending ? (
                  <div role="status" aria-label="Loading memberships" className="flex flex-col gap-2">
                    <Skeleton className="h-9 w-full" />
                    <Skeleton className="h-9 w-full" />
                  </div>
                ) : memberships.length === 0 ? (
                  <EmptyState icon={Building2} title="No memberships" className="py-6" />
                ) : (
                  <ul aria-label="Memberships" className="flex flex-col divide-y rounded-md border">
                    {memberships.map(({ tenant, role }) => (
                      <li key={tenant.id} className="flex items-center gap-3 px-3 py-2">
                        <UserAvatar name={tenant.name || tenant.slug} size="sm" square />
                        <Link to={tenantPath(tenant.slug)} className="min-w-0 flex-1 truncate font-mono text-sm hover:underline">
                          {tenant.slug}
                        </Link>
                        <TenantKindBadge kind={tenant.kind} />
                        <RoleBadge role={role} />
                      </li>
                    ))}
                  </ul>
                )}
              </section>
              <Separator />
              <AccountAttributesSection username={account.username} />
            </div>
          </>
        )}
      </SheetContent>
    </Sheet>
  );
}
