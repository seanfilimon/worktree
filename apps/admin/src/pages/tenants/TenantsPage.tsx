import { useQueries } from "@tanstack/react-query";
import { Building2, Ellipsis, FolderTree, Plus, Settings, Users } from "lucide-react";
import { useState, type MouseEvent } from "react";
import { Link, useNavigate } from "react-router";
import { useApi } from "@/api/context";
import { keys, useTenants } from "@/api/queries";
import { useAuth } from "@/auth/context";
import { PlanBadge, RoleBadge, TenantKindBadge } from "@/components/people/badges";
import { SearchInput } from "@/components/people/SearchInput";
import { EmptyState, QueryError, TableSkeleton } from "@/components/people/states";
import { UserAvatar } from "@/components/people/UserAvatar";
import { PageHeader } from "@/components/PageHeader";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { Skeleton } from "@/components/ui/skeleton";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { absoluteTime, formatNumber, relativeTime } from "@/lib/format";
import { tenantPath } from "@/lib/paths";
import { CreateTenantDialog } from "./CreateTenantDialog";

/** A count that may still be loading, or unavailable to the caller. */
function CountCell({ value, pending }: { value: number | undefined; pending: boolean }) {
  return (
    <TableCell className="hidden text-right tabular-nums sm:table-cell">
      {pending ? (
        <Skeleton className="ml-auto h-4 w-6" />
      ) : value === undefined ? (
        <span className="text-muted-foreground" title="Not available to you">
          —
        </span>
      ) : (
        formatNumber(value)
      )}
    </TableCell>
  );
}

export function TenantsPage() {
  const api = useApi();
  const tenants = useTenants();
  const { roleIn } = useAuth();
  const navigate = useNavigate();
  const [query, setQuery] = useState("");
  const [creating, setCreating] = useState(false);

  const q = query.trim().toLowerCase();
  const rows = (tenants.data ?? []).filter((t) => !q || t.slug.includes(q) || t.name.toLowerCase().includes(q));

  const treeCounts = useQueries({
    queries: rows.map((t) => ({ queryKey: keys.tenantTrees(t.slug), queryFn: () => api.tenants.trees(t.slug) })),
  });
  const memberCounts = useQueries({
    queries: rows.map((t) => ({ queryKey: keys.members(t.slug), queryFn: () => api.tenants.members(t.slug) })),
  });

  const openRow = (e: MouseEvent, slug: string) => {
    if ((e.target as HTMLElement).closest("a, button, [role=menu]")) return;
    void navigate(tenantPath(slug));
  };

  return (
    <>
      <PageHeader
        title="Tenants"
        breadcrumbs={[{ label: "Tenants" }]}
        description="Organisations and personal namespaces that own trees."
        actions={
          <Button onClick={() => setCreating(true)}>
            <Plus aria-hidden="true" />
            New organisation
          </Button>
        }
      />
      <Card className="gap-0 overflow-hidden py-0">
        <div className="flex flex-col gap-2 border-b p-3 sm:flex-row sm:items-center sm:justify-between">
          <SearchInput label="Search tenants" placeholder="Search by slug or name…" value={query} onChange={setQuery} />
          {tenants.data && (
            <p className="text-sm text-muted-foreground">
              {rows.length === tenants.data.length ? `${tenants.data.length} tenants` : `${rows.length} of ${tenants.data.length} tenants`}
            </p>
          )}
        </div>
        {tenants.isPending ? (
          <TableSkeleton label="Loading tenants" columns={5} />
        ) : tenants.isError ? (
          <QueryError error={tenants.error} onRetry={() => void tenants.refetch()} title="Could not load tenants" />
        ) : tenants.data.length === 0 ? (
          <EmptyState
            icon={Building2}
            title="No tenants yet"
            action={
              <Button variant="outline" onClick={() => setCreating(true)}>
                <Plus aria-hidden="true" />
                New organisation
              </Button>
            }
          >
            Create an organisation to group trees and members.
          </EmptyState>
        ) : rows.length === 0 ? (
          <EmptyState icon={Building2} title="No tenants match your search" />
        ) : (
          <Table aria-label="Tenants">
            <TableHeader className="sticky top-0 z-10 bg-background">
              <TableRow className="hover:bg-transparent">
                <TableHead>Tenant</TableHead>
                <TableHead className="hidden md:table-cell">Kind</TableHead>
                <TableHead>Plan</TableHead>
                <TableHead className="hidden md:table-cell">Your role</TableHead>
                <TableHead className="hidden text-right sm:table-cell">Trees</TableHead>
                <TableHead className="hidden text-right sm:table-cell">Members</TableHead>
                <TableHead className="hidden lg:table-cell">Created</TableHead>
                <TableHead>
                  <span className="sr-only">Actions</span>
                </TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {rows.map((t, i) => {
                const role = roleIn(t.id);
                const trees = treeCounts[i];
                const members = memberCounts[i];
                const to = tenantPath(t.slug);
                return (
                  <TableRow key={t.id} className="cursor-pointer" onClick={(e) => openRow(e, t.slug)}>
                    <TableCell>
                      <div className="flex items-center gap-3">
                        <UserAvatar name={t.name || t.slug} square />
                        <div className="min-w-0">
                          <Link to={to} className="font-medium hover:underline">
                            {t.name || t.slug}
                          </Link>
                          <p className="font-mono text-xs text-muted-foreground">{t.slug}</p>
                        </div>
                      </div>
                    </TableCell>
                    <TableCell className="hidden md:table-cell">
                      <TenantKindBadge kind={t.kind} />
                    </TableCell>
                    <TableCell>
                      <PlanBadge plan={t.plan} />
                    </TableCell>
                    <TableCell className="hidden md:table-cell">
                      {role ? <RoleBadge role={role} /> : <span className="text-muted-foreground">—</span>}
                    </TableCell>
                    <CountCell value={trees?.data?.length} pending={trees?.isPending ?? true} />
                    <CountCell value={members?.data?.length} pending={members?.isPending ?? true} />
                    <TableCell className="hidden text-muted-foreground lg:table-cell">
                      <time dateTime={t.created_at} title={absoluteTime(t.created_at)}>
                        {relativeTime(t.created_at)}
                      </time>
                    </TableCell>
                    <TableCell className="w-10 text-right">
                      <DropdownMenu>
                        <DropdownMenuTrigger asChild>
                          <Button variant="ghost" size="icon-sm" aria-label={`Actions for ${t.slug}`}>
                            <Ellipsis aria-hidden="true" />
                          </Button>
                        </DropdownMenuTrigger>
                        <DropdownMenuContent align="end">
                          <DropdownMenuItem onSelect={() => void navigate(`${to}?tab=trees`)}>
                            <FolderTree aria-hidden="true" />
                            Trees
                          </DropdownMenuItem>
                          <DropdownMenuItem onSelect={() => void navigate(`${to}?tab=members`)}>
                            <Users aria-hidden="true" />
                            Members
                          </DropdownMenuItem>
                          <DropdownMenuItem onSelect={() => void navigate(`${to}?tab=settings`)}>
                            <Settings aria-hidden="true" />
                            Settings
                          </DropdownMenuItem>
                        </DropdownMenuContent>
                      </DropdownMenu>
                    </TableCell>
                  </TableRow>
                );
              })}
            </TableBody>
          </Table>
        )}
      </Card>
      <CreateTenantDialog open={creating} onOpenChange={setCreating} />
    </>
  );
}
